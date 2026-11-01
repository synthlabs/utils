use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{CatalogError, InstallError, StoreError, TranslationError, WorkloadError};

macro_rules! codes {
    ($($variant:ident),* $(,)?) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
        #[serde(rename_all = "snake_case")]
        pub enum ErrorCode { $($variant),* }
        impl ErrorCode {
            pub const ALL: &'static [Self] = &[$(Self::$variant),*];
        }
    };
}

codes! {
    NotReady, Unavailable, Paused, Shutdown, OperationNotFound, WrongOwner,
    InvalidPlan, InvalidLanguage, InvalidChecksum, InvalidOptions, InvalidModel,
    DuplicateModelId, DuplicatePair, FileAccess, ChecksumMismatch, ModelLoad,
    InputTooLong, Inference, Capacity, Evicted, Cancelled, WorkerFailed, WorkerStart,
    UnsupportedPair, Busy, NotInstalled, Unmanaged, RecoveryRequired, CleanupPending,
    MetadataIo, UnmanagedDownloads, Io, CatalogInvalid, CatalogHttp, CatalogIo,
    ControlUsed, ModelUnavailable, Transport, Http, Integrity, Decompression, Worker
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type, thiserror::Error)]
#[error("{message}")]
#[serde(rename_all = "camelCase")]
pub struct ServiceError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default)]
    pub details: BTreeMap<String, String>,
}

impl ServiceError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: BTreeMap::new(),
        }
    }

    fn detail(mut self, key: &str, value: impl ToString) -> Self {
        self.details.insert(key.into(), value.to_string());
        self
    }
}

impl From<TranslationError> for ServiceError {
    fn from(error: TranslationError) -> Self {
        let code = match &error {
            TranslationError::InvalidLanguage(_) => ErrorCode::InvalidLanguage,
            TranslationError::InvalidChecksum(_) => ErrorCode::InvalidChecksum,
            TranslationError::InvalidOptions(_) => ErrorCode::InvalidOptions,
            TranslationError::InvalidModel(_) => ErrorCode::InvalidModel,
            TranslationError::DuplicateModelId(_) => ErrorCode::DuplicateModelId,
            TranslationError::DuplicatePair(_) => ErrorCode::DuplicatePair,
            TranslationError::FileAccess { .. } => ErrorCode::FileAccess,
            TranslationError::ChecksumMismatch { .. } => ErrorCode::ChecksumMismatch,
            TranslationError::ModelLoad { .. } => ErrorCode::ModelLoad,
            TranslationError::InputTooLong { .. } => ErrorCode::InputTooLong,
            TranslationError::Inference { .. } => ErrorCode::Inference,
        };
        let mapped = Self::new(code, error.to_string());
        match error {
            TranslationError::FileAccess { path, detail } => mapped
                .detail("path", path.display())
                .detail("detail", detail),
            TranslationError::ChecksumMismatch {
                path,
                expected,
                actual,
            } => mapped
                .detail("path", path.display())
                .detail("expected", expected)
                .detail("actual", actual),
            TranslationError::ModelLoad { model_id, detail }
            | TranslationError::Inference { model_id, detail } => {
                mapped.detail("modelId", model_id).detail("detail", detail)
            }
            TranslationError::InputTooLong { actual, limit } => {
                mapped.detail("actual", actual).detail("limit", limit)
            }
            _ => mapped,
        }
    }
}

impl From<CatalogError> for ServiceError {
    fn from(error: CatalogError) -> Self {
        let code = match &error {
            CatalogError::Invalid(_) => ErrorCode::CatalogInvalid,
            CatalogError::ChecksumMismatch { .. } => ErrorCode::ChecksumMismatch,
            CatalogError::Io(_) => ErrorCode::CatalogIo,
            CatalogError::Http(_) => ErrorCode::CatalogHttp,
        };
        let mapped = Self::new(code, error.to_string());
        match error {
            CatalogError::ChecksumMismatch { expected, actual } => {
                mapped.detail("expected", expected).detail("actual", actual)
            }
            _ => mapped,
        }
    }
}

impl From<&StoreError> for ServiceError {
    fn from(error: &StoreError) -> Self {
        let code = match error {
            StoreError::UnsupportedPair(_) => ErrorCode::UnsupportedPair,
            StoreError::Busy(_) => ErrorCode::Busy,
            StoreError::NotInstalled(_) => ErrorCode::NotInstalled,
            StoreError::Unmanaged(_) => ErrorCode::Unmanaged,
            StoreError::RecoveryRequired { .. } => ErrorCode::RecoveryRequired,
            StoreError::CleanupPending { .. } => ErrorCode::CleanupPending,
            StoreError::MetadataIo { .. } => ErrorCode::MetadataIo,
            StoreError::UnmanagedDownloads { .. } => ErrorCode::UnmanagedDownloads,
            StoreError::Io(_) => ErrorCode::Io,
            StoreError::Catalog(error) => return error.clone().into(),
            StoreError::Verification(error) => return error.clone().into(),
        };
        let mapped = Self::new(code, error.to_string());
        match error {
            StoreError::UnsupportedPair(pair)
            | StoreError::Busy(pair)
            | StoreError::NotInstalled(pair)
            | StoreError::RecoveryRequired { pair, .. }
            | StoreError::CleanupPending { pair, .. }
            | StoreError::UnmanagedDownloads { pair, .. } => mapped
                .detail("source", pair.source)
                .detail("target", pair.target),
            StoreError::MetadataIo { path, source } => mapped
                .detail("path", path.display())
                .detail("detail", source),
            _ => mapped,
        }
    }
}

impl From<StoreError> for ServiceError {
    fn from(error: StoreError) -> Self {
        Self::from(&error)
    }
}

impl From<WorkloadError> for ServiceError {
    fn from(error: WorkloadError) -> Self {
        let code = match &error {
            WorkloadError::InvalidOptions(_) => ErrorCode::InvalidOptions,
            WorkloadError::Capacity => ErrorCode::Capacity,
            WorkloadError::Evicted => ErrorCode::Evicted,
            WorkloadError::Cancelled => ErrorCode::Cancelled,
            WorkloadError::Shutdown => ErrorCode::Shutdown,
            WorkloadError::WorkerFailed => ErrorCode::WorkerFailed,
            WorkloadError::WorkerStart(_) => ErrorCode::WorkerStart,
            WorkloadError::Core(error) => return error.clone().into(),
            WorkloadError::Store(error) => return Self::from(error.as_ref()),
        };
        Self::new(code, error.to_string())
    }
}

impl From<InstallError> for ServiceError {
    fn from(error: InstallError) -> Self {
        let code = match &error {
            InstallError::Cancelled => ErrorCode::Cancelled,
            InstallError::ControlUsed => ErrorCode::ControlUsed,
            InstallError::Unavailable(_) => ErrorCode::ModelUnavailable,
            InstallError::Store(error) => return Self::from(error),
            InstallError::Transport(_) => ErrorCode::Transport,
            InstallError::Http(_) => ErrorCode::Http,
            InstallError::Integrity { .. } => ErrorCode::Integrity,
            InstallError::Decompression { .. } => ErrorCode::Decompression,
            InstallError::Io(_) => ErrorCode::Io,
            InstallError::Worker(_) => ErrorCode::Worker,
        };
        Self::new(code, error.to_string())
    }
}

impl From<tokio::task::JoinError> for ServiceError {
    fn from(error: tokio::task::JoinError) -> Self {
        Self::new(ErrorCode::Worker, error.to_string())
    }
}
