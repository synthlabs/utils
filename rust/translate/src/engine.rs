use std::{
    collections::HashSet,
    fs::File,
    io,
    panic::{catch_unwind, AssertUnwindSafe},
    path::Path,
    sync::Arc,
};

use fxtranslate::{engine::Engine, spm::SpmVocab, weights::Weights};

use sha2::{Digest, Sha256};

use crate::{CoreOptions, ModelDescriptor, Sha256Digest, TranslationError};

pub(crate) trait InferenceEngine: Send + Sync {
    fn source_token_count(&self, text: &str) -> usize;
    fn translate_batch(&self, texts: &[&str]) -> Result<Vec<String>, String>;
}

pub(crate) trait EngineLoader: Send + Sync {
    fn load(
        &self,
        descriptor: &ModelDescriptor,
        options: CoreOptions,
    ) -> Result<Arc<dyn InferenceEngine>, String>;
}

pub(crate) fn backend_call<T>(
    stage: &str,
    call: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(call)).unwrap_or_else(|payload| {
        let detail = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("non-string panic payload");
        Err(format!("backend panicked during {stage}: {detail}"))
    })
}

pub(crate) fn canonical_descriptor(
    descriptor: &ModelDescriptor,
) -> Result<ModelDescriptor, TranslationError> {
    let mut resolved = descriptor.clone();
    for artifact in [
        &mut resolved.model,
        &mut resolved.source_tokenizer,
        &mut resolved.target_tokenizer,
    ] {
        artifact.path =
            artifact
                .path
                .canonicalize()
                .map_err(|error| TranslationError::FileAccess {
                    path: artifact.path.clone(),
                    detail: error.to_string(),
                })?;
    }
    Ok(resolved)
}

pub(crate) fn verify_artifacts(descriptor: &ModelDescriptor) -> Result<(), TranslationError> {
    let mut checked = HashSet::new();
    for artifact in [
        &descriptor.model,
        &descriptor.source_tokenizer,
        &descriptor.target_tokenizer,
    ] {
        if !checked.insert((&artifact.path, artifact.sha256)) {
            continue;
        }
        let verify = || -> io::Result<Sha256Digest> {
            let mut file = File::open(&artifact.path)?;
            if !file.metadata()?.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "not a regular file",
                ));
            }
            let mut digest = Sha256::new();
            io::copy(&mut file, &mut digest)?;
            Ok(Sha256Digest::from_bytes(digest.finalize().into()))
        };
        let actual = verify().map_err(|error| TranslationError::FileAccess {
            path: artifact.path.clone(),
            detail: error.to_string(),
        })?;
        if actual != artifact.sha256 {
            return Err(TranslationError::ChecksumMismatch {
                path: artifact.path.clone(),
                expected: artifact.sha256,
                actual,
            });
        }
    }
    Ok(())
}

pub(crate) struct FxLoader;

impl EngineLoader for FxLoader {
    fn load(
        &self,
        descriptor: &ModelDescriptor,
        options: CoreOptions,
    ) -> Result<Arc<dyn InferenceEngine>, String> {
        let source = load_vocab(&descriptor.source_tokenizer.path)?;
        let target = load_vocab(&descriptor.target_tokenizer.path)?;
        let weights = Weights::load_mmapped(&descriptor.model.path)?;
        let engine = Engine::new(weights, source, target).with_threads(options.threads);
        Ok(Arc::new(FxEngine(engine)))
    }
}

fn load_vocab(path: &Path) -> Result<SpmVocab, String> {
    let vocab = SpmVocab::load(path).map_err(|error| format!("{}: {error}", path.display()))?;
    if vocab.is_empty() {
        return Err(format!("{}: tokenizer vocabulary is empty", path.display()));
    }
    if vocab.piece(vocab.eos_id()) != "</s>"
        || !(0..vocab.len()).any(|id| vocab.piece(id as u32) == "<unk>")
    {
        return Err(format!(
            "{}: tokenizer must contain EOS (</s>) and UNK (<unk>) pieces",
            path.display()
        ));
    }
    Ok(vocab)
}

struct FxEngine(fxtranslate::engine::Engine);

impl InferenceEngine for FxEngine {
    fn source_token_count(&self, text: &str) -> usize {
        self.0.src_ids(text).len()
    }

    fn translate_batch(&self, texts: &[&str]) -> Result<Vec<String>, String> {
        Ok(self.0.translate_batch(texts))
    }
}

#[cfg(test)]
mod tests;
