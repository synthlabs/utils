use std::{
    fs::{self, File, TryLockError},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    catalog::{supported_pair, POINTER_LIMIT},
    engine::verify_artifacts,
    filesystem::check_tree,
    CatalogCache, CatalogError, CatalogModel, CatalogSnapshot, LanguagePair, LoadMode,
    ModelArtifact, ModelDescriptor, TranslationError,
};

pub const INSTALL_MANIFEST_FILENAME: &str = "install.json";

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("unsupported store language pair: {0:?}")]
    UnsupportedPair(LanguagePair),
    #[error("model is in use or being modified: {0:?}")]
    Busy(LanguagePair),
    #[error("model is not installed: {0:?}")]
    NotInstalled(LanguagePair),
    #[error("unmanaged installation: {0}")]
    Unmanaged(String),
    #[error("installation filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Verification(#[from] TranslationError),
}

/// Installer handoff: retain the referenced snapshot before publishing this file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct InstallationManifest {
    schema_version: u32,
    source: String,
    target: String,
    catalog_sha256: String,
    model_id: String,
}

impl InstallationManifest {
    pub fn for_model(model: &CatalogModel) -> Self {
        Self {
            schema_version: 1,
            source: model.pair().source.to_string(),
            target: model.pair().target.to_string(),
            catalog_sha256: model.snapshot_sha256().to_string(),
            model_id: model.id().to_owned(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstallationState {
    NotInstalled,
    PresentUnverified,
    Incomplete,
    Invalid,
    Unmanaged,
    Busy,
}

#[derive(Clone, Debug)]
pub struct InventoryEntry {
    pub directory: PathBuf,
    pub pair: Option<LanguagePair>,
    pub state: InstallationState,
    pub installed: Option<CatalogModel>,
    /// None if no current catalog was supplied, or this is an unmanaged folder.
    pub matches_current_catalog: Option<bool>,
    pub detail: Option<String>,
}

/// Private descriptor ownership prevents accidentally detaching the lock.
pub struct ModelLease {
    pub(crate) descriptor: ModelDescriptor,
    model: CatalogModel,
    _lock: File,
}

impl ModelLease {
    pub fn model(&self) -> &CatalogModel {
        &self.model
    }
}

/// All installer writes must hold this guard, including final publication.
/// It does not create, download, replace, or publish model files.
pub struct ModelMutationGuard {
    directory: PathBuf,
    _lock: File,
}

impl ModelMutationGuard {
    pub fn directory(&self) -> &Path {
        &self.directory
    }
}

#[derive(Clone, Debug)]
pub struct ModelStore {
    cache: CatalogCache,
}

impl ModelStore {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, StoreError> {
        let cache = CatalogCache::open(root)?;
        cache.root.directory(Path::new("models"))?;
        cache.root.directory(Path::new(".locks"))?;
        Ok(Self { cache })
    }

    pub fn root(&self) -> &Path {
        self.cache.root.path()
    }

    pub fn retain_catalog(&self, catalog: &CatalogSnapshot) -> Result<(), StoreError> {
        self.cache.retain(catalog)?;
        Ok(())
    }

    fn relative(pair: LanguagePair) -> Result<PathBuf, StoreError> {
        if !supported_pair(pair) {
            return Err(StoreError::UnsupportedPair(pair));
        }
        Ok(Path::new("models").join(format!("{}-{}", pair.source, pair.target)))
    }

    fn directory(&self, pair: LanguagePair) -> Result<PathBuf, StoreError> {
        Ok(self.cache.root.checked(&Self::relative(pair)?)?)
    }

    fn lock(&self, pair: LanguagePair, exclusive: bool) -> Result<File, StoreError> {
        Self::relative(pair)?;
        let file = self
            .cache
            .root
            .lock_file(&format!("{}-{}.lock", pair.source, pair.target))?;
        let result = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        match result {
            Ok(()) => Ok(file),
            Err(TryLockError::WouldBlock) => Err(StoreError::Busy(pair)),
            Err(TryLockError::Error(error)) => Err(StoreError::Io(error)),
        }
    }

    fn installed(&self, pair: LanguagePair) -> Result<CatalogModel, StoreError> {
        let directory = self.directory(pair)?;
        match fs::symlink_metadata(&directory) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::NotInstalled(pair))
            }
            Err(error) => return Err(error.into()),
            Ok(metadata) if !metadata.is_dir() => {
                return Err(StoreError::Unmanaged(
                    "installation is not a directory".into(),
                ))
            }
            Ok(_) => {}
        }
        let read = || -> Result<CatalogModel, Box<dyn std::error::Error>> {
            let bytes = self.cache.root.read(
                &Self::relative(pair)?.join(INSTALL_MANIFEST_FILENAME),
                POINTER_LIMIT,
            )?;
            let manifest: InstallationManifest = serde_json::from_slice(&bytes)?;
            if manifest.schema_version != 1
                || manifest.source != pair.source.as_str()
                || manifest.target != pair.target.as_str()
            {
                return Err("invalid installation schema or language pair".into());
            }
            let snapshot = self.cache.load_snapshot(&manifest.catalog_sha256)?;
            let model = snapshot
                .model(pair)
                .ok_or("installation references a missing catalog model")?;
            if model.id() != manifest.model_id {
                return Err("installation revision does not match pinned catalog".into());
            }
            Ok(model.clone())
        };
        read().map_err(|error| StoreError::Unmanaged(error.to_string()))
    }

    fn descriptor(&self, model: &CatalogModel) -> Result<ModelDescriptor, StoreError> {
        let base = Self::relative(model.pair())?;
        let artifact = |file: &crate::CatalogArtifact| -> Result<ModelArtifact, StoreError> {
            Ok(ModelArtifact {
                path: self.cache.root.checked(&base.join(&file.filename))?,
                sha256: file.decompressed_sha256,
            })
        };
        Ok(ModelDescriptor {
            id: model.id().to_owned(),
            pair: model.pair(),
            model: artifact(model.model())?,
            source_tokenizer: artifact(model.source_tokenizer())?,
            target_tokenizer: artifact(model.target_tokenizer())?,
            load_mode: LoadMode::Lazy,
        })
    }

    pub fn acquire(&self, pair: LanguagePair) -> Result<ModelLease, StoreError> {
        let lock = self.lock(pair, false)?;
        let model = self.installed(pair)?;
        self.check_sizes(&model)?;
        let descriptor = self.descriptor(&model)?;
        verify_artifacts(&descriptor)?;
        Ok(ModelLease {
            descriptor,
            model,
            _lock: lock,
        })
    }

    pub fn verify(&self, pair: LanguagePair) -> Result<CatalogModel, StoreError> {
        Ok(self.acquire(pair)?.model.clone())
    }

    pub fn exclusive(&self, pair: LanguagePair) -> Result<ModelMutationGuard, StoreError> {
        let lock = self.lock(pair, true)?;
        Ok(ModelMutationGuard {
            directory: self.directory(pair)?,
            _lock: lock,
        })
    }

    /// Returns false for an absent installation. Corrupt artifacts remain removable.
    pub fn remove(&self, pair: LanguagePair) -> Result<bool, StoreError> {
        let guard = self.exclusive(pair)?;
        match self.installed(pair) {
            Err(StoreError::NotInstalled(_)) => return Ok(false),
            result => {
                result?;
            }
        }
        let directory = self.directory(pair)?;
        if directory != guard.directory
            || !directory
                .canonicalize()?
                .starts_with(self.root().join("models"))
        {
            return Err(StoreError::Unmanaged(
                "deletion target escaped the models directory".into(),
            ));
        }
        check_tree(&directory)?;
        // Preserve identification if deleting an artifact fails partway through.
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name();
            if name == INSTALL_MANIFEST_FILENAME
                || (cfg!(windows)
                    && name
                        .as_encoded_bytes()
                        .eq_ignore_ascii_case(INSTALL_MANIFEST_FILENAME.as_bytes()))
            {
                continue;
            }
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(entry.path())?;
            } else {
                fs::remove_file(entry.path())?;
            }
        }
        fs::remove_file(directory.join(INSTALL_MANIFEST_FILENAME))?;
        fs::remove_dir(directory)?;
        Ok(true)
    }

    fn check_sizes(&self, model: &CatalogModel) -> Result<(), StoreError> {
        let base = Self::relative(model.pair())?;
        for file in model.required_files() {
            let path = self.cache.root.checked(&base.join(&file.filename))?;
            let metadata = fs::metadata(&path)?;
            if !metadata.is_file() || metadata.len() != file.decompressed_size {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("unexpected type or size for {}", path.display()),
                )
                .into());
            }
        }
        Ok(())
    }

    pub fn inventory(
        &self,
        current: Option<&CatalogSnapshot>,
    ) -> Result<Vec<InventoryEntry>, StoreError> {
        let pairs = [
            LanguagePair {
                source: crate::Language::RUSSIAN,
                target: crate::Language::ENGLISH,
            },
            LanguagePair {
                source: crate::Language::ENGLISH,
                target: crate::Language::RUSSIAN,
            },
        ];
        let mut entries = Vec::new();
        for pair in pairs {
            let relative = Self::relative(pair)?;
            let mut entry = InventoryEntry {
                directory: self.root().join(&relative),
                pair: Some(pair),
                state: InstallationState::NotInstalled,
                installed: None,
                matches_current_catalog: None,
                detail: None,
            };
            let inspect = || -> Result<(File, CatalogModel), StoreError> {
                let lock = self.lock(pair, false)?;
                Ok((lock, self.installed(pair)?))
            };
            match inspect() {
                Ok((_lock, model)) => {
                    entry.state = match self.check_sizes(&model) {
                        Ok(()) => InstallationState::PresentUnverified,
                        Err(StoreError::Io(error))
                            if error.kind() == std::io::ErrorKind::NotFound =>
                        {
                            entry.detail = Some(error.to_string());
                            InstallationState::Incomplete
                        }
                        Err(error) => {
                            entry.detail = Some(error.to_string());
                            InstallationState::Invalid
                        }
                    };
                    entry.matches_current_catalog = current
                        .map(|catalog| catalog.model(pair).is_some_and(|m| m.id() == model.id()));
                    entry.installed = Some(model);
                }
                Err(StoreError::NotInstalled(_)) => {}
                Err(StoreError::Busy(_)) => entry.state = InstallationState::Busy,
                Err(StoreError::Unmanaged(detail)) => {
                    entry.state = InstallationState::Unmanaged;
                    entry.detail = Some(detail);
                }
                Err(error) => {
                    entry.state = InstallationState::Invalid;
                    entry.detail = Some(error.to_string());
                }
            }
            entries.push(entry);
        }
        for item in fs::read_dir(self.cache.root.checked(Path::new("models"))?)? {
            let item = item?;
            if ["rus-eng", "eng-rus"]
                .iter()
                .any(|name| item.file_name() == *name)
            {
                continue;
            }
            entries.push(InventoryEntry {
                directory: item.path(),
                pair: None,
                state: InstallationState::Unmanaged,
                installed: None,
                matches_current_catalog: None,
                detail: Some("directory is not managed by this store".into()),
            });
        }
        entries.sort_by(|a, b| a.directory.cmp(&b.directory));
        Ok(entries)
    }
}

#[cfg(test)]
pub(crate) mod tests;
