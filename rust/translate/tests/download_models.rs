#![cfg(all(feature = "model-downloads", feature = "catalog-http"))]

use synth_translate::{
    CatalogClient, InstallControl, InstallPlan, InstallStatus, Language, LanguagePair, LoadMode,
    ModelInstaller, ModelStore, SourceLanguage, TranslationCore, TranslationOutcome,
    TranslationRequest,
};

#[tokio::test]
#[ignore = "downloads a real model only with check-translate.mjs --download-model <direction>"]
async fn explicitly_requested_hosted_download_installs_and_translates() {
    let pair = match std::env::var("SYNTH_TRANSLATE_DOWNLOAD_PAIR").as_deref() {
        Ok("rus-eng") => LanguagePair { source: Language::RUSSIAN, target: Language::ENGLISH },
        Ok("eng-rus") => LanguagePair { source: Language::ENGLISH, target: Language::RUSSIAN },
        _ => panic!("use check-translate.mjs --download-model <rus-eng|eng-rus>; no model download was authorized"),
    };
    let directory = tempfile::tempdir().unwrap();
    let catalog = CatalogClient::open(directory.path())
        .unwrap()
        .initialize()
        .await
        .unwrap()
        .snapshot;
    let plan = InstallPlan::new(&catalog, pair).unwrap();
    println!(
        "Explicit live test: {} ({} download bytes; {} installed bytes)",
        plan.model().id(),
        plan.download_bytes(),
        plan.installed_bytes()
    );
    let store = ModelStore::open(directory.path()).unwrap();
    let installer = ModelInstaller::new(store.clone()).unwrap();
    let control = InstallControl::new();
    let result = installer.install(plan, control.clone()).await.unwrap();
    assert_eq!(result.status, InstallStatus::Installed);
    assert!(result.cleanup_warnings.is_empty());
    assert_eq!(store.verify(pair).unwrap().id(), result.model.id());
    let mut core = TranslationCore::default();
    core.register_stored_model(store.acquire(pair).unwrap(), LoadMode::Eager)
        .unwrap();
    let text = if pair.source == Language::RUSSIAN {
        "Сегодня хорошая погода, и мы идём гулять в парк."
    } else {
        "The weather is beautiful today, and we are going for a walk in the park."
    };
    let mut request = TranslationRequest::new(text, pair.target);
    request.source = SourceLanguage::Explicit(pair.source);
    assert!(matches!(
        core.translate(&request).unwrap(),
        TranslationOutcome::Translated(_)
    ));
    core.unregister_model(result.model.id());
    assert!(store.remove(pair).unwrap());
}
