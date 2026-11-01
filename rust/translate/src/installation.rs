use std::{
    fs::{self, File, TryLockError},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[cfg(any(test, feature = "model-downloads"))]
use crate::filesystem::invalid;
use crate::{
    catalog::POINTER_LIMIT, filesystem::check_tree, InstallationManifest, Language, LanguagePair,
    ModelStore, StoreError, INSTALL_MANIFEST_FILENAME,
};

#[derive(Clone, Debug, Default)]
pub struct RecoveryReport {
    pub rolled_back: Vec<LanguagePair>,
    pub cleaned_up: Vec<LanguagePair>,
    pub warnings: Vec<String>,
    pub installer_busy: bool,
}

#[derive(Default)]
pub(crate) struct RecoveryOutcome {
    committed: Option<bool>,
    pub warnings: Vec<String>,
}

impl RecoveryOutcome {
    pub(crate) fn require_clean(self, pair: LanguagePair) -> Result<(), StoreError> {
        if self.warnings.is_empty() {
            Ok(())
        } else {
            Err(StoreError::CleanupPending {
                pair,
                detail: self.warnings.join("; "),
            })
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Journal {
    schema_version: u32,
    #[serde(with = "pair_serde")]
    pair: LanguagePair,
    previous: Option<InstallationManifest>,
    next: InstallationManifest,
    committed: bool,
}

pub(crate) fn area(kind: &str, pair: LanguagePair) -> PathBuf {
    Path::new(kind).join(format!("{}-{}", pair.source, pair.target))
}

pub(crate) fn global_lock(store: &ModelStore, pair: LanguagePair) -> Result<File, StoreError> {
    let file = store.cache.root.lock_file("install.lock")?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(StoreError::Busy(pair)),
        Err(TryLockError::Error(error)) => Err(error.into()),
    }
}

fn remove_owned(store: &ModelStore, relative: &Path) -> Result<bool, StoreError> {
    let path = store.cache.root.checked(relative)?;
    if !path.try_exists()? {
        return Ok(false);
    }
    check_tree(&path)?;
    fs::remove_dir_all(path)?;
    Ok(true)
}

fn manifest_at(store: &ModelStore, relative: &Path) -> Result<InstallationManifest, StoreError> {
    let bytes = store
        .cache
        .root
        .read(&relative.join(INSTALL_MANIFEST_FILENAME), POINTER_LIMIT)?;
    serde_json::from_slice(&bytes).map_err(|e| StoreError::Unmanaged(e.to_string()))
}

fn expect_manifest(
    store: &ModelStore,
    relative: &Path,
    expected: &InstallationManifest,
) -> Result<(), StoreError> {
    if manifest_at(store, relative)? != *expected {
        return Err(StoreError::Unmanaged(format!(
            "unexpected installation identity at {}",
            relative.display()
        )));
    }
    Ok(())
}

fn read_journal(store: &ModelStore, pair: LanguagePair) -> Result<Journal, StoreError> {
    let relative = area(".install", pair);
    let external = relative.with_extension("json");
    let source = if store.cache.root.checked(&external)?.try_exists()? {
        external
    } else {
        relative.join("journal.json")
    };
    let bytes = store.cache.root.read(&source, POINTER_LIMIT)?;
    let journal: Journal =
        serde_json::from_slice(&bytes).map_err(|e| StoreError::Unmanaged(e.to_string()))?;
    if journal.schema_version != 1 || journal.pair != pair {
        return Err(StoreError::Unmanaged(
            "unrecognized installation journal".into(),
        ));
    }
    // Check both identities against retained, verified snapshots before mutating anything.
    for manifest in journal
        .previous
        .iter()
        .chain(std::iter::once(&journal.next))
    {
        let snapshot = store.cache.load_snapshot(&manifest.catalog_sha256)?;
        let model = snapshot
            .model(pair)
            .ok_or_else(|| StoreError::Unmanaged("journal pair missing from catalog".into()))?;
        if InstallationManifest::for_model(model) != *manifest {
            return Err(StoreError::Unmanaged(
                "journal identity does not match its catalog".into(),
            ));
        }
    }
    Ok(journal)
}

#[derive(Serialize, Deserialize)]
struct DownloadOwner {
    schema_version: u32,
    #[serde(with = "pair_serde")]
    pair: LanguagePair,
}

mod pair_serde {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        pair: &LanguagePair,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        (pair.source.as_str(), pair.target.as_str()).serialize(serializer)
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<LanguagePair, D::Error> {
        let (source, target) = <(String, String)>::deserialize(deserializer)?;
        Ok(LanguagePair {
            source: Language::new(&source).map_err(serde::de::Error::custom)?,
            target: Language::new(&target).map_err(serde::de::Error::custom)?,
        })
    }
}

fn check_downloads(store: &ModelStore, pair: LanguagePair) -> Result<PathBuf, StoreError> {
    let relative = area(".downloads", pair);
    let external = relative.with_extension("json");
    let source = if store.cache.root.checked(&external)?.try_exists()? {
        external
    } else {
        relative.join("owner.json")
    };
    let unmanaged = |detail| StoreError::UnmanagedDownloads {
        pair,
        directory: store.root().join(&relative),
        detail,
    };
    let bytes = store
        .read_metadata(&source, POINTER_LIMIT)
        .map_err(|error| match error {
            StoreError::MetadataIo { ref source, .. }
                if source.kind() == std::io::ErrorKind::NotFound =>
            {
                unmanaged(format!("missing ownership metadata: {error}"))
            }
            error => error,
        })?;
    let owner: DownloadOwner = serde_json::from_slice(&bytes)
        .map_err(|e| unmanaged(format!("{}: {e}", store.root().join(&source).display())))?;
    if owner.schema_version != 1 || owner.pair != pair {
        return Err(unmanaged(format!(
            "unrecognized ownership metadata at {}",
            store.root().join(&source).display()
        )));
    }
    Ok(relative)
}

#[cfg(any(test, feature = "model-downloads"))]
pub(crate) fn downloads(store: &ModelStore, pair: LanguagePair) -> Result<PathBuf, StoreError> {
    let relative = area(".downloads", pair);
    if !store.cache.root.checked(&relative)?.try_exists()? {
        let parent = store.cache.root.directory(Path::new(".downloads"))?;
        let temporary = tempfile::tempdir_in(parent)?;
        let owner = DownloadOwner {
            schema_version: 1,
            pair,
        };
        let bytes = serde_json::to_vec(&owner).map_err(|e| invalid(e.to_string()))?;
        let mut file = File::create(temporary.path().join("owner.json"))?;
        std::io::Write::write_all(&mut file, &bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(temporary.path(), store.cache.root.checked(&relative)?)?;
        store
            .cache
            .root
            .atomic_write(&relative.with_extension("json"), &bytes)?;
    }
    check_downloads(store, pair)
}

pub(crate) fn discard(store: &ModelStore, pair: LanguagePair) -> Result<bool, StoreError> {
    let relative = area(".downloads", pair);
    let external = relative.with_extension("json");
    if !store.cache.root.checked(&relative)?.try_exists()?
        && !store.cache.root.checked(&external)?.try_exists()?
    {
        return Ok(false);
    }
    let relative = check_downloads(store, pair)?;
    let bytes = serde_json::to_vec(&DownloadOwner {
        schema_version: 1,
        pair,
    })
    .map_err(|e| std::io::Error::other(e.to_string()))?;
    store.cache.root.atomic_write(&external, &bytes)?;
    remove_owned(store, &relative)?;
    fs::remove_file(store.cache.root.checked(&external)?)?;
    Ok(true)
}

fn remove_partials(store: &ModelStore, pair: LanguagePair) -> Result<(), StoreError> {
    if !store
        .cache
        .root
        .checked(&area(".downloads", pair))?
        .try_exists()?
    {
        return Ok(());
    }
    let relative = check_downloads(store, pair)?;
    for entry in fs::read_dir(store.cache.root.checked(&relative)?)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(".partial-") {
            let path = store
                .cache
                .root
                .checked(&relative.join(entry.file_name()))?;
            if !entry.file_type()?.is_file() {
                return Err(StoreError::Unmanaged("non-file partial download".into()));
            }
            fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(any(test, feature = "model-downloads"))]
pub(crate) fn begin(
    store: &ModelStore,
    model: &crate::CatalogModel,
    previous: Option<&crate::CatalogModel>,
) -> Result<PathBuf, StoreError> {
    let relative = area(".install", model.pair());
    if store.cache.root.checked(&relative)?.try_exists()?
        || store
            .cache
            .root
            .checked(&relative.with_extension("json"))?
            .try_exists()?
    {
        return Err(StoreError::RecoveryRequired {
            pair: model.pair(),
            detail: "pending installation transaction".into(),
        });
    }
    let parent = store.cache.root.directory(Path::new(".install"))?;
    let temporary = tempfile::tempdir_in(parent)?;
    let journal = Journal {
        schema_version: 1,
        pair: model.pair(),
        previous: previous.map(InstallationManifest::for_model),
        next: InstallationManifest::for_model(model),
        committed: false,
    };
    let bytes = serde_json::to_vec(&journal).map_err(|e| invalid(e.to_string()))?;
    let mut file = File::create(temporary.path().join("journal.json"))?;
    std::io::Write::write_all(&mut file, &bytes)?;
    file.sync_all()?;
    drop(file);
    fs::create_dir(temporary.path().join("staged"))?;
    fs::rename(temporary.path(), store.cache.root.checked(&relative)?)?;
    store
        .cache
        .root
        .atomic_write(&relative.with_extension("json"), &bytes)?;
    Ok(relative.join("staged"))
}

#[cfg(any(test, feature = "model-downloads"))]
pub(crate) fn publish(store: &ModelStore, pair: LanguagePair) -> Result<Vec<String>, StoreError> {
    let relative = area(".install", pair);
    let mut journal = read_journal(store, pair)?;
    let installed = ModelStore::relative(pair)?;
    let staged = relative.join("staged");
    expect_manifest(store, &staged, &journal.next)?;
    check_tree(&store.cache.root.checked(&staged)?)?;
    if let Some(previous) = &journal.previous {
        expect_manifest(store, &installed, previous)?;
        check_tree(&store.cache.root.checked(&installed)?)?;
    }
    let result = (|| -> Result<(), StoreError> {
        checkpoint("prepared")?;
        if journal.previous.is_some() {
            fs::rename(
                store.cache.root.checked(&installed)?,
                store.cache.root.checked(&relative.join("backup"))?,
            )?;
        }
        checkpoint("backup")?;
        fs::rename(
            store.cache.root.checked(&staged)?,
            store.cache.root.checked(&installed)?,
        )?;
        checkpoint("published")?;
        journal.committed = true;
        let bytes = serde_json::to_vec(&journal).map_err(|e| invalid(e.to_string()))?;
        store
            .cache
            .root
            .atomic_write(&relative.with_extension("json"), &bytes)?;
        Ok(())
    })();
    if let Err(error) = result {
        recover_pair(store, pair).map_err(|recovery| StoreError::RecoveryRequired {
            pair,
            detail: format!("{error}; rollback failed: {recovery}"),
        })?;
        return Err(error);
    }
    if let Err(error) = checkpoint("committed") {
        return Ok(vec![format!(
            "installed successfully; cleanup pending: {error}"
        )]);
    }
    match recover_pair(store, pair) {
        Ok(outcome) => Ok(outcome.warnings),
        Err(error) => Ok(vec![format!(
            "installed successfully; cleanup pending: {error}"
        )]),
    }
}

fn recover_pair(store: &ModelStore, pair: LanguagePair) -> Result<RecoveryOutcome, StoreError> {
    let relative = area(".install", pair);
    let external = relative.with_extension("json");
    if !store.cache.root.checked(&relative)?.try_exists()?
        && !store.cache.root.checked(&external)?.try_exists()?
    {
        return Ok(RecoveryOutcome::default());
    }
    let journal = read_journal(store, pair)?;
    let bytes = serde_json::to_vec(&journal).map_err(|e| std::io::Error::other(e.to_string()))?;
    store.cache.root.atomic_write(&external, &bytes)?;
    let installed = ModelStore::relative(pair)?;
    let backup = relative.join("backup");
    if journal.committed {
        expect_manifest(store, &installed, &journal.next)?;
    } else if store.cache.root.checked(&backup)?.try_exists()? {
        let previous = journal
            .previous
            .as_ref()
            .ok_or_else(|| StoreError::Unmanaged("unexpected transaction backup".into()))?;
        expect_manifest(store, &backup, previous)?;
        if store.cache.root.checked(&installed)?.try_exists()? {
            expect_manifest(store, &installed, &journal.next)?;
            fs::rename(
                store.cache.root.checked(&installed)?,
                store.cache.root.checked(&relative.join("discard"))?,
            )?;
        }
        fs::rename(
            store.cache.root.checked(&backup)?,
            store.cache.root.checked(&installed)?,
        )?;
    } else if let Some(previous) = &journal.previous {
        expect_manifest(store, &installed, previous)?;
    } else if store.cache.root.checked(&installed)?.try_exists()? {
        expect_manifest(store, &installed, &journal.next)?;
        fs::rename(
            store.cache.root.checked(&installed)?,
            store.cache.root.checked(&relative.join("discard"))?,
        )?;
    }
    let mut outcome = RecoveryOutcome {
        committed: Some(journal.committed),
        warnings: Vec::new(),
    };
    let cleanup = || -> Result<(), StoreError> {
        #[cfg(test)]
        checkpoint("cleanup")?;
        if journal.committed {
            discard(store, pair)?;
        }
        remove_owned(store, &relative)?;
        fs::remove_file(store.cache.root.checked(&external)?)?;
        Ok(())
    };
    if let Err(error) = cleanup() {
        outcome.warnings.push(format!(
            "cleanup pending at {}: {error}",
            store.root().join(&relative).display()
        ));
    }
    Ok(outcome)
}

pub(crate) fn recover_transaction(
    store: &ModelStore,
    pair: LanguagePair,
) -> Result<RecoveryOutcome, StoreError> {
    recover_pair(store, pair).map_err(|error| StoreError::RecoveryRequired {
        pair,
        detail: error.to_string(),
    })
}

#[cfg(feature = "model-downloads")]
pub(crate) fn recover_locked(
    store: &ModelStore,
    pair: LanguagePair,
) -> Result<RecoveryOutcome, StoreError> {
    let mut outcome = recover_transaction(store, pair)?;
    if let Err(error) = remove_partials(store, pair) {
        match error {
            StoreError::Io(_) | StoreError::MetadataIo { .. } => {
                outcome.warnings.push(error.to_string())
            }
            error => return Err(error),
        }
    }
    Ok(outcome)
}

pub(crate) fn recover_on_open(store: &ModelStore) -> Result<RecoveryReport, StoreError> {
    let mut report = RecoveryReport::default();
    let pairs = [
        LanguagePair {
            source: Language::RUSSIAN,
            target: Language::ENGLISH,
        },
        LanguagePair {
            source: Language::ENGLISH,
            target: Language::RUSSIAN,
        },
    ];
    let _global = match global_lock(store, pairs[0]) {
        Ok(guard) => guard,
        Err(StoreError::Busy(_)) => {
            report.installer_busy = true;
            return Ok(report);
        }
        Err(error) => return Err(error),
    };
    for pair in pairs {
        let relative = area(".install", pair);
        let has_transaction = store.cache.root.checked(&relative)?.try_exists()?
            || store
                .cache
                .root
                .checked(&relative.with_extension("json"))?
                .try_exists()?;
        let has_downloads = store
            .cache
            .root
            .checked(&area(".downloads", pair))?
            .try_exists()?;
        if !has_transaction && !has_downloads {
            continue;
        }
        let _guard = match store.exclusive(pair) {
            Ok(guard) => guard,
            Err(StoreError::Busy(_)) => {
                report.installer_busy = true;
                continue;
            }
            Err(error) => return Err(error),
        };
        if has_transaction {
            let outcome = recover_transaction(store, pair)?;
            match outcome.committed {
                Some(true) if outcome.warnings.is_empty() => report.cleaned_up.push(pair),
                Some(false) => report.rolled_back.push(pair),
                _ => {}
            }
            report.warnings.extend(outcome.warnings);
        }
        if let Err(error) = remove_partials(store, pair) {
            report.warnings.push(error.to_string());
        }
    }
    Ok(report)
}

#[cfg(not(test))]
#[cfg(feature = "model-downloads")]
fn checkpoint(_: &str) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
fn checkpoint(phase: &str) -> std::io::Result<()> {
    if std::env::var("SYNTH_TRANSLATE_CRASH_PHASE").as_deref() == Ok(phase) {
        std::process::exit(77);
    }
    FAIL_PHASE.with(|value| {
        if value.borrow().as_deref() == Some(phase) {
            Err(std::io::Error::other(format!("injected {phase} failure")))
        } else {
            Ok(())
        }
    })
}

#[cfg(test)]
thread_local! { pub(super) static FAIL_PHASE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) }; }

#[cfg(test)]
mod tests;
