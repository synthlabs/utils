use std::fs;

use sha2::{Digest, Sha256};

use super::*;
use crate::{
    Language, LanguagePair, LoadMode, ModelArtifact, SourceLanguage, TranslationCore,
    TranslationRequest,
};

fn tokenizer(pieces: &[(&str, u8)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for &(piece, kind) in pieces {
        let mut message = vec![0x0a, u8::try_from(piece.len()).unwrap()];
        message.extend_from_slice(piece.as_bytes());
        message.push(0x15);
        message.extend_from_slice(&0.0f32.to_le_bytes());
        message.extend_from_slice(&[0x18, kind]);
        bytes.extend_from_slice(&[0x0a, u8::try_from(message.len()).unwrap()]);
        bytes.extend_from_slice(&message);
    }
    bytes
}

fn valid_tokenizer() -> Vec<u8> {
    tokenizer(&[("</s>", 3), ("<unk>", 2), ("▁a", 1)])
}

// One intgemm8 embedding tensor, deliberately missing output quantization metadata.
fn incomplete_model(dim: i32) -> Vec<u8> {
    let mut data = vec![0; 3 * dim as usize];
    data.extend_from_slice(&1.0f32.to_le_bytes());
    let mut bytes = Vec::new();
    for value in [1u64, 1, 4, 0x4101, 2, data.len() as u64] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(b"Wemb");
    bytes.extend_from_slice(&3i32.to_le_bytes());
    bytes.extend_from_slice(&dim.to_le_bytes());
    bytes.extend_from_slice(&0u64.to_le_bytes());
    bytes.extend_from_slice(&data);
    bytes
}

fn assert_load_failure(model: &[u8], source: &[u8], target: &[u8], expected: &str) {
    let directory = tempfile::tempdir().unwrap();
    let artifact = |name: &str, bytes: &[u8]| {
        let path = directory.path().join(name);
        fs::write(&path, bytes).unwrap();
        ModelArtifact {
            path,
            sha256: Sha256Digest::from_bytes(Sha256::digest(bytes).into()),
        }
    };
    let mut descriptor = ModelDescriptor {
        id: "synthetic".to_owned(),
        pair: LanguagePair {
            source: Language::RUSSIAN,
            target: Language::ENGLISH,
        },
        model: artifact("model.bin", model),
        source_tokenizer: artifact("source.spm", source),
        target_tokenizer: artifact("target.spm", target),
        load_mode: LoadMode::Lazy,
    };
    let mut request = TranslationRequest::new("a", Language::ENGLISH);
    request.source = SourceLanguage::Explicit(Language::RUSSIAN);
    for mode in [LoadMode::Lazy, LoadMode::Eager] {
        descriptor.load_mode = mode;
        let mut core = TranslationCore::default();
        if mode == LoadMode::Lazy {
            core.register_model(descriptor.clone()).unwrap();
        }
        for _ in 0..2 {
            let failure = if mode == LoadMode::Eager {
                core.register_model(descriptor.clone())
            } else {
                core.translate(&request).map(|_| ())
            };
            match failure {
                Err(TranslationError::ModelLoad { model_id, detail }) => {
                    assert_eq!(model_id, "synthetic");
                    assert!(
                        detail.contains(expected),
                        "expected {expected:?}, got {detail:?}"
                    );
                }
                other => panic!("expected model load rejection, got {other:?}"),
            }
        }
        assert_eq!(core.unregister_model("synthetic"), mode == LoadMode::Lazy);
    }
}

#[test]
fn tokenizer_guards_accept_a_usable_vocabulary() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("valid.spm");
    fs::write(&path, valid_tokenizer()).unwrap();
    let vocab = load_vocab(&path).unwrap();
    let ids = vocab.encode_with_eos("a");
    assert_eq!(ids, vec![2, 0]);
    assert_eq!(vocab.decode(&ids), "a");
}

#[test]
fn matching_hashes_do_not_allow_empty_or_unusable_source_and_target_vocabularies() {
    let valid = valid_tokenizer();
    let cases = [
        (Vec::new(), "tokenizer vocabulary is empty"),
        (
            b"not a sentencepiece tokenizer".to_vec(),
            "tokenizer vocabulary is empty",
        ),
        (tokenizer(&[("<unk>", 2), ("▁a", 1)]), "must contain EOS"),
        (tokenizer(&[("</s>", 3), ("▁a", 1)]), "must contain EOS"),
    ];
    for (malformed, expected) in cases {
        assert_load_failure(b"unused weights", &malformed, &valid, expected);
        assert_load_failure(b"unused weights", &valid, &malformed, expected);
    }
}

#[test]
fn matching_hashes_do_not_allow_overflowing_tokenizer_lengths_to_escape() {
    let mut malformed = vec![0x0a];
    malformed.extend_from_slice(&[0xff; 9]);
    malformed.push(0x01);
    let valid = valid_tokenizer();
    assert_load_failure(
        b"unused weights",
        &malformed,
        &valid,
        "backend panicked during model loading",
    );
    assert_load_failure(
        b"unused weights",
        &valid,
        &malformed,
        "backend panicked during model loading",
    );
}

#[test]
fn matching_hashes_do_not_allow_zero_embedding_dimensions_to_escape() {
    let valid = valid_tokenizer();
    assert_load_failure(&incomplete_model(0), &valid, &valid, "divide by zero");
}

#[test]
fn matching_hashes_do_not_allow_missing_output_quantization_to_escape() {
    let valid = valid_tokenizer();
    assert_load_failure(
        &incomplete_model(4),
        &valid,
        &valid,
        "model has no output-projection QuantMultA",
    );
}

#[test]
fn non_string_backend_panics_are_reported() {
    let result: Result<(), String> = backend_call("inference", || std::panic::panic_any(42u32));
    assert_eq!(
        result,
        Err("backend panicked during inference: non-string panic payload".to_owned())
    );
}
