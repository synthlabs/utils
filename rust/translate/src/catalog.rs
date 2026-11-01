use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{filesystem::Root, Language, LanguagePair, Sha256Digest};

#[cfg(feature = "catalog-http")]
mod http;
#[cfg(feature = "catalog-http")]
pub use http::*;

pub const DEFAULT_CATALOG_URL: &str = "https://models.jerod.me/latest.json";
const ORIGIN: &str = "https://models.jerod.me";
pub(crate) const POINTER_LIMIT: usize = 16 * 1024;
pub(crate) const CATALOG_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("invalid model catalog: {0}")]
    Invalid(String),
    #[error("catalog SHA-256 mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("catalog filesystem error: {0}")]
    Io(String),
    #[error("catalog metadata request failed: {0}")]
    Http(String),
}

impl From<std::io::Error> for CatalogError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

fn invalid(detail: impl Into<String>) -> CatalogError {
    CatalogError::Invalid(detail.into())
}

pub(crate) fn hash(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

fn digest(value: &str) -> Result<Sha256Digest, CatalogError> {
    value.parse().map_err(|_| invalid("invalid SHA-256 digest"))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogPointer {
    catalog_url: String,
    sha256: String,
}

impl CatalogPointer {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CatalogError> {
        if bytes.len() > POINTER_LIMIT {
            return Err(invalid("catalog pointer exceeds its size limit"));
        }
        let pointer: Self = serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
        let sha256 = digest(&pointer.sha256)?;
        if pointer.catalog_url != format!("{ORIGIN}/snapshots/{sha256}/catalog.json") {
            return Err(invalid("unexpected catalog snapshot URL"));
        }
        Ok(Self {
            sha256: sha256.to_string(),
            ..pointer
        })
    }

    pub fn catalog_url(&self) -> &str {
        &self.catalog_url
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CatalogSource {
    pub license_sha256: String,
    pub license_url: String,
    pub records_sha256: String,
    pub records_url: String,
    pub retrieved_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArtifactRole {
    Model,
    Vocabulary,
    SourceVocabulary,
    TargetVocabulary,
    Lexicon,
    Other(String),
}

#[derive(Clone, Debug)]
pub struct CatalogArtifact {
    pub filename: String,
    pub role: ArtifactRole,
    pub url: String,
    pub size: u64,
    pub sha256: Sha256Digest,
    pub decompressed_size: u64,
    pub decompressed_sha256: Sha256Digest,
}

#[derive(Clone, Debug)]
pub struct CatalogModel {
    id: String,
    pair: LanguagePair,
    version: String,
    architecture: String,
    snapshot_sha256: Sha256Digest,
    files: Vec<CatalogArtifact>,
    required: [usize; 3],
}

impl CatalogModel {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn pair(&self) -> LanguagePair {
        self.pair
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn architecture(&self) -> &str {
        &self.architecture
    }
    pub fn snapshot_sha256(&self) -> Sha256Digest {
        self.snapshot_sha256
    }
    pub fn files(&self) -> &[CatalogArtifact] {
        &self.files
    }
    pub fn model(&self) -> &CatalogArtifact {
        &self.files[self.required[0]]
    }
    pub fn source_tokenizer(&self) -> &CatalogArtifact {
        &self.files[self.required[1]]
    }
    pub fn target_tokenizer(&self) -> &CatalogArtifact {
        &self.files[self.required[2]]
    }
    pub fn required_files(&self) -> Vec<&CatalogArtifact> {
        let mut indexes = self.required.to_vec();
        indexes.sort_unstable();
        indexes.dedup();
        indexes
            .into_iter()
            .map(|index| &self.files[index])
            .collect()
    }
    pub fn installed_bytes(&self) -> u64 {
        self.required_files()
            .iter()
            .map(|file| file.decompressed_size)
            .sum()
    }
    pub fn download_bytes(&self) -> u64 {
        self.required_files().iter().map(|file| file.size).sum()
    }
}

#[derive(Clone, Debug)]
pub struct CatalogSnapshot {
    sha256: Sha256Digest,
    source: CatalogSource,
    models: BTreeMap<LanguagePair, CatalogModel>,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
struct WireCatalog {
    schema_version: u32,
    source: CatalogSource,
    models: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct WireModel {
    source: String,
    target: String,
    version: String,
    architecture: String,
    filter_expression: String,
    files: Vec<WireArtifact>,
}

#[derive(Deserialize)]
struct WireArtifact {
    filename: String,
    role: String,
    url: String,
    size: u64,
    sha256: String,
    decompressed_size: u64,
    decompressed_sha256: String,
}

impl CatalogSnapshot {
    /// Hashes exact response bytes before interpreting any catalog fields.
    pub fn from_bytes(bytes: &[u8], expected_sha256: &str) -> Result<Self, CatalogError> {
        if bytes.len() > CATALOG_LIMIT {
            return Err(invalid("catalog exceeds its size limit"));
        }
        let expected = digest(expected_sha256)?;
        let actual = hash(bytes);
        if actual != expected {
            return Err(CatalogError::ChecksumMismatch {
                expected: expected.to_string(),
                actual: actual.to_string(),
            });
        }
        let wire: WireCatalog =
            serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
        if wire.schema_version != 1 {
            return Err(invalid("unsupported catalog schema version"));
        }
        digest(&wire.source.license_sha256)?;
        digest(&wire.source.records_sha256)?;
        let mut models = BTreeMap::new();
        for value in wire.models {
            let source = value.get("source").and_then(|v| v.as_str());
            let target = value.get("target").and_then(|v| v.as_str());
            if !matches!(
                (source, target),
                (Some("ru"), Some("en")) | (Some("en"), Some("ru"))
            ) {
                continue;
            }
            let entry: WireModel =
                serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
            match entry.filter_expression.as_str() {
                "env.appinfo.OS == 'Android'" => continue,
                "" | "env.appinfo.OS != 'Android'" => {}
                _ => return Err(invalid("unsupported desktop model filter")),
            }
            let model = normalize(entry, actual)?;
            if models.insert(model.pair, model).is_some() {
                return Err(invalid("ambiguous desktop model for a language pair"));
            }
        }
        Ok(Self {
            sha256: actual,
            source: wire.source,
            models,
            bytes: bytes.to_vec(),
        })
    }

    pub fn sha256(&self) -> Sha256Digest {
        self.sha256
    }
    pub fn source(&self) -> &CatalogSource {
        &self.source
    }
    pub fn models(&self) -> impl Iterator<Item = &CatalogModel> {
        self.models.values()
    }
    pub fn model(&self, pair: LanguagePair) -> Option<&CatalogModel> {
        self.models.get(&pair)
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn pointer(&self) -> CatalogPointer {
        CatalogPointer {
            catalog_url: format!("{ORIGIN}/snapshots/{}/catalog.json", self.sha256),
            sha256: self.sha256.to_string(),
        }
    }
}

pub(crate) fn supported_pair(pair: LanguagePair) -> bool {
    matches!(
        (pair.source, pair.target),
        (Language::RUSSIAN, Language::ENGLISH) | (Language::ENGLISH, Language::RUSSIAN)
    )
}

fn normalize(wire: WireModel, snapshot_sha256: Sha256Digest) -> Result<CatalogModel, CatalogError> {
    let language = |code: &str| {
        if code == "en" {
            Language::ENGLISH
        } else {
            Language::RUSSIAN
        }
    };
    let pair = LanguagePair {
        source: language(&wire.source),
        target: language(&wire.target),
    };
    if wire.version.trim().is_empty() || wire.architecture.trim().is_empty() {
        return Err(invalid("blank model version or architecture"));
    }
    let mut files = Vec::new();
    for artifact in wire.files {
        let filename = &artifact.filename;
        let stem = filename
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'));
        if filename.is_empty()
            || !filename
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            || filename == "."
            || filename == ".."
            || filename.ends_with('.')
            || filename.eq_ignore_ascii_case("install.json")
            || reserved
        {
            return Err(invalid("unsafe artifact filename"));
        }
        if files
            .iter()
            .any(|f: &CatalogArtifact| f.filename.eq_ignore_ascii_case(filename))
        {
            return Err(invalid("duplicate artifact filename"));
        }
        let sha256 = digest(&artifact.sha256)?;
        if artifact.url != format!("{ORIGIN}/objects/sha256/{sha256}.zst") {
            return Err(invalid("unexpected artifact URL"));
        }
        if artifact.size == 0 || artifact.decompressed_size == 0 {
            return Err(invalid("empty artifact"));
        }
        let role = match artifact.role.as_str() {
            "model" => ArtifactRole::Model,
            "vocab" => ArtifactRole::Vocabulary,
            "srcvocab" => ArtifactRole::SourceVocabulary,
            "trgvocab" => ArtifactRole::TargetVocabulary,
            "lex" => ArtifactRole::Lexicon,
            _ => ArtifactRole::Other(artifact.role),
        };
        files.push(CatalogArtifact {
            filename: artifact.filename,
            role,
            url: artifact.url,
            size: artifact.size,
            sha256,
            decompressed_size: artifact.decompressed_size,
            decompressed_sha256: digest(&artifact.decompressed_sha256)?,
        });
    }
    let index = |role| -> Result<Option<usize>, CatalogError> {
        let indexes: Vec<_> = files
            .iter()
            .enumerate()
            .filter(|(_, f)| f.role == role)
            .map(|(i, _)| i)
            .collect();
        if indexes.len() > 1 {
            return Err(invalid("duplicate artifact role"));
        }
        Ok(indexes.first().copied())
    };
    let model = index(ArtifactRole::Model)?.ok_or_else(|| invalid("missing model weights"))?;
    let vocab = index(ArtifactRole::Vocabulary)?;
    let src = index(ArtifactRole::SourceVocabulary)?;
    let trg = index(ArtifactRole::TargetVocabulary)?;
    let (src, trg) = match (vocab, src, trg) {
        (Some(v), None, None) => (v, v),
        (None, Some(s), Some(t)) => (s, t),
        _ => return Err(invalid("missing or ambiguous tokenizer roles")),
    };
    let required = [model, src, trg];
    let identity = serde_json::to_vec(&(
        &wire.version,
        &wire.architecture,
        required.map(|i| {
            (
                files[i].decompressed_sha256.to_string(),
                files[i].decompressed_size,
            )
        }),
    ))
    .map_err(|e| invalid(e.to_string()))?;
    let id = format!(
        "mozilla-{}-{}-{}",
        pair.source,
        pair.target,
        hash(&identity)
    );
    let normalized = CatalogModel {
        id,
        pair,
        version: wire.version,
        architecture: wire.architecture,
        snapshot_sha256,
        files,
        required,
    };
    for compressed in [true, false] {
        normalized
            .required_files()
            .iter()
            .try_fold(0_u64, |total, f| {
                total.checked_add(if compressed {
                    f.size
                } else {
                    f.decompressed_size
                })
            })
            .ok_or_else(|| invalid("artifact sizes overflow"))?;
    }
    Ok(normalized)
}

/// Local metadata only. Opening or reading a cache never makes HTTP requests.
#[derive(Clone, Debug)]
pub struct CatalogCache {
    pub(crate) root: Root,
}

impl CatalogCache {
    pub fn open(store_root: impl AsRef<Path>) -> Result<Self, CatalogError> {
        let root = Root::open(store_root.as_ref())?;
        root.directory(Path::new("catalog"))?;
        Ok(Self { root })
    }
    pub fn load_snapshot(&self, sha256: &str) -> Result<CatalogSnapshot, CatalogError> {
        let sha256 = digest(sha256)?;
        let bytes = self.root.read(
            &Path::new("catalog").join(format!("{sha256}.json")),
            CATALOG_LIMIT,
        )?;
        CatalogSnapshot::from_bytes(&bytes, &sha256.to_string())
    }
    pub fn current(&self) -> Result<CatalogSnapshot, CatalogError> {
        let pointer = CatalogPointer::from_bytes(
            &self
                .root
                .read(Path::new("catalog/current.json"), POINTER_LIMIT)?,
        )?;
        self.load_snapshot(pointer.sha256())
    }
    pub fn retain(&self, snapshot: &CatalogSnapshot) -> Result<(), CatalogError> {
        self.root.atomic_write(
            &Path::new("catalog").join(format!("{}.json", snapshot.sha256)),
            snapshot.as_bytes(),
        )?;
        Ok(())
    }
    pub fn commit(&self, snapshot: &CatalogSnapshot) -> Result<(), CatalogError> {
        let lock = self.root.lock_file("catalog.lock")?;
        lock.try_lock()
            .map_err(|e| CatalogError::Io(e.to_string()))?;
        self.retain(snapshot)?;
        let bytes = serde_json::to_vec(&snapshot.pointer()).map_err(|e| invalid(e.to_string()))?;
        self.root
            .atomic_write(Path::new("catalog/current.json"), &bytes)?;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;
