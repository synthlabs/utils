use std::{
    fs,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Barrier,
    },
};

use sha2::{Digest, Sha256};
use tempfile::TempDir;

use super::*;
use crate::{Language, ModelArtifact, Sha256Digest};

const RU_EN: LanguagePair = LanguagePair {
    source: Language::RUSSIAN,
    target: Language::ENGLISH,
};
const EN_RU: LanguagePair = LanguagePair {
    source: Language::ENGLISH,
    target: Language::RUSSIAN,
};

#[derive(Default)]
struct Control {
    loads: AtomicUsize,
    drops: AtomicUsize,
    fail_next_load: AtomicBool,
    panic_next_load: AtomicBool,
    descriptors: Mutex<Vec<ModelDescriptor>>,
    batches: Mutex<Vec<(String, Vec<String>)>>,
}

struct MockLoader(Arc<Control>);
struct MockEngine(String, Arc<Control>);

impl EngineLoader for MockLoader {
    fn load(
        &self,
        model: &ModelDescriptor,
        _: CoreOptions,
    ) -> Result<Arc<dyn InferenceEngine>, String> {
        self.0.loads.fetch_add(1, Ordering::SeqCst);
        assert!(
            !self.0.panic_next_load.swap(false, Ordering::SeqCst),
            "temporary loader panic"
        );
        if self.0.fail_next_load.swap(false, Ordering::SeqCst) {
            return Err("temporary loader failure".to_owned());
        }
        self.0.descriptors.lock().unwrap().push(model.clone());
        Ok(Arc::new(MockEngine(model.id.clone(), self.0.clone())))
    }
}

impl Drop for MockEngine {
    fn drop(&mut self) {
        self.1.drops.fetch_add(1, Ordering::SeqCst);
    }
}

impl InferenceEngine for MockEngine {
    fn source_token_count(&self, text: &str) -> usize {
        assert_ne!(text, "tokenizer-panic", "tokenizer panic");
        text.split_whitespace().count() + 1
    }

    fn translate_batch(&self, texts: &[&str]) -> Result<Vec<String>, String> {
        self.1.batches.lock().unwrap().push((
            self.0.clone(),
            texts.iter().map(|text| (*text).to_owned()).collect(),
        ));
        assert!(!texts.contains(&"backend-panic"), "inference panic");
        if texts.contains(&"backend-error") {
            return Err("inference failed".to_owned());
        }
        if texts.contains(&"bad-count") {
            return Ok(Vec::new());
        }
        Ok(texts
            .iter()
            .map(|text| format!("{}:{text}", self.0))
            .collect())
    }
}

struct MockDetector;

impl LanguageDetector for MockDetector {
    fn detect(&self, text: &str) -> Option<Detection> {
        (text != "unknown").then_some(Detection {
            language: if text.starts_with("en:") {
                Language::ENGLISH
            } else {
                Language::RUSSIAN
            },
            confidence: if text == "uncertain" { 0.4 } else { 1.0 },
            reliable: text != "uncertain",
        })
    }
}

struct Fixture {
    _directory: TempDir,
    descriptor: ModelDescriptor,
}

fn hash(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest::from_bytes(Sha256::digest(bytes).into())
}

impl Fixture {
    fn new(id: &str, pair: LanguagePair) -> Self {
        Self::in_directory(tempfile::tempdir().unwrap(), id, pair)
    }

    fn in_directory(directory: TempDir, id: &str, pair: LanguagePair) -> Self {
        let model_path = directory.path().join("model.bin");
        let vocab_path = directory.path().join("vocab.spm");
        fs::write(&model_path, b"weights").unwrap();
        fs::write(&vocab_path, b"vocabulary").unwrap();
        let tokenizer = ModelArtifact {
            path: vocab_path,
            sha256: hash(b"vocabulary"),
        };
        Self {
            descriptor: ModelDescriptor {
                id: id.to_owned(),
                pair,
                model: ModelArtifact {
                    path: model_path,
                    sha256: hash(b"weights"),
                },
                source_tokenizer: tokenizer.clone(),
                target_tokenizer: tokenizer,
                load_mode: LoadMode::default(),
            },
            _directory: directory,
        }
    }
}

fn mock_core(options: CoreOptions) -> (TranslationCore, Arc<Control>) {
    let mut core = TranslationCore::new(options).unwrap();
    let control = Arc::new(Control::default());
    core.loader = Arc::new(MockLoader(control.clone()));
    core.detector = Arc::new(MockDetector);
    (core, control)
}

fn request(text: &str, pair: LanguagePair) -> TranslationRequest {
    TranslationRequest {
        text: text.to_owned(),
        source: SourceLanguage::Explicit(pair.source),
        target: pair.target,
        detection_policy: DetectionPolicy::default(),
    }
}

fn translated(result: TranslationResult) -> Translation {
    match result.unwrap() {
        TranslationOutcome::Translated(translation) => translation,
        other => panic!("expected translation, got {other:?}"),
    }
}

#[test]
fn language_and_checksum_types_validate_and_normalize() {
    assert_eq!(Language::new("RUS").unwrap(), Language::RUSSIAN);
    for code in ["ru", "english", "123", "éé", "eng "] {
        assert!(matches!(
            Language::new(code),
            Err(TranslationError::InvalidLanguage(_))
        ));
    }
    let hex = "AB".repeat(32);
    assert_eq!(
        Sha256Digest::from_hex(&hex).unwrap().to_string(),
        hex.to_lowercase()
    );
    for invalid in ["", "123", &"gg".repeat(32), &"é".repeat(32)] {
        assert!(Sha256Digest::from_hex(invalid).is_err());
    }
}

#[test]
fn detector_identifies_full_sentences_beyond_the_initial_translation_pair() {
    let core = TranslationCore::default();
    for (text, expected) in [
        (
            "This is an English sentence about the weather and the beautiful mountains.",
            "eng",
        ),
        (
            "Сегодня прекрасная погода, и мы собираемся гулять в большом городском парке.",
            "rus",
        ),
        (
            "Це українська мова, якою розмовляють багато людей у нашій незалежній країні.",
            "ukr",
        ),
        (
            "Cette phrase est écrite en français et décrit une belle journée ensoleillée.",
            "fra",
        ),
    ] {
        let detection = core.detect(text).unwrap();
        assert_eq!(detection.language.as_str(), expected, "{text}");
        assert!(detection.confidence.is_finite());
        assert!((0.0..=1.0).contains(&detection.confidence));
    }
    for text in ["", " \n\t", "123456", "😀🎉"] {
        assert_eq!(core.detect(text), None);
    }
}

#[test]
fn invalid_options_are_rejected() {
    assert_eq!(CoreOptions::default().max_source_tokens, 128);
    for max_source_tokens in [1, 128] {
        assert!(TranslationCore::new(CoreOptions {
            max_source_tokens,
            ..CoreOptions::default()
        })
        .is_ok());
    }
    for options in [
        CoreOptions {
            max_source_tokens: 0,
            ..CoreOptions::default()
        },
        CoreOptions {
            max_source_tokens: 129,
            ..CoreOptions::default()
        },
        CoreOptions {
            native_batch_size: 0,
            ..CoreOptions::default()
        },
        CoreOptions {
            threads: 0,
            ..CoreOptions::default()
        },
        CoreOptions {
            threads: 5,
            ..CoreOptions::default()
        },
    ] {
        assert!(matches!(
            TranslationCore::new(options),
            Err(TranslationError::InvalidOptions(_))
        ));
    }
}

#[test]
fn lazy_registration_does_not_open_files_and_first_use_retries_after_missing_file() {
    let fixture = Fixture::new("ru-en", RU_EN);
    fs::remove_file(&fixture.descriptor.model.path).unwrap();
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    assert_eq!(control.loads.load(Ordering::SeqCst), 0);
    assert!(matches!(
        core.translate(&request("hello", RU_EN)),
        Err(TranslationError::FileAccess { .. })
    ));
    assert_eq!(control.loads.load(Ordering::SeqCst), 0);
    fs::write(&fixture.descriptor.model.path, b"weights").unwrap();
    translated(core.translate(&request("hello", RU_EN)));
    translated(core.translate(&request("again", RU_EN)));
    assert_eq!(control.loads.load(Ordering::SeqCst), 1);
}

#[test]
fn eager_loads_immediately_and_failed_eager_registration_is_atomic() {
    let mut fixture = Fixture::new("ru-en", RU_EN);
    fixture.descriptor.load_mode = LoadMode::Eager;
    let (mut core, control) = mock_core(CoreOptions::default());
    control.fail_next_load.store(true, Ordering::SeqCst);
    assert!(matches!(
        core.register_model(fixture.descriptor.clone()),
        Err(TranslationError::ModelLoad { .. })
    ));
    assert!(core.models.is_empty());
    core.register_model(fixture.descriptor.clone()).unwrap();
    assert_eq!(control.loads.load(Ordering::SeqCst), 2);
    translated(core.translate(&request("hello", RU_EN)));
    assert_eq!(control.loads.load(Ordering::SeqCst), 2);
}

#[test]
fn failed_lazy_load_is_retryable() {
    let fixture = Fixture::new("ru-en", RU_EN);
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    control.fail_next_load.store(true, Ordering::SeqCst);
    assert!(matches!(
        core.translate(&request("hello", RU_EN)),
        Err(TranslationError::ModelLoad { .. })
    ));
    translated(core.translate(&request("hello", RU_EN)));
    assert_eq!(control.loads.load(Ordering::SeqCst), 2);
}

#[test]
fn loader_panics_are_typed_and_leave_eager_and_lazy_initialization_retryable() {
    for mode in [LoadMode::Lazy, LoadMode::Eager] {
        let mut fixture = Fixture::new("ru-en", RU_EN);
        fixture.descriptor.load_mode = mode;
        let (mut core, control) = mock_core(CoreOptions::default());
        control.panic_next_load.store(true, Ordering::SeqCst);
        let registration = core.register_model(fixture.descriptor.clone());
        let failure = if mode == LoadMode::Eager {
            assert!(core.models.is_empty());
            registration
        } else {
            registration.unwrap();
            core.translate(&request("hello", RU_EN)).map(|_| ())
        };
        match failure {
            Err(TranslationError::ModelLoad { model_id, detail }) => {
                assert_eq!(model_id, "ru-en");
                assert!(detail.contains("backend panicked during model loading"));
                assert!(detail.contains("temporary loader panic"));
            }
            other => panic!("expected contained loader panic, got {other:?}"),
        }
        if mode == LoadMode::Eager {
            core.register_model(fixture.descriptor.clone()).unwrap();
        }
        translated(core.translate(&request("hello", RU_EN)));
        assert_eq!(control.loads.load(Ordering::SeqCst), 2);
    }
}

#[test]
fn concurrent_initialization_recovers_after_one_loader_panics() {
    let fixture = Fixture::new("ru-en", RU_EN);
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    control.panic_next_load.store(true, Ordering::SeqCst);
    let barrier = Barrier::new(8);
    let failures = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                barrier.wait();
                match core.translate(&request("hello", RU_EN)) {
                    Err(TranslationError::ModelLoad { detail, .. }) => {
                        assert!(detail.contains("temporary loader panic"));
                        failures.fetch_add(1, Ordering::SeqCst);
                    }
                    result => {
                        translated(result);
                    }
                }
            });
        }
    });
    assert_eq!(failures.load(Ordering::SeqCst), 1);
    assert_eq!(control.loads.load(Ordering::SeqCst), 2);
}

#[test]
fn filesystem_aliases_are_canonicalized_before_loading() {
    let mut fixture = Fixture::new("ru-en", RU_EN);
    let parent = fixture
        .descriptor
        .model
        .path
        .parent()
        .unwrap()
        .to_path_buf();
    fs::create_dir(parent.join("alias")).unwrap();
    fixture.descriptor.model.path = parent.join("alias/../model.bin");
    fixture.descriptor.source_tokenizer.path = parent.join("alias/../vocab.spm");
    let expected = canonical_descriptor(&fixture.descriptor).unwrap();
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    assert!(control.descriptors.lock().unwrap().is_empty());
    translated(core.translate(&request("hello", RU_EN)));
    let descriptors = control.descriptors.lock().unwrap();
    assert_eq!(descriptors[0].model.path, expected.model.path);
    assert_eq!(
        descriptors[0].source_tokenizer.path,
        expected.target_tokenizer.path
    );
    assert_eq!(
        descriptors[0].target_tokenizer.path,
        expected.target_tokenizer.path
    );
}

#[test]
fn relative_paths_are_anchored_at_registration_and_canonicalized_on_load() {
    let cwd = std::env::current_dir().unwrap().canonicalize().unwrap();
    let mut fixture = Fixture::in_directory(tempfile::tempdir_in(&cwd).unwrap(), "ru-en", RU_EN);
    let expected = canonical_descriptor(&fixture.descriptor).unwrap();
    for artifact in [
        &mut fixture.descriptor.model,
        &mut fixture.descriptor.source_tokenizer,
        &mut fixture.descriptor.target_tokenizer,
    ] {
        artifact.path = artifact.path.strip_prefix(&cwd).unwrap().to_path_buf();
        assert!(artifact.path.is_relative());
    }
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    let registered = &core.models[&RU_EN].descriptor;
    assert_eq!(
        registered.model.path,
        std::path::absolute(&fixture.descriptor.model.path).unwrap()
    );
    assert_eq!(
        registered.source_tokenizer.path,
        std::path::absolute(&fixture.descriptor.source_tokenizer.path).unwrap()
    );
    translated(core.translate(&request("hello", RU_EN)));
    let loaded = control.descriptors.lock().unwrap();
    assert_eq!(loaded[0].model.path, expected.model.path);
    assert_eq!(
        loaded[0].source_tokenizer.path,
        expected.source_tokenizer.path
    );
}

#[test]
fn conflicting_hashes_through_a_path_alias_are_still_rejected() {
    let mut fixture = Fixture::new("ru-en", RU_EN);
    let parent = fixture.descriptor.target_tokenizer.path.parent().unwrap();
    fs::create_dir(parent.join("alias")).unwrap();
    fixture.descriptor.target_tokenizer.path = parent.join("alias/../vocab.spm");
    fixture.descriptor.target_tokenizer.sha256 = hash(b"different");
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    assert!(matches!(
        core.translate(&request("hello", RU_EN)),
        Err(TranslationError::ChecksumMismatch { .. })
    ));
    assert_eq!(control.loads.load(Ordering::SeqCst), 0);
}

#[test]
fn symlinked_tokenizers_are_canonicalized_and_hash_conflicts_are_preserved() {
    let mut fixture = Fixture::new("ru-en", RU_EN);
    let alias = fixture._directory.path().join("alias.spm");
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&fixture.descriptor.source_tokenizer.path, &alias);
    #[cfg(windows)]
    let linked =
        std::os::windows::fs::symlink_file(&fixture.descriptor.source_tokenizer.path, &alias);
    if let Err(error) = linked {
        if error.kind() == std::io::ErrorKind::PermissionDenied {
            eprintln!("symlink test unavailable: {error}");
            return;
        }
        panic!("cannot create test symlink: {error}");
    }
    fixture.descriptor.target_tokenizer.path = alias;
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    translated(core.translate(&request("hello", RU_EN)));
    let loaded = control.descriptors.lock().unwrap();
    assert_eq!(
        loaded[0].source_tokenizer.path,
        loaded[0].target_tokenizer.path
    );
    drop(loaded);
    core.unregister_model("ru-en");
    fixture.descriptor.target_tokenizer.sha256 = hash(b"different");
    core.register_model(fixture.descriptor.clone()).unwrap();
    assert!(matches!(
        core.translate(&request("hello", RU_EN)),
        Err(TranslationError::ChecksumMismatch { .. })
    ));
    assert_eq!(control.loads.load(Ordering::SeqCst), 1);
}

#[test]
fn every_artifact_is_verified_before_the_loader_runs() {
    for role in ["model", "source", "target"] {
        let mut fixture = Fixture::new("ru-en", RU_EN);
        if role == "target" {
            fixture.descriptor.target_tokenizer.path = fixture._directory.path().join("target.spm");
            fs::write(&fixture.descriptor.target_tokenizer.path, b"vocabulary").unwrap();
        }
        let artifact = match role {
            "model" => &fixture.descriptor.model,
            "source" => &fixture.descriptor.source_tokenizer,
            _ => &fixture.descriptor.target_tokenizer,
        };
        fs::write(&artifact.path, b"corrupt").unwrap();
        let (mut core, control) = mock_core(CoreOptions::default());
        core.register_model(fixture.descriptor.clone()).unwrap();
        assert!(
            matches!(
                core.translate(&request("hello", RU_EN)),
                Err(TranslationError::ChecksumMismatch { .. })
            ),
            "{role}"
        );
        assert_eq!(control.loads.load(Ordering::SeqCst), 0, "{role}");
    }
}

#[test]
fn conflicting_hashes_for_a_shared_tokenizer_are_not_deduplicated_away() {
    let mut fixture = Fixture::new("ru-en", RU_EN);
    fixture.descriptor.target_tokenizer.sha256 = hash(b"different");
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    assert!(matches!(
        core.translate(&request("hello", RU_EN)),
        Err(TranslationError::ChecksumMismatch { .. })
    ));
    assert_eq!(control.loads.load(Ordering::SeqCst), 0);
}

#[test]
fn duplicate_registration_requires_explicit_removal_and_preserves_original() {
    let fixture = Fixture::new("ru-en", RU_EN);
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    let mut duplicate = fixture.descriptor.clone();
    duplicate.pair = EN_RU;
    assert!(matches!(
        core.register_model(duplicate),
        Err(TranslationError::DuplicateModelId(_))
    ));
    let mut duplicate = fixture.descriptor.clone();
    duplicate.id = "replacement".to_owned();
    assert_eq!(
        core.register_model(duplicate.clone()),
        Err(TranslationError::DuplicatePair(RU_EN))
    );
    assert_eq!(
        translated(core.translate(&request("hello", RU_EN))).model_id,
        "ru-en"
    );
    assert!(!core.unregister_model("absent"));
    assert!(core.unregister_model("ru-en"));
    assert_eq!(control.drops.load(Ordering::SeqCst), 1);
    assert!(fixture.descriptor.model.path.exists());
    core.register_model(duplicate).unwrap();
    assert_eq!(
        translated(core.translate(&request("hello", RU_EN))).model_id,
        "replacement"
    );
}

#[test]
fn invalid_descriptors_do_not_reserve_a_pair() {
    let fixture = Fixture::new("ru-en", RU_EN);
    for kind in ["id", "pair", "path"] {
        let mut descriptor = fixture.descriptor.clone();
        match kind {
            "id" => descriptor.id = "  ".to_owned(),
            "pair" => descriptor.pair.target = descriptor.pair.source,
            _ => descriptor.model.path.clear(),
        }
        let (mut core, _) = mock_core(CoreOptions::default());
        assert!(matches!(
            core.register_model(descriptor),
            Err(TranslationError::InvalidModel(_))
        ));
        core.register_model(fixture.descriptor.clone()).unwrap();
    }
}

#[test]
fn concurrent_first_use_initializes_once() {
    let fixture = Fixture::new("ru-en", RU_EN);
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    let barrier = Barrier::new(8);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            scope.spawn(|| {
                barrier.wait();
                translated(core.translate(&request("hello", RU_EN)));
            });
        }
    });
    assert_eq!(control.loads.load(Ordering::SeqCst), 1);
}

#[test]
fn detection_policy_is_per_request_and_best_guess_preserves_uncertainty() {
    let fixture = Fixture::new("ru-en", RU_EN);
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    let mut request = TranslationRequest::new("uncertain", Language::ENGLISH);
    assert!(matches!(
        core.translate(&request),
        Ok(TranslationOutcome::Skipped(
            SkipReason::UnreliableDetection(_)
        ))
    ));
    assert_eq!(control.loads.load(Ordering::SeqCst), 0);
    request.detection_policy = DetectionPolicy::BestGuess;
    let result = translated(core.translate(&request));
    assert!(!result.detection.unwrap().reliable);
    request.detection_policy = DetectionPolicy::ReliableOnly;
    assert!(matches!(
        core.translate(&request),
        Ok(TranslationOutcome::Skipped(
            SkipReason::UnreliableDetection(_)
        ))
    ));
    request.text = "unknown".to_owned();
    request.detection_policy = DetectionPolicy::BestGuess;
    assert_eq!(
        core.translate(&request),
        Ok(TranslationOutcome::Skipped(
            SkipReason::LanguageUndetermined
        ))
    );
}

#[test]
fn explicit_sources_bypass_detection_and_text_is_not_cleaned() {
    struct NoDetection;
    impl LanguageDetector for NoDetection {
        fn detect(&self, _: &str) -> Option<Detection> {
            panic!("explicit sources must not detect")
        }
    }
    let fixture = Fixture::new("ru-en", RU_EN);
    let (mut core, _) = mock_core(CoreOptions::default());
    core.detector = Arc::new(NoDetection);
    core.register_model(fixture.descriptor.clone()).unwrap();
    let input = "  @name https://example.com 😀\n:emote:  ";
    let result = translated(core.translate(&request(input, RU_EN)));
    assert_eq!(result.text, format!("ru-en:{input}"));
    assert_eq!(result.detection, None);
}

#[test]
fn expected_skips_never_load_a_model() {
    let fixture = Fixture::new("ru-en", RU_EN);
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    assert_eq!(
        core.translate(&request(" \n\t", RU_EN)),
        Ok(TranslationOutcome::Skipped(SkipReason::BlankInput))
    );
    let same = LanguagePair {
        source: Language::ENGLISH,
        target: Language::ENGLISH,
    };
    assert_eq!(
        core.translate(&request("text", same)),
        Ok(TranslationOutcome::Skipped(SkipReason::SameAsTarget(
            Language::ENGLISH
        )))
    );
    assert_eq!(
        core.translate(&request("text", EN_RU)),
        Ok(TranslationOutcome::Skipped(SkipReason::ModelNotRegistered(
            EN_RU
        )))
    );
    assert_eq!(control.loads.load(Ordering::SeqCst), 0);
    assert!(core.translate_batch(&[]).is_empty());
}

#[test]
fn mixed_batches_route_and_preserve_order_and_item_errors() {
    let ru_en = Fixture::new("ru-en", RU_EN);
    let en_ru = Fixture::new("en-ru", EN_RU);
    let (mut core, _) = mock_core(CoreOptions {
        max_source_tokens: 4,
        ..CoreOptions::default()
    });
    core.register_model(ru_en.descriptor.clone()).unwrap();
    core.register_model(en_ru.descriptor.clone()).unwrap();
    let requests = [
        request("first", RU_EN),
        request("second", EN_RU),
        request("", RU_EN),
        request("one two three four", RU_EN),
        request("one two three", RU_EN),
        TranslationRequest::new("en: automatic", Language::RUSSIAN),
    ];
    let mut results = core.translate_batch(&requests).into_iter();
    assert_eq!(translated(results.next().unwrap()).text, "ru-en:first");
    assert_eq!(translated(results.next().unwrap()).text, "en-ru:second");
    assert_eq!(
        results.next().unwrap(),
        Ok(TranslationOutcome::Skipped(SkipReason::BlankInput))
    );
    assert_eq!(
        results.next().unwrap(),
        Err(TranslationError::InputTooLong {
            actual: 5,
            limit: 4
        })
    );
    assert_eq!(
        translated(results.next().unwrap()).text,
        "ru-en:one two three"
    );
    assert_eq!(
        translated(results.next().unwrap())
            .detection
            .unwrap()
            .language,
        Language::ENGLISH
    );
}

#[test]
fn default_window_counts_eos_and_rejects_before_inference() {
    let fixture = Fixture::new("ru-en", RU_EN);
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(fixture.descriptor.clone()).unwrap();
    let requests: Vec<_> = [126, 127, 128]
        .into_iter()
        .map(|count| request(&vec!["word"; count].join(" "), RU_EN))
        .collect();
    let mut results = core.translate_batch(&requests).into_iter();
    translated(results.next().unwrap());
    translated(results.next().unwrap());
    assert_eq!(
        results.next().unwrap(),
        Err(TranslationError::InputTooLong {
            actual: 129,
            limit: 128,
        })
    );
    let batches = control.batches.lock().unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(
        batches[0].1,
        vec![requests[0].text.clone(), requests[1].text.clone()]
    );
}

#[test]
fn tokenization_panics_affect_only_the_item_and_preserve_other_pairs() {
    let ru_en = Fixture::new("ru-en", RU_EN);
    let en_ru = Fixture::new("en-ru", EN_RU);
    let (mut core, control) = mock_core(CoreOptions::default());
    core.register_model(ru_en.descriptor.clone()).unwrap();
    core.register_model(en_ru.descriptor.clone()).unwrap();
    let mut results = core
        .translate_batch(&[
            request("tokenizer-panic", RU_EN),
            request("good", RU_EN),
            request("other", EN_RU),
        ])
        .into_iter();
    match results.next().unwrap() {
        Err(TranslationError::Inference { detail, .. }) => assert!(detail.contains("tokenization")),
        other => panic!("expected tokenization failure, got {other:?}"),
    }
    assert_eq!(translated(results.next().unwrap()).text, "ru-en:good");
    assert_eq!(translated(results.next().unwrap()).text, "en-ru:other");
    assert!(control
        .batches
        .lock()
        .unwrap()
        .iter()
        .all(|(_, texts)| !texts.contains(&"tokenizer-panic".to_owned())));
}

#[test]
fn inference_panics_affect_the_chunk_and_preserve_later_chunks_and_pairs() {
    let ru_en = Fixture::new("ru-en", RU_EN);
    let en_ru = Fixture::new("en-ru", EN_RU);
    let (mut core, _) = mock_core(CoreOptions {
        native_batch_size: 2,
        ..CoreOptions::default()
    });
    core.register_model(ru_en.descriptor.clone()).unwrap();
    core.register_model(en_ru.descriptor.clone()).unwrap();
    let mut results = core
        .translate_batch(&[
            request("backend-panic", RU_EN),
            request("same chunk", RU_EN),
            request("next chunk", RU_EN),
            request("other", EN_RU),
        ])
        .into_iter();
    for _ in 0..2 {
        match results.next().unwrap() {
            Err(TranslationError::Inference { detail, .. }) => {
                assert!(detail.contains("inference panic"))
            }
            other => panic!("expected inference failure, got {other:?}"),
        }
    }
    assert_eq!(translated(results.next().unwrap()).text, "ru-en:next chunk");
    assert_eq!(translated(results.next().unwrap()).text, "en-ru:other");
    translated(core.translate(&request("retry", RU_EN)));
}

#[test]
fn large_batches_are_chunked_without_losing_items_or_order() {
    for chunk_size in [16, 3] {
        let fixture = Fixture::new("ru-en", RU_EN);
        let (mut core, control) = mock_core(CoreOptions {
            native_batch_size: chunk_size,
            ..CoreOptions::default()
        });
        core.register_model(fixture.descriptor.clone()).unwrap();
        let requests: Vec<_> = (0..35)
            .map(|i| request(&format!("item-{i}"), RU_EN))
            .collect();
        let results = core.translate_batch(&requests);
        assert_eq!(results.len(), 35);
        for (i, result) in results.into_iter().enumerate() {
            assert_eq!(translated(result).text, format!("ru-en:item-{i}"));
        }
        let batches = control.batches.lock().unwrap();
        assert!(batches.iter().all(|(_, batch)| batch.len() <= chunk_size));
        assert_eq!(
            batches.iter().map(|(_, batch)| batch.len()).sum::<usize>(),
            35
        );
    }
}

#[test]
fn one_model_load_failure_does_not_discard_another_pairs_results() {
    let ru_en = Fixture::new("ru-en", RU_EN);
    let en_ru = Fixture::new("en-ru", EN_RU);
    fs::write(&en_ru.descriptor.model.path, b"corrupt").unwrap();
    let (mut core, _) = mock_core(CoreOptions::default());
    core.register_model(ru_en.descriptor.clone()).unwrap();
    core.register_model(en_ru.descriptor.clone()).unwrap();
    let mut results = core
        .translate_batch(&[
            request("a", RU_EN),
            request("b", EN_RU),
            request("c", RU_EN),
        ])
        .into_iter();
    assert_eq!(translated(results.next().unwrap()).text, "ru-en:a");
    assert!(matches!(
        results.next().unwrap(),
        Err(TranslationError::ChecksumMismatch { .. })
    ));
    assert_eq!(translated(results.next().unwrap()).text, "ru-en:c");
}

#[test]
fn backend_errors_and_output_count_mismatches_are_typed_and_do_not_stop_later_chunks() {
    for bad_input in ["backend-error", "bad-count"] {
        let fixture = Fixture::new("ru-en", RU_EN);
        let (mut core, _) = mock_core(CoreOptions {
            native_batch_size: 1,
            ..CoreOptions::default()
        });
        core.register_model(fixture.descriptor.clone()).unwrap();
        let mut results = core
            .translate_batch(&[request(bad_input, RU_EN), request("good", RU_EN)])
            .into_iter();
        assert!(matches!(
            results.next().unwrap(),
            Err(TranslationError::Inference { .. })
        ));
        assert_eq!(translated(results.next().unwrap()).text, "ru-en:good");
    }
}
