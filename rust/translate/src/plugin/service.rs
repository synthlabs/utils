use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use tokio::sync::{watch, Mutex as AsyncMutex};

use super::{dto::*, ErrorCode, ServiceError};
use crate::*;

#[derive(Clone, Debug, Default)]
pub struct PluginConfig {
    pub store_root: Option<PathBuf>,
    pub core_options: CoreOptions,
    pub workload_options: WorkloadOptions,
    #[cfg(any(test, feature = "tauri-test"))]
    #[doc(hidden)]
    pub test_origin: Option<String>,
}

struct Resources {
    store: ModelStore,
    catalog: CatalogClient,
    installer: ModelInstaller,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    Native,
    Webview(String),
}

struct Request {
    owner: Owner,
    cancelled: watch::Sender<bool>,
    result: watch::Sender<Option<Result<TranslationOutcome, ServiceError>>>,
    claimed: bool,
}

struct State {
    revision: u64,
    models_revision: u64,
    phase: ServicePhase,
    error: Option<ServiceError>,
    resources: Option<Arc<Resources>>,
    manager: Option<Arc<WorkloadManager>>,
    catalog: CatalogStateDto,
    recovery: RecoveryDto,
    installation: Option<InstallationDto>,
    install_control: Option<InstallControl>,
    next_id: u64,
    requests: HashMap<String, Request>,
    initialization: Option<watch::Sender<Option<Result<ServiceSnapshot, ServiceError>>>>,
}

/// One app-owned service. Construction is local and does not start work.
pub struct TranslationService {
    config: PluginConfig,
    root: Result<PathBuf, ServiceError>,
    state: Mutex<State>,
    lifecycle: AsyncMutex<()>,
    refresh: AsyncMutex<()>,
    changes: watch::Sender<ServiceSnapshot>,
    shutdown: watch::Sender<bool>,
}

impl TranslationService {
    pub fn new(root: PathBuf, config: PluginConfig) -> Arc<Self> {
        Self::with_root(Ok(root), config)
    }

    pub(crate) fn with_root(
        root: Result<PathBuf, ServiceError>,
        config: PluginConfig,
    ) -> Arc<Self> {
        let state = State {
            revision: 0,
            models_revision: 0,
            phase: ServicePhase::Initializing,
            error: None,
            resources: None,
            manager: None,
            catalog: CatalogStateDto::default(),
            recovery: RecoveryDto::default(),
            installation: None,
            install_control: None,
            next_id: 0,
            requests: HashMap::new(),
            initialization: None,
        };
        let changes = watch::channel(Self::snapshot_from(&state)).0;
        Arc::new(Self {
            config,
            root,
            state: Mutex::new(state),
            lifecycle: AsyncMutex::new(()),
            refresh: AsyncMutex::new(()),
            changes,
            shutdown: watch::channel(false).0,
        })
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn snapshot_from(state: &State) -> ServiceSnapshot {
        ServiceSnapshot {
            revision: state.revision.to_string(),
            models_revision: state.models_revision.to_string(),
            phase: state.phase,
            error: state.error.clone(),
            catalog: state.catalog.clone(),
            installation: state.installation.clone(),
            recovery: state.recovery.clone(),
            workload: state
                .manager
                .as_ref()
                .map(|manager| manager.snapshot().into()),
        }
    }

    fn publish(&self, state: &mut State) {
        state.revision += 1;
        self.changes.send_replace(Self::snapshot_from(state));
    }

    pub fn snapshot(&self) -> ServiceSnapshot {
        Self::snapshot_from(&self.lock())
    }

    pub fn subscribe(&self) -> watch::Receiver<ServiceSnapshot> {
        self.changes.subscribe()
    }

    fn phase_error(state: &State) -> ServiceError {
        let (code, message) = match state.phase {
            ServicePhase::Initializing => {
                (ErrorCode::NotReady, "translation service is initializing")
            }
            ServicePhase::Unavailable => {
                (ErrorCode::Unavailable, "translation service is unavailable")
            }
            ServicePhase::Releasing | ServicePhase::Paused => {
                (ErrorCode::Paused, "translation workload is paused")
            }
            ServicePhase::Shutdown => (ErrorCode::Shutdown, "translation service has shut down"),
            ServicePhase::Ready => (
                ErrorCode::Unavailable,
                "translation resources are unavailable",
            ),
        };
        ServiceError::new(
            code,
            state
                .error
                .as_ref()
                .map_or_else(|| message.into(), |error| error.message.clone()),
        )
    }

    fn resources(&self) -> Result<Arc<Resources>, ServiceError> {
        let state = self.lock();
        if matches!(
            state.phase,
            ServicePhase::Shutdown | ServicePhase::Unavailable | ServicePhase::Initializing
        ) {
            return Err(Self::phase_error(&state));
        }
        state
            .resources
            .clone()
            .ok_or_else(|| Self::phase_error(&state))
    }

    pub async fn initialize(self: &Arc<Self>) -> Result<ServiceSnapshot, ServiceError> {
        let mut result = self.start_initialization();
        loop {
            if let Some(result) = result.borrow().clone() {
                return result;
            }
            result.changed().await.map_err(|_| {
                ServiceError::new(ErrorCode::WorkerFailed, "initialization task stopped")
            })?;
        }
    }

    pub(crate) fn start_initialization(
        self: &Arc<Self>,
    ) -> watch::Receiver<Option<Result<ServiceSnapshot, ServiceError>>> {
        let mut state = self.lock();
        if let Some(attempt) = &state.initialization {
            return attempt.subscribe();
        }
        if matches!(state.phase, ServicePhase::Ready | ServicePhase::Paused) {
            return watch::channel(Some(Ok(Self::snapshot_from(&state)))).1;
        }
        if state.phase == ServicePhase::Shutdown {
            return watch::channel(Some(Err(Self::phase_error(&state)))).1;
        }
        let (sender, receiver) = watch::channel(None);
        state.initialization = Some(sender.clone());
        let service = self.clone();
        let task = async move {
            let result = service.initialize_inner().await;
            let mut state = service.lock();
            state.initialization = None;
            sender.send_replace(Some(result));
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(task);
        } else {
            tauri::async_runtime::spawn(task);
        }
        receiver
    }

    async fn initialize_inner(self: &Arc<Self>) -> Result<ServiceSnapshot, ServiceError> {
        let _lifecycle = self.lifecycle.lock().await;
        {
            let mut state = self.lock();
            match state.phase {
                ServicePhase::Ready | ServicePhase::Paused => {
                    return Ok(Self::snapshot_from(&state))
                }
                ServicePhase::Shutdown => return Err(Self::phase_error(&state)),
                _ => {}
            }
            state.phase = ServicePhase::Initializing;
            state.error = None;
            self.publish(&mut state);
        }
        let root = self.root.clone();
        let config = self.config.clone();
        let opened = tokio::task::spawn_blocking(move || -> Result<_, ServiceError> {
            let root = root?;
            let store = ModelStore::open(&root)?;
            let catalog = CatalogClient::open(&root)?;
            let installer = ModelInstaller::new(store.clone())?;
            #[cfg(any(test, feature = "tauri-test"))]
            let (catalog, installer) = if let Some(origin) = &config.test_origin {
                (
                    catalog.with_test_origin(origin),
                    installer.with_test_origin(origin),
                )
            } else {
                (catalog, installer)
            };
            let cached = catalog.cached().ok().as_ref().map(Into::into);
            let recovery = store.recovery_report().clone().into();
            let manager = Arc::new(WorkloadManager::new(
                store.clone(),
                config.core_options,
                config.workload_options,
            )?);
            Ok((
                Arc::new(Resources {
                    store,
                    catalog,
                    installer,
                }),
                manager,
                cached,
                recovery,
            ))
        })
        .await
        .map_err(ServiceError::from)
        .and_then(|result| result);
        {
            let mut state = self.lock();
            if state.phase == ServicePhase::Shutdown {
                if let Ok((_, manager, _, _)) = opened {
                    manager.request_shutdown();
                }
                return Err(Self::phase_error(&state));
            }
            match opened {
                Ok((resources, manager, cached, recovery)) => {
                    state.resources = Some(resources);
                    state.manager = Some(manager);
                    state.catalog.catalog = cached;
                    state.catalog.freshness =
                        state.catalog.catalog.as_ref().map(|_| FreshnessDto::Cached);
                    state.recovery = recovery;
                    state.phase = ServicePhase::Ready;
                }
                Err(error) => {
                    state.phase = ServicePhase::Unavailable;
                    state.error = Some(error.clone());
                    self.publish(&mut state);
                    return Err(error);
                }
            }
            self.publish(&mut state);
        }
        let service = Arc::downgrade(self);
        tokio::spawn(async move {
            if let Some(service) = service.upgrade() {
                let _ = service.refresh_catalog().await;
            }
        });
        Ok(self.snapshot())
    }

    pub async fn refresh_catalog(&self) -> Result<CatalogStateDto, ServiceError> {
        let _refresh = self.refresh.lock().await;
        let resources = self.resources()?;
        {
            let mut state = self.lock();
            state.catalog.refreshing = true;
            self.publish(&mut state);
        }
        let mut shutdown = self.shutdown.subscribe();
        let result = if *shutdown.borrow() {
            return Err(ServiceError::new(
                ErrorCode::Shutdown,
                "translation service has shut down",
            ));
        } else {
            tokio::select! {
                biased;
                _ = shutdown.changed() => return Err(ServiceError::new(ErrorCode::Shutdown, "translation service has shut down")),
                result = resources.catalog.refresh() => result,
            }
        };
        let mut state = self.lock();
        state.catalog.refreshing = false;
        match result {
            Ok(load) => {
                state.catalog.catalog = Some((&load.snapshot).into());
                state.catalog.freshness = Some(match load.freshness {
                    CatalogFreshness::Fresh => FreshnessDto::Fresh,
                    CatalogFreshness::Cached => FreshnessDto::Cached,
                });
                state.catalog.error = load.refresh_error.map(Into::into);
                state.models_revision += 1;
            }
            Err(error) => {
                let error = ServiceError::from(error);
                state.catalog.error = Some(error.clone());
                self.publish(&mut state);
                return Err(error);
            }
        }
        self.publish(&mut state);
        Ok(state.catalog.clone())
    }

    pub fn catalog(&self) -> Result<CatalogStateDto, ServiceError> {
        self.resources()?;
        Ok(self.lock().catalog.clone())
    }

    pub async fn detect(&self, text: String) -> Result<Option<Detection>, ServiceError> {
        self.resources()?;
        Ok(tokio::task::spawn_blocking(move || TranslationCore::default().detect(&text)).await?)
    }

    pub async fn submit(
        self: &Arc<Self>,
        request: TranslationRequest,
    ) -> Result<TranslationTicket, ServiceError> {
        let id = self.submit_owned(Owner::Native, request)?;
        Ok(TranslationTicket {
            id,
            service: Arc::downgrade(self),
        })
    }

    pub(crate) fn submit_owned(
        &self,
        owner: Owner,
        request: TranslationRequest,
    ) -> Result<String, ServiceError> {
        let mut state = self.lock();
        if state.phase != ServicePhase::Ready {
            return Err(Self::phase_error(&state));
        }
        let manager = state
            .manager
            .clone()
            .ok_or_else(|| Self::phase_error(&state))?;
        state.next_id += 1;
        let id = format!("translation-{}", state.next_id);
        let cancelled = watch::channel(false).0;
        let result = watch::channel(None).0;
        let mut cancel_rx = cancelled.subscribe();
        let result_tx = result.clone();
        state.requests.insert(
            id.clone(),
            Request {
                owner,
                cancelled,
                result,
                claimed: false,
            },
        );
        tokio::spawn(async move {
            let translate = async {
                let job = manager.submit(request).await?;
                job.result().await
            };
            let result = tokio::select! {
                biased;
                _ = cancel_rx.changed() => Err(ServiceError::new(ErrorCode::Cancelled, "translation request was cancelled")),
                result = translate => result.map_err(Into::into),
            };
            result_tx.send_replace(Some(result));
        });
        Ok(id)
    }

    pub(crate) async fn result_owned(
        &self,
        owner: &Owner,
        id: &str,
    ) -> Result<TranslationOutcome, ServiceError> {
        let mut receiver = {
            let mut state = self.lock();
            let request = state.requests.get_mut(id).ok_or_else(operation_not_found)?;
            if &request.owner != owner {
                return Err(wrong_owner());
            }
            if request.claimed {
                return Err(ServiceError::new(
                    ErrorCode::Busy,
                    "translation result already has a receiver",
                ));
            }
            request.claimed = true;
            request.result.subscribe()
        };
        let result = loop {
            if let Some(result) = receiver.borrow().clone() {
                break result;
            }
            if receiver.changed().await.is_err() {
                break Err(operation_not_found());
            }
        };
        self.lock().requests.remove(id);
        result
    }

    pub(crate) fn cancel_owned(&self, owner: &Owner, id: &str) -> Result<(), ServiceError> {
        let state = self.lock();
        let request = state.requests.get(id).ok_or_else(operation_not_found)?;
        if &request.owner != owner {
            return Err(wrong_owner());
        }
        if request.result.borrow().is_none() {
            request.cancelled.send_replace(true);
        }
        Ok(())
    }

    pub(crate) fn forget_owner(&self, owner: &Owner) {
        self.lock().requests.retain(|_, request| {
            if &request.owner == owner {
                request.cancelled.send_replace(true);
                false
            } else {
                true
            }
        });
    }

    pub async fn release(self: &Arc<Self>) -> Result<ServiceSnapshot, ServiceError> {
        let service = self.clone();
        tokio::spawn(async move { service.release_inner().await }).await?
    }

    async fn release_inner(&self) -> Result<ServiceSnapshot, ServiceError> {
        let _lifecycle = self.lifecycle.lock().await;
        let manager = {
            let mut state = self.lock();
            if state.phase == ServicePhase::Paused {
                return Ok(Self::snapshot_from(&state));
            }
            if state.phase != ServicePhase::Ready {
                return Err(Self::phase_error(&state));
            }
            state.phase = ServicePhase::Releasing;
            let manager = state
                .manager
                .clone()
                .ok_or_else(|| Self::phase_error(&state))?;
            manager.request_shutdown();
            self.publish(&mut state);
            manager
        };
        let outcome = manager.shutdown().await.map_err(ServiceError::from);
        let mut state = self.lock();
        state.manager = None;
        if state.phase != ServicePhase::Shutdown {
            state.phase = ServicePhase::Paused;
        }
        state.error = outcome.as_ref().err().cloned();
        self.publish(&mut state);
        outcome.map(|_| Self::snapshot_from(&state))
    }

    pub async fn resume(&self) -> Result<ServiceSnapshot, ServiceError> {
        let _lifecycle = self.lifecycle.lock().await;
        let mut state = self.lock();
        if state.phase == ServicePhase::Ready {
            return Ok(Self::snapshot_from(&state));
        }
        if state.phase != ServicePhase::Paused {
            return Err(Self::phase_error(&state));
        }
        let resources = state
            .resources
            .clone()
            .ok_or_else(|| Self::phase_error(&state))?;
        state.manager = Some(Arc::new(WorkloadManager::new(
            resources.store.clone(),
            self.config.core_options,
            self.config.workload_options,
        )?));
        state.phase = ServicePhase::Ready;
        state.error = None;
        self.publish(&mut state);
        Ok(Self::snapshot_from(&state))
    }

    pub fn request_shutdown(&self) {
        let mut state = self.lock();
        if state.phase == ServicePhase::Shutdown {
            return;
        }
        state.phase = ServicePhase::Shutdown;
        state.catalog.refreshing = false;
        self.shutdown.send_replace(true);
        if let Some(manager) = &state.manager {
            manager.request_shutdown();
        }
        if let Some(control) = &state.install_control {
            control.cancel();
        }
        for request in state.requests.values() {
            request.cancelled.send_replace(true);
        }
        state.requests.clear();
        self.publish(&mut state);
    }

    pub async fn inventory(&self) -> Result<Vec<InventoryDto>, ServiceError> {
        let resources = self.resources()?;
        Ok(tokio::task::spawn_blocking(move || {
            let current = resources.catalog.cached().ok();
            resources
                .store
                .inventory(current.as_ref())
                .map(|entries| entries.into_iter().map(Into::into).collect())
        })
        .await??)
    }

    pub async fn verify_model(&self, pair: LanguagePair) -> Result<ModelDto, ServiceError> {
        let resources = self.resources()?;
        Ok(tokio::task::spawn_blocking(move || {
            resources.store.verify(pair).map(|model| (&model).into())
        })
        .await??)
    }

    pub async fn remove_model(&self, pair: LanguagePair) -> Result<bool, ServiceError> {
        let resources = self.resources()?;
        let result = tokio::task::spawn_blocking(move || resources.store.remove(pair)).await??;
        let mut state = self.lock();
        state.models_revision += 1;
        self.publish(&mut state);
        Ok(result)
    }

    pub async fn discard_downloads(&self, pair: LanguagePair) -> Result<bool, ServiceError> {
        let resources = self.resources()?;
        Ok(
            tokio::task::spawn_blocking(move || resources.installer.discard_downloads(pair))
                .await??,
        )
    }

    pub async fn plan_install(&self, pair: LanguagePair) -> Result<InstallPlanDto, ServiceError> {
        let resources = self.resources()?;
        tokio::task::spawn_blocking(move || {
            let snapshot = resources.catalog.cached()?;
            let plan = InstallPlan::new(&snapshot, pair)?;
            Ok((&plan).into())
        })
        .await?
    }

    pub async fn start_install(
        self: &Arc<Self>,
        selection: InstallPlanDto,
    ) -> Result<InstallationDto, ServiceError> {
        let resources = self.resources()?;
        let plan_resources = resources.clone();
        let plan = tokio::task::spawn_blocking(move || -> Result<_, ServiceError> {
            let snapshot = CatalogCache::open(plan_resources.store.root())?
                .load_snapshot(&selection.catalog_sha256)?;
            let plan = InstallPlan::new(&snapshot, selection.pair.try_into()?)?;
            if plan.model().id() != selection.model_id {
                return Err(ServiceError::new(
                    ErrorCode::InvalidPlan,
                    "selected model does not match the pinned catalog",
                ));
            }
            Ok(plan)
        })
        .await??;
        let control = InstallControl::new();
        let mut progress = control.subscribe();
        let id = {
            let mut state = self.lock();
            if state.phase == ServicePhase::Shutdown {
                return Err(Self::phase_error(&state));
            }
            if state.install_control.is_some() {
                return Err(ServiceError::new(
                    ErrorCode::Busy,
                    "an installation is already active",
                ));
            }
            state.next_id += 1;
            let id = format!("install-{}", state.next_id);
            state.installation = Some(InstallationDto {
                id: id.clone(),
                model: plan.model().into(),
                progress: control.progress().into(),
                finished: false,
                result: None,
                error: None,
            });
            state.install_control = Some(control.clone());
            self.publish(&mut state);
            id
        };
        let service = Arc::downgrade(self);
        let progress_service = service.clone();
        let progress_id = id.clone();
        let progress_task = tokio::spawn(async move {
            while progress.changed().await.is_ok() {
                let value = progress.borrow_and_update().clone();
                let Some(service) = progress_service.upgrade() else {
                    break;
                };
                let mut state = service.lock();
                if let Some(installation) = state
                    .installation
                    .as_mut()
                    .filter(|v| v.id == progress_id && !v.finished)
                {
                    installation.progress = value.into();
                    service.publish(&mut state);
                }
            }
        });
        let operation_id = id.clone();
        tokio::spawn(async move {
            let outcome = resources.installer.install(plan, control.clone()).await;
            progress_task.abort();
            if let Some(service) = service.upgrade() {
                let mut state = service.lock();
                if let Some(installation) =
                    state.installation.as_mut().filter(|v| v.id == operation_id)
                {
                    installation.progress = control.progress().into();
                    installation.finished = true;
                    match outcome {
                        Ok(result) => installation.result = Some(result.into()),
                        Err(error) => installation.error = Some(error.into()),
                    }
                    state.install_control = None;
                    state.models_revision += 1;
                    service.publish(&mut state);
                }
            }
        });
        self.installation(&id)
    }

    pub fn installation(&self, id: &str) -> Result<InstallationDto, ServiceError> {
        self.lock()
            .installation
            .as_ref()
            .filter(|v| v.id == id)
            .cloned()
            .ok_or_else(operation_not_found)
    }

    pub fn cancel_install(&self, id: &str) -> Result<(), ServiceError> {
        let state = self.lock();
        if !state.installation.as_ref().is_some_and(|v| v.id == id) {
            return Err(operation_not_found());
        }
        if let Some(control) = &state.install_control {
            control.cancel();
        }
        Ok(())
    }
}

impl Drop for TranslationService {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

pub struct TranslationTicket {
    id: String,
    service: Weak<TranslationService>,
}

impl TranslationTicket {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn result(self) -> Result<TranslationOutcome, ServiceError> {
        let service = self.service.upgrade().ok_or_else(operation_not_found)?;
        service.result_owned(&Owner::Native, &self.id).await
    }

    pub fn cancel(&self) -> Result<(), ServiceError> {
        self.service
            .upgrade()
            .ok_or_else(operation_not_found)?
            .cancel_owned(&Owner::Native, &self.id)
    }
}

impl Drop for TranslationTicket {
    fn drop(&mut self) {
        if let Some(service) = self.service.upgrade() {
            if let Some(request) = service.lock().requests.remove(&self.id) {
                request.cancelled.send_replace(true);
            }
        }
    }
}

fn operation_not_found() -> ServiceError {
    ServiceError::new(
        ErrorCode::OperationNotFound,
        "operation is no longer available",
    )
}
fn wrong_owner() -> ServiceError {
    ServiceError::new(
        ErrorCode::WrongOwner,
        "translation belongs to another caller",
    )
}

#[cfg(test)]
mod tests;
