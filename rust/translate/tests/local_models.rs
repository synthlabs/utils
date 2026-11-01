use std::{fs, path::PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};
use synth_translate::{
    CoreOptions, DetectionPolicy, LanguagePair, LoadMode, ModelArtifact, ModelDescriptor,
    SkipReason, SourceLanguage, Translation, TranslationCore, TranslationError, TranslationOutcome,
    TranslationRequest, TranslationResult,
};

#[derive(Deserialize)]
struct Fixtures {
    models: Vec<ModelFixture>,
}

#[derive(Deserialize)]
struct ArtifactFixture {
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
struct ModelFixture {
    id: String,
    source: String,
    target: String,
    model: ArtifactFixture,
    tokenizer: ArtifactFixture,
    texts: Vec<String>,
}

fn fixtures() -> Vec<ModelFixture> {
    serde_json::from_str::<Fixtures>(include_str!("fixtures/mozilla-v3.1.json"))
        .unwrap()
        .models
}

impl ModelFixture {
    fn descriptor(&self, load_mode: LoadMode) -> ModelDescriptor {
        let root =
            PathBuf::from(std::env::var_os("SYNTH_TRANSLATE_TEST_MODELS").expect(
                "use check-translate.mjs --models <directory>; models are never downloaded",
            ));
        let artifact = |fixture: &ArtifactFixture| ModelArtifact {
            path: root.join(&fixture.path),
            sha256: fixture.sha256.parse().unwrap(),
        };
        let tokenizer = artifact(&self.tokenizer);
        ModelDescriptor {
            id: self.id.clone(),
            pair: self.pair(),
            model: artifact(&self.model),
            source_tokenizer: tokenizer.clone(),
            target_tokenizer: tokenizer,
            load_mode,
        }
    }

    fn pair(&self) -> LanguagePair {
        LanguagePair {
            source: self.source.parse().unwrap(),
            target: self.target.parse().unwrap(),
        }
    }

    fn request(&self, text: &str, automatic: bool) -> TranslationRequest {
        let mut request = TranslationRequest::new(text, self.pair().target);
        if !automatic {
            request.source = SourceLanguage::Explicit(self.pair().source);
        }
        request
    }
}

fn translated(result: TranslationResult) -> Translation {
    match result.unwrap() {
        TranslationOutcome::Translated(translation) => translation,
        other => panic!("expected translation, got {other:?}"),
    }
}

#[test]
fn fixture_manifest_is_well_formed_without_local_models() {
    let fixtures = fixtures();
    assert_eq!(fixtures.len(), 2);
    assert_eq!(fixtures[0].pair().source, fixtures[1].pair().target);
    assert_eq!(fixtures[0].pair().target, fixtures[1].pair().source);
    for fixture in fixtures {
        for artifact in [&fixture.model, &fixture.tokenizer] {
            assert!(artifact
                .sha256
                .parse::<synth_translate::Sha256Digest>()
                .is_ok());
            assert!(!std::path::Path::new(&artifact.path).is_absolute());
            assert!(!artifact.path.contains(".."));
        }
    }
}

#[test]
#[ignore = "requires explicitly supplied Mozilla v3.1 model files"]
fn both_directions_match_across_loading_detection_and_batch_modes() {
    let fixtures = fixtures();
    let mut lazy = TranslationCore::default();
    let mut eager = TranslationCore::default();
    for fixture in &fixtures {
        lazy.register_model(fixture.descriptor(LoadMode::Lazy))
            .unwrap();
        eager
            .register_model(fixture.descriptor(LoadMode::Eager))
            .unwrap();
    }
    let mut requests = Vec::new();
    let mut expected = Vec::new();
    let mut reliable_translations = 0;
    let mut conservative_skips = 0;
    for index in 0..35 {
        let fixture = &fixtures[index % 2];
        let text = &fixture.texts[(index / 2) % fixture.texts.len()];
        let explicit = fixture.request(text, false);
        let baseline = translated(lazy.translate(&explicit));
        assert!(!baseline.text.trim().is_empty());
        assert_ne!(&baseline.text, text);
        assert_eq!(baseline.source, fixture.pair().source);
        assert_eq!(baseline.target, fixture.pair().target);
        assert_eq!(
            lazy.detect(&baseline.text).unwrap().language,
            fixture.pair().target
        );
        assert_eq!(translated(eager.translate(&explicit)), baseline);
        let mut automatic = fixture.request(text, true);
        let detection = lazy.detect(text).unwrap();
        assert_eq!(detection.language, fixture.pair().source);
        let conservative = lazy.translate(&automatic);
        if detection.reliable {
            assert_eq!(translated(conservative).text, baseline.text);
            reliable_translations += 1;
        } else {
            assert_eq!(
                conservative,
                Ok(TranslationOutcome::Skipped(
                    SkipReason::UnreliableDetection(detection)
                ))
            );
            conservative_skips += 1;
        }
        automatic.detection_policy = DetectionPolicy::BestGuess;
        let detected = translated(lazy.translate(&automatic));
        assert_eq!(detected.detection, Some(detection));
        assert_eq!(detected.text, baseline.text);
        requests.push(if index % 3 == 0 { automatic } else { explicit });
        expected.push(baseline.text);
    }
    assert!(
        reliable_translations > 0,
        "fixtures must exercise reliable automatic translation"
    );
    assert!(
        conservative_skips > 0,
        "fixtures must exercise conservative skips of valid text"
    );
    for core in [&lazy, &eager] {
        let results = core.translate_batch(&requests);
        assert_eq!(results.len(), requests.len());
        for (result, expected) in results.into_iter().zip(&expected) {
            assert_eq!(&translated(result).text, expected);
        }
    }
}

#[test]
#[ignore = "requires explicitly supplied Mozilla v3.1 model files"]
fn native_token_limits_include_eos_and_reject_without_truncation() {
    for fixture in fixtures() {
        let text = &fixture.texts[0];
        let request = fixture.request(text, false);
        let mut tiny = TranslationCore::new(CoreOptions {
            max_source_tokens: 1,
            ..CoreOptions::default()
        })
        .unwrap();
        tiny.register_model(fixture.descriptor(LoadMode::Lazy))
            .unwrap();
        let actual = match tiny.translate(&request) {
            Err(TranslationError::InputTooLong { actual, limit: 1 }) => actual,
            other => panic!("expected source token limit error, got {other:?}"),
        };
        assert!(actual > 1);
        for (limit, accepted) in [(actual, true), (actual - 1, false)] {
            let mut core = TranslationCore::new(CoreOptions {
                max_source_tokens: limit,
                ..CoreOptions::default()
            })
            .unwrap();
            core.register_model(fixture.descriptor(LoadMode::Eager))
                .unwrap();
            let result = core.translate(&request);
            if accepted {
                assert!(!translated(result).text.is_empty());
            } else {
                assert_eq!(
                    result,
                    Err(TranslationError::InputTooLong { actual, limit })
                );
            }
        }
    }
}

#[test]
#[ignore = "requires explicitly supplied Mozilla v3.1 model files"]
fn real_artifact_checks_reject_malformed_tokenizers_with_matching_hashes() {
    let directory = tempfile::tempdir().unwrap();
    let bad_vocab = directory.path().join("malformed.spm");
    let malformed = b"not a sentencepiece tokenizer";
    fs::write(&bad_vocab, malformed).unwrap();
    let checksum = format!("{:x}", Sha256::digest(malformed)).parse().unwrap();
    for fixture in fixtures() {
        for mode in [LoadMode::Lazy, LoadMode::Eager] {
            let mut descriptor = fixture.descriptor(mode);
            descriptor.source_tokenizer.path = bad_vocab.clone();
            descriptor.target_tokenizer.path = bad_vocab.clone();
            descriptor.source_tokenizer.sha256 = checksum;
            descriptor.target_tokenizer.sha256 = checksum;
            let mut core = TranslationCore::default();
            let result = core.register_model(descriptor);
            let failure = if mode == LoadMode::Eager {
                result
            } else {
                result.unwrap();
                core.translate(&fixture.request(&fixture.texts[0], false))
                    .map(|_| ())
            };
            match failure {
                Err(TranslationError::ModelLoad { detail, .. }) => {
                    assert!(detail.contains("tokenizer vocabulary is empty"));
                }
                other => panic!("expected malformed tokenizer rejection, got {other:?}"),
            }
        }
    }
}
