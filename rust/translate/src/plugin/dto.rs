use serde::{Deserialize, Serialize};
use specta::Type;

use super::ServiceError;
use crate::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PairDto {
    pub source: String,
    pub target: String,
}

impl TryFrom<PairDto> for LanguagePair {
    type Error = ServiceError;
    fn try_from(pair: PairDto) -> Result<Self, Self::Error> {
        Ok(Self {
            source: pair.source.parse()?,
            target: pair.target.parse()?,
        })
    }
}

impl From<LanguagePair> for PairDto {
    fn from(pair: LanguagePair) -> Self {
        Self {
            source: pair.source.to_string(),
            target: pair.target.to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DetectionPolicyDto {
    #[default]
    ReliableOnly,
    BestGuess,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RequestDto {
    pub text: String,
    pub target: String,
    pub source: Option<String>,
    #[serde(default)]
    pub detection_policy: DetectionPolicyDto,
}

impl TryFrom<RequestDto> for TranslationRequest {
    type Error = ServiceError;
    fn try_from(request: RequestDto) -> Result<Self, Self::Error> {
        Ok(Self {
            text: request.text,
            target: request.target.parse()?,
            source: match request.source {
                Some(source) => SourceLanguage::Explicit(source.parse()?),
                None => SourceLanguage::Auto,
            },
            detection_policy: match request.detection_policy {
                DetectionPolicyDto::ReliableOnly => DetectionPolicy::ReliableOnly,
                DetectionPolicyDto::BestGuess => DetectionPolicy::BestGuess,
            },
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DetectionDto {
    pub language: String,
    pub confidence: f64,
    pub reliable: bool,
}

impl From<Detection> for DetectionDto {
    fn from(value: Detection) -> Self {
        Self {
            language: value.language.to_string(),
            confidence: value.confidence,
            reliable: value.reliable,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TranslationDto {
    pub text: String,
    pub source: String,
    pub target: String,
    pub model_id: String,
    pub detection: Option<DetectionDto>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkipDto {
    BlankInput,
    LanguageUndetermined,
    UnreliableDetection { detection: DetectionDto },
    SameAsTarget { language: String },
    ModelNotRegistered { pair: PairDto },
    UnsupportedPair { pair: PairDto },
    ModelNotInstalled { pair: PairDto },
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum OutcomeDto {
    Translated { translation: TranslationDto },
    Skipped { reason: SkipDto },
}

impl From<TranslationOutcome> for OutcomeDto {
    fn from(value: TranslationOutcome) -> Self {
        match value {
            TranslationOutcome::Translated(value) => Self::Translated {
                translation: TranslationDto {
                    text: value.text,
                    source: value.source.to_string(),
                    target: value.target.to_string(),
                    model_id: value.model_id,
                    detection: value.detection.map(Into::into),
                },
            },
            TranslationOutcome::Skipped(reason) => Self::Skipped {
                reason: match reason {
                    SkipReason::BlankInput => SkipDto::BlankInput,
                    SkipReason::LanguageUndetermined => SkipDto::LanguageUndetermined,
                    SkipReason::UnreliableDetection(detection) => SkipDto::UnreliableDetection {
                        detection: detection.into(),
                    },
                    SkipReason::SameAsTarget(language) => SkipDto::SameAsTarget {
                        language: language.to_string(),
                    },
                    SkipReason::ModelNotRegistered(pair) => {
                        SkipDto::ModelNotRegistered { pair: pair.into() }
                    }
                    SkipReason::UnsupportedPair(pair) => {
                        SkipDto::UnsupportedPair { pair: pair.into() }
                    }
                    SkipReason::ModelNotInstalled(pair) => {
                        SkipDto::ModelNotInstalled { pair: pair.into() }
                    }
                },
            },
        }
    }
}

/// Byte counts use decimal strings to preserve the full u64 range in JavaScript.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelDto {
    pub id: String,
    pub pair: PairDto,
    pub version: String,
    pub architecture: String,
    pub catalog_sha256: String,
    pub download_bytes: String,
    pub installed_bytes: String,
}

impl From<&CatalogModel> for ModelDto {
    fn from(model: &CatalogModel) -> Self {
        Self {
            id: model.id().into(),
            pair: model.pair().into(),
            version: model.version().into(),
            architecture: model.architecture().into(),
            catalog_sha256: model.snapshot_sha256().to_string(),
            download_bytes: model.download_bytes().to_string(),
            installed_bytes: model.installed_bytes().to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogDto {
    pub sha256: String,
    pub models: Vec<ModelDto>,
    pub license_url: String,
    pub license_sha256: String,
    pub records_url: String,
    pub records_sha256: String,
    pub retrieved_at: String,
}

impl From<&CatalogSnapshot> for CatalogDto {
    fn from(snapshot: &CatalogSnapshot) -> Self {
        let source = snapshot.source();
        Self {
            sha256: snapshot.sha256().to_string(),
            models: snapshot.models().map(Into::into).collect(),
            license_url: source.license_url.clone(),
            license_sha256: source.license_sha256.clone(),
            records_url: source.records_url.clone(),
            records_sha256: source.records_sha256.clone(),
            retrieved_at: source.retrieved_at.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum FreshnessDto {
    Fresh,
    Cached,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogStateDto {
    pub refreshing: bool,
    pub catalog: Option<CatalogDto>,
    pub freshness: Option<FreshnessDto>,
    pub error: Option<ServiceError>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum InstallationStateDto {
    NotInstalled,
    PresentUnverified,
    Incomplete,
    Invalid,
    Unavailable,
    Unmanaged,
    Busy,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InventoryDto {
    pub directory: String,
    pub pair: Option<PairDto>,
    pub state: InstallationStateDto,
    pub installed: Option<ModelDto>,
    pub matches_current_catalog: Option<bool>,
    pub detail: Option<String>,
}

impl From<InventoryEntry> for InventoryDto {
    fn from(value: InventoryEntry) -> Self {
        Self {
            directory: value.directory.to_string_lossy().into_owned(),
            pair: value.pair.map(Into::into),
            state: match value.state {
                InstallationState::NotInstalled => InstallationStateDto::NotInstalled,
                InstallationState::PresentUnverified => InstallationStateDto::PresentUnverified,
                InstallationState::Incomplete => InstallationStateDto::Incomplete,
                InstallationState::Invalid => InstallationStateDto::Invalid,
                InstallationState::Unavailable => InstallationStateDto::Unavailable,
                InstallationState::Unmanaged => InstallationStateDto::Unmanaged,
                InstallationState::Busy => InstallationStateDto::Busy,
            },
            installed: value.installed.as_ref().map(Into::into),
            matches_current_catalog: value.matches_current_catalog,
            detail: value.detail,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlanDto {
    pub pair: PairDto,
    pub catalog_sha256: String,
    pub model_id: String,
    pub model: ModelDto,
}

impl From<&InstallPlan> for InstallPlanDto {
    fn from(plan: &InstallPlan) -> Self {
        let model = plan.model();
        Self {
            pair: model.pair().into(),
            catalog_sha256: model.snapshot_sha256().to_string(),
            model_id: model.id().into(),
            model: model.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum InstallPhaseDto {
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

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgressDto {
    pub phase: InstallPhaseDto,
    pub filename: Option<String>,
    pub file_index: usize,
    pub file_count: usize,
    pub attempt: u32,
    pub max_attempts: u32,
    pub file_download_bytes: String,
    pub file_download_total: String,
    pub completed_download_bytes: String,
    pub download_total: String,
    pub file_installed_bytes: String,
    pub file_installed_total: String,
    pub completed_installed_bytes: String,
    pub installed_total: String,
    pub reused_bytes: String,
    pub network_bytes: String,
    pub detail: Option<String>,
}

impl From<InstallProgress> for InstallProgressDto {
    fn from(value: InstallProgress) -> Self {
        Self {
            phase: match value.phase {
                InstallPhase::Idle => InstallPhaseDto::Idle,
                InstallPhase::Preparing => InstallPhaseDto::Preparing,
                InstallPhase::Downloading => InstallPhaseDto::Downloading,
                InstallPhase::RetryWaiting => InstallPhaseDto::RetryWaiting,
                InstallPhase::Verifying => InstallPhaseDto::Verifying,
                InstallPhase::Decompressing => InstallPhaseDto::Decompressing,
                InstallPhase::Publishing => InstallPhaseDto::Publishing,
                InstallPhase::Completed => InstallPhaseDto::Completed,
                InstallPhase::Cancelled => InstallPhaseDto::Cancelled,
                InstallPhase::Failed => InstallPhaseDto::Failed,
            },
            filename: value.filename,
            file_index: value.file_index,
            file_count: value.file_count,
            attempt: value.attempt,
            max_attempts: value.max_attempts,
            file_download_bytes: value.file_download_bytes.to_string(),
            file_download_total: value.file_download_total.to_string(),
            completed_download_bytes: value.completed_download_bytes.to_string(),
            download_total: value.download_total.to_string(),
            file_installed_bytes: value.file_installed_bytes.to_string(),
            file_installed_total: value.file_installed_total.to_string(),
            completed_installed_bytes: value.completed_installed_bytes.to_string(),
            installed_total: value.installed_total.to_string(),
            reused_bytes: value.reused_bytes.to_string(),
            network_bytes: value.network_bytes.to_string(),
            detail: value.detail,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatusDto {
    Installed,
    AlreadyInstalled,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallResultDto {
    pub status: InstallStatusDto,
    pub model: ModelDto,
    pub cleanup_warnings: Vec<String>,
}

impl From<InstallResult> for InstallResultDto {
    fn from(value: InstallResult) -> Self {
        Self {
            status: match value.status {
                InstallStatus::Installed => InstallStatusDto::Installed,
                InstallStatus::AlreadyInstalled => InstallStatusDto::AlreadyInstalled,
            },
            model: (&value.model).into(),
            cleanup_warnings: value.cleanup_warnings,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InstallationDto {
    pub id: String,
    pub model: ModelDto,
    pub progress: InstallProgressDto,
    pub finished: bool,
    pub result: Option<InstallResultDto>,
    pub error: Option<ServiceError>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResidentDto {
    pub pair: PairDto,
    pub model_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadDto {
    pub queued_requests: usize,
    pub running_requests: usize,
    pub cached_results: usize,
    pub resident_models: Vec<ResidentDto>,
}

impl From<WorkloadSnapshot> for WorkloadDto {
    fn from(value: WorkloadSnapshot) -> Self {
        Self {
            queued_requests: value.queued_requests,
            running_requests: value.running_requests,
            cached_results: value.cached_results,
            resident_models: value
                .resident_models
                .into_iter()
                .map(|v| ResidentDto {
                    pair: v.pair.into(),
                    model_id: v.model_id,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ServicePhase {
    Initializing,
    Ready,
    Unavailable,
    Releasing,
    Paused,
    Shutdown,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryDto {
    pub rolled_back: Vec<PairDto>,
    pub cleaned_up: Vec<PairDto>,
    pub warnings: Vec<String>,
    pub installer_busy: bool,
}

impl From<RecoveryReport> for RecoveryDto {
    fn from(value: RecoveryReport) -> Self {
        Self {
            rolled_back: value.rolled_back.into_iter().map(Into::into).collect(),
            cleaned_up: value.cleaned_up.into_iter().map(Into::into).collect(),
            warnings: value.warnings,
            installer_busy: value.installer_busy,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ServiceSnapshot {
    pub revision: String,
    pub models_revision: String,
    pub phase: ServicePhase,
    pub error: Option<ServiceError>,
    pub catalog: CatalogStateDto,
    pub installation: Option<InstallationDto>,
    pub recovery: RecoveryDto,
    pub workload: Option<WorkloadDto>,
}
