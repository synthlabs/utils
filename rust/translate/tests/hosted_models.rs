use std::{fs, path::PathBuf};

use sha2::{Digest, Sha256};
use synth_translate::{
    CatalogSnapshot, InstallationManifest, Language, LoadMode, ModelStore, SourceLanguage,
    StoreError, TranslationCore, TranslationOutcome, TranslationRequest, INSTALL_MANIFEST_FILENAME,
};

fn catalog() -> CatalogSnapshot {
    let bytes = include_bytes!("fixtures/hosted-catalog-v1.json");
    let digest = format!("{:x}", Sha256::digest(bytes));
    CatalogSnapshot::from_bytes(bytes, &digest).unwrap()
}

#[test]
fn captured_hosted_metadata_preserves_selected_revisions_and_sizes() {
    let catalog = catalog();
    assert_eq!(catalog.models().count(), 2);
    for model in catalog.models() {
        assert_eq!(model.required_files().len(), 2);
        if model.pair().source == Language::ENGLISH {
            assert_eq!(model.version(), "3.0");
            assert_eq!(model.architecture(), "base");
            assert_eq!(model.model().decompressed_size, 42_992_955);
            assert_eq!(
                model.model().decompressed_sha256.to_string(),
                "0ef9a209c5edc46692750e7505b3695655b1c7c3ec73058b641201ef18c481ce"
            );
        } else {
            assert_eq!(model.version(), "3.1");
            assert_eq!(model.architecture(), "base-memory");
            assert_eq!(model.model().decompressed_size, 31_561_787);
            assert_eq!(
                model.model().decompressed_sha256.to_string(),
                "aa0efa194b3dbd2da129ba0b8db8ef139126cb0914e57072f62061294746f1e2"
            );
        }
    }
}

#[test]
#[ignore = "requires explicitly supplied decompressed hosted models; never downloads"]
fn hosted_revisions_load_translate_and_release_store_guards() {
    let supplied =
        PathBuf::from(std::env::var_os("SYNTH_TRANSLATE_HOSTED_MODELS").expect(
            "use check-translate.mjs --hosted-models <directory>; files are never downloaded",
        ));
    let directory = tempfile::tempdir().unwrap();
    let store = ModelStore::open(directory.path()).unwrap();
    let catalog = catalog();
    store.retain_catalog(&catalog).unwrap();
    for model in catalog.models() {
        let guard = store.exclusive(model.pair()).unwrap();
        fs::create_dir(guard.directory()).unwrap();
        let source = supplied.join(if model.pair().source == Language::RUSSIAN {
            "firefox-ru-en"
        } else {
            "firefox-en-ru"
        });
        for file in model.required_files() {
            fs::copy(
                source.join(&file.filename),
                guard.directory().join(&file.filename),
            )
            .unwrap();
        }
        fs::write(
            guard.directory().join(INSTALL_MANIFEST_FILENAME),
            serde_json::to_vec(&InstallationManifest::for_model(model)).unwrap(),
        )
        .unwrap();
        drop(guard);
        store.verify(model.pair()).unwrap();
    }
    for mode in [LoadMode::Lazy, LoadMode::Eager] {
        let mut core = TranslationCore::default();
        for model in catalog.models() {
            core.register_stored_model(store.acquire(model.pair()).unwrap(), mode)
                .unwrap();
            assert!(matches!(
                store.remove(model.pair()),
                Err(StoreError::Busy(_))
            ));
        }
        let requests: Vec<_> = catalog
            .models()
            .map(|model| {
                let text = if model.pair().source == Language::RUSSIAN {
                    "Сегодня хорошая погода, и мы идём гулять в парк."
                } else {
                    "The weather is beautiful today, and we are going for a walk in the park."
                };
                let mut request = TranslationRequest::new(text, model.pair().target);
                request.source = SourceLanguage::Explicit(model.pair().source);
                request
            })
            .collect();
        for request in &requests {
            let TranslationOutcome::Translated(result) = core.translate(request).unwrap() else {
                panic!("expected translation")
            };
            assert!(!result.text.trim().is_empty());
            assert_eq!(result.target, request.target);
            assert_eq!(
                core.translate(request).unwrap(),
                TranslationOutcome::Translated(result.clone())
            );
            let mut automatic = request.clone();
            automatic.source = SourceLanguage::Auto;
            let TranslationOutcome::Translated(detected) = core.translate(&automatic).unwrap()
            else {
                panic!("expected reliable automatic translation")
            };
            assert_eq!(detected.text, result.text);
        }
        let batch: Vec<_> = requests.iter().cycle().take(20).cloned().collect();
        for result in core.translate_batch(&batch) {
            assert!(matches!(result.unwrap(), TranslationOutcome::Translated(_)));
        }
        for model in catalog.models() {
            assert!(core.unregister_model(model.id()));
            assert!(store.exclusive(model.pair()).is_ok());
        }
    }
    for model in catalog.models() {
        assert!(store.remove(model.pair()).unwrap());
    }
}
