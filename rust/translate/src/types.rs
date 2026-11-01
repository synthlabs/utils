use std::{fmt, path::PathBuf, str::FromStr};

/// An ISO 639-3 language identifier, independent of the detector's coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Language([u8; 3]);

impl Language {
    pub const ENGLISH: Self = Self(*b"eng");
    pub const RUSSIAN: Self = Self(*b"rus");

    /// Validate the three-letter form and normalize ASCII case.
    /// Callers are responsible for using assigned ISO 639-3 codes.
    pub fn new(code: &str) -> Result<Self, TranslationError> {
        if code.len() != 3 || !code.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err(TranslationError::InvalidLanguage(code.to_owned()));
        }
        let mut bytes = [0; 3];
        bytes.copy_from_slice(code.as_bytes());
        bytes.make_ascii_lowercase();
        Ok(Self(bytes))
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).expect("language contains three ASCII letters")
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Language {
    type Err = TranslationError;

    fn from_str(code: &str) -> Result<Self, Self::Err> {
        Self::new(code)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguagePair {
    pub source: Language,
    pub target: Language,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    pub fn from_hex(hex: &str) -> Result<Self, TranslationError> {
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(TranslationError::InvalidChecksum(hex.to_owned()));
        }
        let mut bytes = [0; 32];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16)
                .expect("checksum contains validated hex pairs");
        }
        Ok(Self(bytes))
    }

    pub(crate) fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Sha256Digest {
    type Err = TranslationError;

    fn from_str(hex: &str) -> Result<Self, Self::Err> {
        Self::from_hex(hex)
    }
}

#[derive(Clone, Debug)]
pub struct ModelArtifact {
    pub path: PathBuf,
    /// Hash of the uncompressed file consumed by the engine.
    pub sha256: Sha256Digest,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoadMode {
    #[default]
    Lazy,
    Eager,
}

#[derive(Clone, Debug)]
pub struct ModelDescriptor {
    pub id: String,
    pub pair: LanguagePair,
    pub model: ModelArtifact,
    pub source_tokenizer: ModelArtifact,
    pub target_tokenizer: ModelArtifact,
    pub load_mode: LoadMode,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SourceLanguage {
    #[default]
    Auto,
    Explicit(Language),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DetectionPolicy {
    #[default]
    ReliableOnly,
    BestGuess,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Detection {
    pub language: Language,
    /// Detector score, not a calibrated probability of correctness.
    pub confidence: f64,
    pub reliable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TranslationRequest {
    pub text: String,
    pub source: SourceLanguage,
    pub target: Language,
    pub detection_policy: DetectionPolicy,
}

impl TranslationRequest {
    pub fn new(text: impl Into<String>, target: Language) -> Self {
        Self {
            text: text.into(),
            source: SourceLanguage::Auto,
            target,
            detection_policy: DetectionPolicy::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Translation {
    pub text: String,
    pub source: Language,
    pub target: Language,
    pub model_id: String,
    /// Absent when the caller supplied an explicit source language.
    pub detection: Option<Detection>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SkipReason {
    BlankInput,
    LanguageUndetermined,
    UnreliableDetection(Detection),
    SameAsTarget(Language),
    ModelNotRegistered(LanguagePair),
    UnsupportedPair(LanguagePair),
    ModelNotInstalled(LanguagePair),
}

#[derive(Clone, Debug, PartialEq)]
pub enum TranslationOutcome {
    Translated(Translation),
    Skipped(SkipReason),
}

pub type TranslationResult = Result<TranslationOutcome, TranslationError>;

#[derive(Clone, Copy, Debug)]
pub struct CoreOptions {
    /// Source tokenizer count, including EOS; must be between 1 and 128.
    pub max_source_tokens: usize,
    pub native_batch_size: usize,
    /// Per-model batch worker count, limited to four. This is not a global budget.
    pub threads: usize,
}

impl Default for CoreOptions {
    fn default() -> Self {
        Self {
            max_source_tokens: fxtranslate::segment::CONTEXT_WINDOW,
            native_batch_size: 16,
            threads: std::thread::available_parallelism()
                .map(|count| count.get().min(4))
                .unwrap_or(1),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TranslationError {
    #[error("expected a three-letter ISO 639-3 code, got {0:?}")]
    InvalidLanguage(String),
    #[error("expected a 64-digit SHA-256 hex digest, got {0:?}")]
    InvalidChecksum(String),
    #[error("invalid core configuration: {0}")]
    InvalidOptions(String),
    #[error("invalid model descriptor: {0}")]
    InvalidModel(String),
    #[error("model ID {0:?} is already registered")]
    DuplicateModelId(String),
    #[error("a model for {0:?} is already registered")]
    DuplicatePair(LanguagePair),
    #[error("cannot access {path}: {detail}")]
    FileAccess { path: PathBuf, detail: String },
    #[error("SHA-256 mismatch for {path}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: PathBuf,
        expected: Sha256Digest,
        actual: Sha256Digest,
    },
    #[error("cannot load model {model_id:?}: {detail}")]
    ModelLoad { model_id: String, detail: String },
    #[error("source has {actual} tokens including EOS; limit is {limit}")]
    InputTooLong { actual: usize, limit: usize },
    #[error("model {model_id:?} inference failed: {detail}")]
    Inference { model_id: String, detail: String },
}
