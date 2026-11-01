use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use crate::{
    detect::{LanguageDetector, WhatlangDetector},
    engine::{
        backend_call, canonical_descriptor, verify_artifacts, EngineLoader, FxLoader,
        InferenceEngine,
    },
    CoreOptions, Detection, DetectionPolicy, LanguagePair, LoadMode, ModelDescriptor, SkipReason,
    SourceLanguage, Translation, TranslationError, TranslationOutcome, TranslationRequest,
    TranslationResult,
};

struct RegisteredModel {
    descriptor: ModelDescriptor,
    engine: Mutex<Option<Arc<dyn InferenceEngine>>>,
    // Fields drop in declaration order: the mapped engine must release first.
    _lease: Option<crate::ModelLease>,
}

impl RegisteredModel {
    fn engine(
        &self,
        loader: &dyn EngineLoader,
        options: CoreOptions,
    ) -> Result<Arc<dyn InferenceEngine>, TranslationError> {
        let mut loaded = self
            .engine
            .lock()
            .map_err(|_| TranslationError::ModelLoad {
                model_id: self.descriptor.id.clone(),
                detail: "model initialization lock was poisoned".to_owned(),
            })?;
        if let Some(engine) = loaded.as_ref() {
            return Ok(Arc::clone(engine));
        }
        let descriptor = canonical_descriptor(&self.descriptor)?;
        verify_artifacts(&descriptor)?;
        let engine = backend_call("model loading", || loader.load(&descriptor, options)).map_err(
            |detail| TranslationError::ModelLoad {
                model_id: self.descriptor.id.clone(),
                detail,
            },
        )?;
        *loaded = Some(Arc::clone(&engine));
        Ok(engine)
    }
}

/// Models load synchronously; callers choose where blocking work runs.
/// Registration/removal require exclusive access. Translation can share the core.
pub struct TranslationCore {
    options: CoreOptions,
    detector: Arc<dyn LanguageDetector>,
    loader: Arc<dyn EngineLoader>,
    models: BTreeMap<LanguagePair, RegisteredModel>,
}

impl Default for TranslationCore {
    fn default() -> Self {
        Self::new(CoreOptions::default()).expect("default options are valid")
    }
}

impl TranslationCore {
    pub fn new(options: CoreOptions) -> Result<Self, TranslationError> {
        if !(1..=fxtranslate::segment::CONTEXT_WINDOW).contains(&options.max_source_tokens) {
            return Err(TranslationError::InvalidOptions(format!(
                "source token limit must be between one and {} including EOS",
                fxtranslate::segment::CONTEXT_WINDOW
            )));
        }
        if options.native_batch_size == 0 {
            return Err(TranslationError::InvalidOptions(
                "native batch limit must be positive".to_owned(),
            ));
        }
        if !(1..=4).contains(&options.threads) {
            return Err(TranslationError::InvalidOptions(
                "threads must be between one and four".to_owned(),
            ));
        }
        Ok(Self {
            options,
            detector: Arc::new(WhatlangDetector::default()),
            loader: Arc::new(FxLoader),
            models: BTreeMap::new(),
        })
    }

    pub fn detect(&self, text: &str) -> Option<Detection> {
        self.detector.detect(text)
    }

    /// Relative paths are made absolute at registration. Lazy registration does
    /// not open files; paths are canonicalized before verification and loading.
    /// Eager failure leaves the model unregistered.
    pub fn register_model(&mut self, descriptor: ModelDescriptor) -> Result<(), TranslationError> {
        self.register_with_lease(descriptor, None)
    }

    /// Retains the store's shared file-use guard until unregister or core drop.
    pub fn register_stored_model(
        &mut self,
        lease: crate::ModelLease,
        load_mode: LoadMode,
    ) -> Result<(), TranslationError> {
        let mut descriptor = lease.descriptor.clone();
        descriptor.load_mode = load_mode;
        self.register_with_lease(descriptor, Some(lease))
    }

    fn register_with_lease(
        &mut self,
        mut descriptor: ModelDescriptor,
        lease: Option<crate::ModelLease>,
    ) -> Result<(), TranslationError> {
        if descriptor.id.trim().is_empty() || descriptor.pair.source == descriptor.pair.target {
            return Err(TranslationError::InvalidModel(
                "model ID must be nonblank and language pair must be distinct".to_owned(),
            ));
        }
        if self
            .models
            .values()
            .any(|model| model.descriptor.id == descriptor.id)
        {
            return Err(TranslationError::DuplicateModelId(descriptor.id));
        }
        if self.models.contains_key(&descriptor.pair) {
            return Err(TranslationError::DuplicatePair(descriptor.pair));
        }
        for artifact in [
            &mut descriptor.model,
            &mut descriptor.source_tokenizer,
            &mut descriptor.target_tokenizer,
        ] {
            if artifact.path.as_os_str().is_empty() {
                return Err(TranslationError::InvalidModel(
                    "artifact path is empty".to_owned(),
                ));
            }
            artifact.path = std::path::absolute(&artifact.path).map_err(|error| {
                TranslationError::FileAccess {
                    path: artifact.path.clone(),
                    detail: error.to_string(),
                }
            })?;
        }
        let eager = descriptor.load_mode == LoadMode::Eager;
        let pair = descriptor.pair;
        let registered = RegisteredModel {
            descriptor,
            engine: Mutex::new(None),
            _lease: lease,
        };
        if eager {
            registered.engine(self.loader.as_ref(), self.options)?;
        }
        self.models.insert(pair, registered);
        Ok(())
    }

    /// Releases the registered engine without deleting caller-owned files.
    pub fn unregister_model(&mut self, id: &str) -> bool {
        let pair = self
            .models
            .iter()
            .find_map(|(pair, model)| (model.descriptor.id == id).then_some(*pair));
        pair.and_then(|pair| self.models.remove(&pair)).is_some()
    }

    pub fn translate(&self, request: &TranslationRequest) -> TranslationResult {
        self.translate_batch(std::slice::from_ref(request))
            .pop()
            .expect("one request produces one result")
    }

    pub fn translate_batch(&self, requests: &[TranslationRequest]) -> Vec<TranslationResult> {
        let mut results: Vec<Option<TranslationResult>> =
            (0..requests.len()).map(|_| None).collect();
        let mut groups: BTreeMap<LanguagePair, Vec<(usize, Option<Detection>)>> = BTreeMap::new();
        for (index, request) in requests.iter().enumerate() {
            match self.prepare(request) {
                Ok((pair, detection)) => groups.entry(pair).or_default().push((index, detection)),
                Err(reason) => results[index] = Some(Ok(TranslationOutcome::Skipped(reason))),
            }
        }
        for (pair, entries) in groups {
            let model = &self.models[&pair];
            let engine = match model.engine(self.loader.as_ref(), self.options) {
                Ok(engine) => engine,
                Err(error) => {
                    for (index, _) in entries {
                        results[index] = Some(Err(error.clone()));
                    }
                    continue;
                }
            };
            let mut eligible = Vec::new();
            for (index, detection) in entries {
                let actual = match backend_call("tokenization", || {
                    Ok(engine.source_token_count(&requests[index].text))
                }) {
                    Ok(actual) => actual,
                    Err(detail) => {
                        results[index] = Some(Err(TranslationError::Inference {
                            model_id: model.descriptor.id.clone(),
                            detail,
                        }));
                        continue;
                    }
                };
                if actual > self.options.max_source_tokens {
                    results[index] = Some(Err(TranslationError::InputTooLong {
                        actual,
                        limit: self.options.max_source_tokens,
                    }));
                } else {
                    eligible.push((index, detection));
                }
            }
            for chunk in eligible.chunks(self.options.native_batch_size) {
                let inputs: Vec<&str> = chunk
                    .iter()
                    .map(|(index, _)| requests[*index].text.as_str())
                    .collect();
                let translated = backend_call("inference", || engine.translate_batch(&inputs))
                    .and_then(|outputs| {
                        if outputs.len() != inputs.len() {
                            Err(format!(
                                "expected {} outputs, received {}",
                                inputs.len(),
                                outputs.len()
                            ))
                        } else {
                            Ok(outputs)
                        }
                    });
                match translated {
                    Ok(outputs) => {
                        for ((index, detection), text) in chunk.iter().zip(outputs) {
                            results[*index] =
                                Some(Ok(TranslationOutcome::Translated(Translation {
                                    text,
                                    source: pair.source,
                                    target: pair.target,
                                    model_id: model.descriptor.id.clone(),
                                    detection: *detection,
                                })));
                        }
                    }
                    Err(detail) => {
                        for (index, _) in chunk {
                            results[*index] = Some(Err(TranslationError::Inference {
                                model_id: model.descriptor.id.clone(),
                                detail: detail.clone(),
                            }));
                        }
                    }
                }
            }
        }
        results
            .into_iter()
            .map(|result| result.expect("every input has an outcome"))
            .collect()
    }

    fn prepare(
        &self,
        request: &TranslationRequest,
    ) -> Result<(LanguagePair, Option<Detection>), SkipReason> {
        if request.text.trim().is_empty() {
            return Err(SkipReason::BlankInput);
        }
        let (source, detection) = match request.source {
            SourceLanguage::Explicit(source) => (source, None),
            SourceLanguage::Auto => {
                let detection = self
                    .detect(&request.text)
                    .ok_or(SkipReason::LanguageUndetermined)?;
                if request.detection_policy == DetectionPolicy::ReliableOnly && !detection.reliable
                {
                    return Err(SkipReason::UnreliableDetection(detection));
                }
                (detection.language, Some(detection))
            }
        };
        if source == request.target {
            return Err(SkipReason::SameAsTarget(source));
        }
        let pair = LanguagePair {
            source,
            target: request.target,
        };
        if !self.models.contains_key(&pair) {
            return Err(SkipReason::ModelNotRegistered(pair));
        }
        Ok((pair, detection))
    }
}

#[cfg(test)]
mod tests;
