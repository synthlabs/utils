use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use sha2::{Digest, Sha256};
use tokio::sync::{mpsc, watch};

use crate::{
    installation, CatalogArtifact, CatalogModel, CatalogSnapshot, InstallationManifest,
    LanguagePair, ModelMutationGuard, ModelStore, Sha256Digest, StoreError,
    INSTALL_MANIFEST_FILENAME,
};

const BUFFER_SIZE: usize = 64 * 1024;
const ATTEMPTS: u32 = 3;

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("installation cancelled")]
    Cancelled,
    #[error("installation control has already been used")]
    ControlUsed,
    #[error("model not available in selected catalog: {0:?}")]
    Unavailable(LanguagePair),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("model request failed: {0}")]
    Transport(#[source] reqwest::Error),
    #[error("unexpected model HTTP status: {0}")]
    Http(u16),
    #[error("artifact integrity failure for {file}: {detail}")]
    Integrity { file: String, detail: String },
    #[error("decompression failed for {file}: {source}")]
    Decompression {
        file: String,
        #[source]
        source: std::io::Error,
    },
    #[error("installation filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("installation worker failed: {0}")]
    Worker(String),
}

impl InstallError {
    fn retryable(&self) -> bool {
        match self {
            Self::Transport(error) => {
                error.is_timeout()
                    || error.is_connect()
                    || error.is_body()
                    || error.is_request()
                    || error.is_decode()
            }
            Self::Http(status) => matches!(status, 408 | 500 | 502 | 503 | 504),
            _ => false,
        }
    }
}

/// A local, immutable selection. Creating a plan never fetches metadata or objects.
#[derive(Clone, Debug)]
pub struct InstallPlan {
    snapshot: CatalogSnapshot,
    model: CatalogModel,
}

impl InstallPlan {
    pub fn new(snapshot: &CatalogSnapshot, pair: LanguagePair) -> Result<Self, InstallError> {
        let model = snapshot
            .model(pair)
            .ok_or(InstallError::Unavailable(pair))?
            .clone();
        Ok(Self {
            snapshot: snapshot.clone(),
            model,
        })
    }
    pub fn model(&self) -> &CatalogModel {
        &self.model
    }
    pub fn download_bytes(&self) -> u64 {
        self.model.download_bytes()
    }
    pub fn installed_bytes(&self) -> u64 {
        self.model.installed_bytes()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InstallPhase {
    #[default]
    Idle,
    Preparing,
    Downloading,
    RetryWaiting,
    Verifying,
    Decompressing,
    Publishing,
    Completed,
    Cancelled,
    Failed,
}

/// A latest-state snapshot, not an event log. Current-file counters reset on retry.
#[derive(Clone, Debug, Default)]
pub struct InstallProgress {
    pub phase: InstallPhase,
    pub model_id: Option<String>,
    pub pair: Option<LanguagePair>,
    pub filename: Option<String>,
    pub file_index: usize,
    pub file_count: usize,
    pub attempt: u32,
    pub max_attempts: u32,
    pub file_download_bytes: u64,
    pub file_download_total: u64,
    pub completed_download_bytes: u64,
    pub download_total: u64,
    pub file_installed_bytes: u64,
    pub file_installed_total: u64,
    pub completed_installed_bytes: u64,
    pub installed_total: u64,
    pub reused_bytes: u64,
    pub network_bytes: u64,
    pub detail: Option<String>,
}

struct ControlState {
    cancelled: watch::Sender<bool>,
    progress: watch::Sender<InstallProgress>,
    used: AtomicBool,
    #[cfg(test)]
    worker_pause: std::sync::Mutex<Option<InstallPhase>>,
    #[cfg(test)]
    paused: AtomicBool,
    #[cfg(test)]
    cancel_on_publish: AtomicBool,
    #[cfg(test)]
    write_failure: std::sync::Mutex<Option<InstallPhase>>,
}

#[derive(Clone)]
pub struct InstallControl(Arc<ControlState>);

impl Default for InstallControl {
    fn default() -> Self {
        Self::new()
    }
}

impl InstallControl {
    pub fn new() -> Self {
        Self(Arc::new(ControlState {
            cancelled: watch::channel(false).0,
            progress: watch::channel(InstallProgress::default()).0,
            used: AtomicBool::new(false),
            #[cfg(test)]
            worker_pause: std::sync::Mutex::new(None),
            #[cfg(test)]
            paused: AtomicBool::new(false),
            #[cfg(test)]
            cancel_on_publish: AtomicBool::new(false),
            #[cfg(test)]
            write_failure: std::sync::Mutex::new(None),
        }))
    }
    pub fn cancel(&self) {
        self.0.cancelled.send_replace(true);
    }
    pub fn subscribe(&self) -> watch::Receiver<InstallProgress> {
        self.0.progress.subscribe()
    }
    pub fn progress(&self) -> InstallProgress {
        self.0.progress.borrow().clone()
    }
    fn update(&self, change: impl FnOnce(&mut InstallProgress)) {
        self.0.progress.send_modify(|progress| {
            let previous = progress.phase;
            change(progress);
            if matches!(previous, InstallPhase::Cancelled | InstallPhase::Failed)
                && !matches!(
                    progress.phase,
                    InstallPhase::Completed | InstallPhase::Cancelled | InstallPhase::Failed
                )
            {
                progress.phase = previous;
            }
        });
    }
    fn check(&self) -> Result<(), InstallError> {
        if *self.0.cancelled.borrow() {
            Err(InstallError::Cancelled)
        } else {
            Ok(())
        }
    }
    async fn cancelled(&self) {
        let mut receiver = self.0.cancelled.subscribe();
        while !*receiver.borrow_and_update() {
            if receiver.changed().await.is_err() {
                return;
            }
        }
    }
    fn worker_checkpoint(&self, _phase: InstallPhase) -> Result<(), InstallError> {
        #[cfg(test)]
        if *self.0.worker_pause.lock().unwrap() == Some(_phase) {
            self.0.paused.store(true, Ordering::SeqCst);
            while self.check().is_ok() {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        self.check()
    }
    fn write_checkpoint(&self, _phase: InstallPhase) -> std::io::Result<()> {
        #[cfg(test)]
        if *self.0.write_failure.lock().unwrap() == Some(_phase) {
            return Err(std::io::Error::other("injected storage write failure"));
        }
        Ok(())
    }
}

struct CancelOnDrop {
    control: InstallControl,
    armed: bool,
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.control.cancel();
            self.control.update(|p| {
                if p.phase != InstallPhase::Publishing && p.phase != InstallPhase::Completed {
                    p.phase = InstallPhase::Cancelled;
                }
            });
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallStatus {
    Installed,
    AlreadyInstalled,
}

#[derive(Clone, Debug)]
pub struct InstallResult {
    pub status: InstallStatus,
    pub model: CatalogModel,
    pub cleanup_warnings: Vec<String>,
}

struct Session {
    store: ModelStore,
    plan: InstallPlan,
    control: InstallControl,
    staged: PathBuf,
    downloads: PathBuf,
    _global: File,
    _direction: ModelMutationGuard,
}

pub struct ModelInstaller {
    store: ModelStore,
    client: reqwest::Client,
    #[cfg(test)]
    test_origin: Option<String>,
    #[cfg(test)]
    retry_unit: Duration,
}

impl ModelInstaller {
    /// Builds an HTTP client but performs no requests or background work.
    pub fn new(store: ModelStore) -> Result<Self, InstallError> {
        let client = reqwest::Client::builder()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .build()
            .map_err(InstallError::Transport)?;
        Ok(Self {
            store,
            client,
            #[cfg(test)]
            test_origin: None,
            #[cfg(test)]
            retry_unit: Duration::from_secs(1),
        })
    }

    /// Invoke only for an explicit user action. Cancellation never starts a retry later.
    pub async fn install(
        &self,
        plan: InstallPlan,
        control: InstallControl,
    ) -> Result<InstallResult, InstallError> {
        if control.0.used.swap(true, Ordering::SeqCst) {
            return Err(InstallError::ControlUsed);
        }
        let mut cancellation = CancelOnDrop {
            control: control.clone(),
            armed: true,
        };
        control.update(|p| {
            p.phase = InstallPhase::Preparing;
            p.pair = Some(plan.model.pair());
            p.model_id = Some(plan.model.id().to_owned());
            p.file_count = plan.model.required_files().len();
            p.download_total = plan.download_bytes();
            p.installed_total = plan.installed_bytes();
            p.max_attempts = ATTEMPTS;
        });
        let result = self.run(plan, control.clone()).await;
        control.update(|p| match &result {
            Ok(_) => {
                p.phase = InstallPhase::Completed;
                p.completed_download_bytes = p.download_total;
                p.completed_installed_bytes = p.installed_total;
                p.detail = None;
            }
            Err(InstallError::Cancelled) => {
                p.phase = InstallPhase::Cancelled;
                p.detail = None;
            }
            Err(error) => {
                p.phase = InstallPhase::Failed;
                p.detail = Some(error.to_string());
            }
        });
        cancellation.armed = false;
        result
    }

    pub fn discard_downloads(&self, pair: LanguagePair) -> Result<bool, InstallError> {
        let _global = installation::global_lock(&self.store, pair)?;
        let _guard = self.store.exclusive(pair)?;
        Ok(installation::discard(&self.store, pair)?)
    }

    async fn run(
        &self,
        plan: InstallPlan,
        control: InstallControl,
    ) -> Result<InstallResult, InstallError> {
        control.check()?;
        let store = self.store.clone();
        let prepared_control = control.clone();
        let prepared = tokio::task::spawn_blocking(move || -> Result<Prepared, InstallError> {
            prepared_control.check()?;
            let pair = plan.model.pair();
            let global = installation::global_lock(&store, pair)?;
            let direction = store.exclusive(pair)?;
            // Recover stale local state before preparing another explicit operation.
            let recovery = installation::recover_locked(&store, pair)?;
            let previous = match store.installed(pair) {
                Ok(model) => Some(model),
                Err(StoreError::NotInstalled(_)) => None,
                Err(error) => return Err(error.into()),
            };
            if let Some(model) = &previous {
                if model.id() == plan.model.id()
                    && store.check_sizes(model).is_ok()
                    && verify_installed(&store, model, &prepared_control).is_ok()
                {
                    prepared_control.check()?;
                    let warnings = if recovery.warnings.is_empty() {
                        installation::discard(&store, pair)
                            .err()
                            .map(|e| e.to_string())
                            .into_iter()
                            .collect()
                    } else {
                        recovery.warnings
                    };
                    return Ok(Prepared::Existing(model.clone(), warnings));
                }
            }
            recovery.require_clean(pair)?;
            prepared_control.check()?;
            store.retain_catalog(&plan.snapshot)?;
            let downloads = installation::downloads(&store, pair)?;
            let staged = installation::begin(&store, &plan.model, previous.as_ref())?;
            Ok(Prepared::Session(Arc::new(Session {
                store,
                plan,
                control: prepared_control,
                staged,
                downloads,
                _global: global,
                _direction: direction,
            })))
        })
        .await
        .map_err(|e| InstallError::Worker(e.to_string()))??;
        let session = match prepared {
            Prepared::Existing(model, cleanup_warnings) => {
                return Ok(InstallResult {
                    status: InstallStatus::AlreadyInstalled,
                    model,
                    cleanup_warnings,
                })
            }
            Prepared::Session(session) => session,
        };
        let result = self.transfer_and_publish(session.clone()).await;
        if result.is_err() {
            let cleanup = session.clone();
            tokio::task::spawn_blocking(move || {
                installation::recover_locked(&cleanup.store, cleanup.plan.model.pair())
            })
            .await
            .map_err(|e| InstallError::Worker(e.to_string()))??;
        }
        result
    }

    async fn transfer_and_publish(
        &self,
        session: Arc<Session>,
    ) -> Result<InstallResult, InstallError> {
        let files: Vec<_> = session
            .plan
            .model
            .required_files()
            .into_iter()
            .cloned()
            .collect();
        for (index, file) in files.into_iter().enumerate() {
            session.control.check()?;
            session.control.update(|p| {
                p.phase = InstallPhase::Verifying;
                p.filename = Some(file.filename.clone());
                p.file_index = index + 1;
                p.attempt = 0;
                p.file_download_bytes = 0;
                p.file_download_total = file.size;
                p.file_installed_bytes = 0;
                p.file_installed_total = file.decompressed_size;
            });
            let cached_session = session.clone();
            let cached_file = file.clone();
            let reused = worker(move || cached(&cached_session, &cached_file)).await?;
            if reused {
                session.control.update(|p| {
                    p.reused_bytes += file.size;
                    p.file_download_bytes = file.size;
                });
            } else {
                for attempt in 1..=ATTEMPTS {
                    session.control.check()?;
                    session.control.update(|p| {
                        p.phase = InstallPhase::Downloading;
                        p.attempt = attempt;
                        p.file_download_bytes = 0;
                    });
                    match self.download(session.clone(), file.clone()).await {
                        Ok(()) => break,
                        Err(error) if error.retryable() && attempt < ATTEMPTS => {
                            session.control.update(|p| {
                                p.phase = InstallPhase::RetryWaiting;
                                p.file_download_bytes = 0;
                                p.detail = Some(error.to_string());
                            });
                            #[cfg(not(test))]
                            let delay = Duration::from_secs(u64::from(attempt));
                            #[cfg(test)]
                            let delay = self.retry_unit * attempt;
                            tokio::select! { biased; _ = session.control.cancelled() => return Err(InstallError::Cancelled), _ = tokio::time::sleep(delay) => {} }
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
            session.control.update(|p| {
                p.phase = InstallPhase::Verifying;
                p.completed_download_bytes += file.size;
                p.file_download_bytes = file.size;
                p.detail = None;
            });
            let decode_session = session.clone();
            let decoded_file = file.clone();
            worker(move || decompress(&decode_session, &decoded_file)).await?;
            session
                .control
                .update(|p| p.completed_installed_bytes += file.decompressed_size);
        }
        session.control.check()?;
        let commit = session.clone();
        worker(move || {
            commit.control.check()?;
            let bytes = serde_json::to_vec(&InstallationManifest::for_model(&commit.plan.model))
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            commit
                .store
                .cache
                .root
                .atomic_write(&commit.staged.join(INSTALL_MANIFEST_FILENAME), &bytes)?;
            commit.control.check()?;
            commit
                .control
                .update(|p| p.phase = InstallPhase::Publishing);
            #[cfg(test)]
            if commit.control.0.cancel_on_publish.load(Ordering::SeqCst) {
                commit.control.cancel();
            }
            let publication = installation::publish(&commit.store, commit.plan.model.pair());
            commit.control.update(|p| {
                p.phase = if publication.is_ok() {
                    InstallPhase::Completed
                } else {
                    InstallPhase::Failed
                };
            });
            let warnings = publication?;
            Ok(InstallResult {
                status: InstallStatus::Installed,
                model: commit.plan.model.clone(),
                cleanup_warnings: warnings,
            })
        })
        .await
    }

    async fn download(
        &self,
        session: Arc<Session>,
        file: CatalogArtifact,
    ) -> Result<(), InstallError> {
        #[cfg(test)]
        let rewritten = self
            .test_origin
            .as_ref()
            .map(|origin| file.url.replacen("https://models.jerod.me", origin, 1));
        #[cfg(test)]
        let url = rewritten.as_deref().unwrap_or(&file.url);
        #[cfg(not(test))]
        let url = &file.url;
        let request = self
            .client
            .get(url)
            .header(reqwest::header::ACCEPT_ENCODING, "identity")
            .send();
        let mut response = tokio::select! { biased; _ = session.control.cancelled() => return Err(InstallError::Cancelled), response = request => response.map_err(InstallError::Transport)? };
        if response.status() != reqwest::StatusCode::OK {
            return Err(InstallError::Http(response.status().as_u16()));
        }
        if response
            .content_length()
            .is_some_and(|size| size != file.size)
        {
            return Err(integrity(&file, "unexpected compressed Content-Length"));
        }
        let (sender, receiver) = mpsc::channel::<Vec<u8>>(4);
        let writer_session = session.clone();
        let writer_file = file.clone();
        let mut writer = tokio::task::spawn_blocking(move || {
            write_download(&writer_session, &writer_file, receiver)
        });
        let transfer = async {
            let mut received = 0_u64;
            while let Some(chunk) = response.chunk().await.map_err(InstallError::Transport)? {
                session.control.update(|p| {
                    p.network_bytes = p.network_bytes.saturating_add(chunk.len() as u64)
                });
                if chunk.len() as u64 > file.size.saturating_sub(received) {
                    return Err(integrity(&file, "compressed response exceeds catalog size"));
                }
                for piece in chunk.chunks(BUFFER_SIZE) {
                    if sender.send(piece.to_vec()).await.is_err() {
                        return Ok(());
                    }
                    received += piece.len() as u64;
                    session.control.update(|p| p.file_download_bytes = received);
                }
            }
            if received != file.size {
                return Err(integrity(&file, "incomplete compressed response"));
            }
            Ok(())
        };
        let mut finished_writer = None;
        let transfer_result = tokio::select! {
            biased;
            _ = session.control.cancelled() => Err(InstallError::Cancelled),
            result = &mut writer => {
                finished_writer = Some(result.map_err(|e| InstallError::Worker(e.to_string()))?);
                Ok(())
            }
            result = transfer => result
        };
        drop(sender);
        let write_result = match finished_writer {
            Some(result) => result,
            None => writer
                .await
                .map_err(|e| InstallError::Worker(e.to_string()))?,
        };
        // The writer owns the locks until it stops, including on cancellation.
        if matches!(
            write_result,
            Err(InstallError::Io(_) | InstallError::Store(_) | InstallError::Worker(_))
        ) {
            return write_result;
        }
        match transfer_result {
            Err(error) => Err(error),
            Ok(()) => write_result,
        }
    }
}

enum Prepared {
    Existing(CatalogModel, Vec<String>),
    Session(Arc<Session>),
}

async fn worker<T: Send + 'static>(
    operation: impl FnOnce() -> Result<T, InstallError> + Send + 'static,
) -> Result<T, InstallError> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|e| InstallError::Worker(e.to_string()))?
}

fn integrity(file: &CatalogArtifact, detail: impl Into<String>) -> InstallError {
    InstallError::Integrity {
        file: file.filename.clone(),
        detail: detail.into(),
    }
}

fn object_path(session: &Session, file: &CatalogArtifact) -> PathBuf {
    session.downloads.join(format!("{}.zst", file.sha256))
}

fn hash_file(
    path: &Path,
    size: u64,
    expected: Sha256Digest,
    control: &InstallControl,
) -> Result<(), InstallError> {
    control.worker_checkpoint(InstallPhase::Verifying)?;
    let mut file = File::open(path)?;
    if !file.metadata()?.is_file() || file.metadata()?.len() != size {
        return Err(InstallError::Integrity {
            file: path.display().to_string(),
            detail: "unexpected file size or type".into(),
        });
    }
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; BUFFER_SIZE];
    let mut read = 0_u64;
    loop {
        control.check()?;
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        read += count as u64;
        if read > size {
            return Err(InstallError::Integrity {
                file: path.display().to_string(),
                detail: "file grew during verification".into(),
            });
        }
        hash.update(&buffer[..count]);
    }
    if read != size || Sha256Digest::from_bytes(hash.finalize().into()) != expected {
        return Err(InstallError::Integrity {
            file: path.display().to_string(),
            detail: "SHA-256 or size mismatch".into(),
        });
    }
    Ok(())
}

fn verify_installed(
    store: &ModelStore,
    model: &CatalogModel,
    control: &InstallControl,
) -> Result<(), InstallError> {
    for file in model.required_files() {
        let path = store
            .cache
            .root
            .checked(&ModelStore::relative(model.pair())?.join(&file.filename))?;
        hash_file(
            &path,
            file.decompressed_size,
            file.decompressed_sha256,
            control,
        )?;
    }
    Ok(())
}

fn cached(session: &Session, file: &CatalogArtifact) -> Result<bool, InstallError> {
    let path = session
        .store
        .cache
        .root
        .checked(&object_path(session, file))?;
    if !path.try_exists()? {
        return Ok(false);
    }
    match hash_file(&path, file.size, file.sha256, &session.control) {
        Ok(()) => Ok(true),
        Err(InstallError::Integrity { .. }) => {
            fs::remove_file(path)?;
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

fn write_download(
    session: &Session,
    file: &CatalogArtifact,
    mut chunks: mpsc::Receiver<Vec<u8>>,
) -> Result<(), InstallError> {
    let directory = session.store.cache.root.checked(&session.downloads)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(".partial-")
        .tempfile_in(directory)?;
    let mut hash = Sha256::new();
    let mut written = 0_u64;
    while let Some(chunk) = chunks.blocking_recv() {
        session.control.check()?;
        if chunk.len() as u64 > file.size.saturating_sub(written) {
            return Err(integrity(file, "compressed file exceeds catalog size"));
        }
        session
            .control
            .write_checkpoint(InstallPhase::Downloading)?;
        temporary.write_all(&chunk)?;
        hash.update(&chunk);
        written += chunk.len() as u64;
    }
    session.control.worker_checkpoint(InstallPhase::Verifying)?;
    session
        .control
        .update(|p| p.phase = InstallPhase::Verifying);
    if written != file.size || Sha256Digest::from_bytes(hash.finalize().into()) != file.sha256 {
        return Err(integrity(file, "compressed SHA-256 or size mismatch"));
    }
    temporary.as_file().sync_all()?;
    temporary
        .persist(
            session
                .store
                .cache
                .root
                .checked(&object_path(session, file))?,
        )
        .map_err(|e| e.error)?;
    Ok(())
}

fn decompress(session: &Session, file: &CatalogArtifact) -> Result<(), InstallError> {
    session
        .control
        .worker_checkpoint(InstallPhase::Decompressing)?;
    session
        .control
        .update(|p| p.phase = InstallPhase::Decompressing);
    let input = File::open(
        session
            .store
            .cache
            .root
            .checked(&object_path(session, file))?,
    )?;
    let mut decoder =
        zstd::stream::read::Decoder::new(input).map_err(|source| InstallError::Decompression {
            file: file.filename.clone(),
            source,
        })?;
    let directory = session.store.cache.root.checked(&session.staged)?;
    let mut output = tempfile::NamedTempFile::new_in(directory)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; BUFFER_SIZE];
    let mut size = 0_u64;
    loop {
        session.control.check()?;
        let count = decoder
            .read(&mut buffer)
            .map_err(|source| InstallError::Decompression {
                file: file.filename.clone(),
                source,
            })?;
        if count == 0 {
            break;
        }
        if count as u64 > file.decompressed_size.saturating_sub(size) {
            return Err(integrity(file, "decompressed output exceeds catalog size"));
        }
        session
            .control
            .write_checkpoint(InstallPhase::Decompressing)?;
        output.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
        size += count as u64;
        session.control.update(|p| p.file_installed_bytes = size);
    }
    if size != file.decompressed_size
        || Sha256Digest::from_bytes(hash.finalize().into()) != file.decompressed_sha256
    {
        return Err(integrity(file, "decompressed SHA-256 or size mismatch"));
    }
    session.control.check()?;
    output.as_file().sync_all()?;
    output
        .persist(
            session
                .store
                .cache
                .root
                .checked(&session.staged.join(&file.filename))?,
        )
        .map_err(|e| e.error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
