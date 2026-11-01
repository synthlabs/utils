use std::{future::Future, sync::Condvar, time::Duration};

use super::*;
use crate::{
    catalog::tests::{snapshot, wire, RU_EN},
    detect::WhatlangDetector,
    engine::{EngineLoader, InferenceEngine},
    installer::tests::{compressed_catalog, Reply, Server},
};

async fn bounded<T>(future: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), future)
        .await
        .expect("operation must complete")
}

async fn changed(
    service: &TranslationService,
    predicate: impl Fn(&ServiceSnapshot) -> bool,
) -> ServiceSnapshot {
    bounded(async {
        let mut changes = service.subscribe();
        loop {
            let value = changes.borrow_and_update().clone();
            if predicate(&value) {
                return value;
            }
            changes.changed().await.unwrap();
        }
    })
    .await
}

fn config() -> PluginConfig {
    PluginConfig {
        test_origin: Some("http://127.0.0.1:0".into()),
        ..PluginConfig::default()
    }
}

async fn service(root: &std::path::Path) -> Arc<TranslationService> {
    let service = TranslationService::new(root.into(), config());
    service.initialize().await.unwrap();
    service
}

fn request(text: &str) -> TranslationRequest {
    let mut request = TranslationRequest::new(text, Language::ENGLISH);
    request.source = SourceLanguage::Explicit(Language::RUSSIAN);
    request
}

#[tokio::test]
async fn initialization_failure_is_visible_and_retryable() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("store");
    std::fs::write(&root, b"obstruction").unwrap();
    let service = TranslationService::new(root.clone(), config());
    assert!(service.initialize().await.is_err());
    assert_eq!(service.snapshot().phase, ServicePhase::Unavailable);
    assert!(service.snapshot().error.is_some());
    assert_eq!(
        service.submit(request("hello")).await.err().unwrap().code,
        ErrorCode::Unavailable
    );
    std::fs::remove_file(root).unwrap();
    assert_eq!(
        service.initialize().await.unwrap().phase,
        ServicePhase::Ready
    );
    service.request_shutdown();
}

#[tokio::test]
async fn offline_catalog_does_not_disable_local_translation() {
    let directory = tempfile::tempdir().unwrap();
    let service = service(directory.path()).await;
    let state = changed(&service, |s| s.catalog.error.is_some()).await;
    assert_eq!(state.phase, ServicePhase::Ready);
    assert!(state.catalog.catalog.is_none());
    assert!(service.refresh_catalog().await.is_err());
    assert!(matches!(
        bounded(service.submit(request("hello")).await.unwrap().result())
            .await
            .unwrap(),
        TranslationOutcome::Skipped(SkipReason::ModelNotInstalled(RU_EN))
    ));
    let blank = service
        .submit(request(" "))
        .await
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(blank, TranslationOutcome::Skipped(SkipReason::BlankInput));
    assert_eq!(service.inventory().await.unwrap().len(), 2);
    service.request_shutdown();
}

#[tokio::test]
async fn offline_catalog_retains_verified_cache_and_error() {
    let directory = tempfile::tempdir().unwrap();
    let catalog = snapshot(&wire());
    CatalogCache::open(directory.path())
        .unwrap()
        .commit(&catalog)
        .unwrap();
    let service = service(directory.path()).await;
    let state = changed(&service, |s| s.catalog.error.is_some()).await;
    assert!(matches!(
        state.catalog.freshness,
        Some(FreshnessDto::Cached)
    ));
    assert_eq!(
        state.catalog.catalog.unwrap().sha256,
        catalog.sha256().to_string()
    );
    service.request_shutdown();
}

#[tokio::test]
async fn concurrent_initializers_share_one_workload_and_shutdown_cannot_reopen_it() {
    let directory = tempfile::tempdir().unwrap();
    let service = TranslationService::new(directory.path().into(), config());
    let (one, two) = tokio::join!(service.initialize(), service.initialize());
    assert_eq!(one.unwrap().phase, ServicePhase::Ready);
    assert_eq!(two.unwrap().phase, ServicePhase::Ready);
    let manager = service.lock().manager.clone().unwrap();
    service.initialize().await.unwrap();
    assert!(Arc::ptr_eq(
        &manager,
        service.lock().manager.as_ref().unwrap()
    ));
    service.request_shutdown();
    assert_eq!(
        service.initialize().await.unwrap_err().code,
        ErrorCode::Shutdown
    );
    assert_eq!(
        service.resume().await.unwrap_err().code,
        ErrorCode::Shutdown
    );
    assert_eq!(
        manager
            .submit(request("late"))
            .await
            .err()
            .unwrap()
            .to_string(),
        WorkloadError::Shutdown.to_string()
    );
}

#[tokio::test]
async fn simultaneous_failed_initializers_share_one_attempt_without_an_implicit_retry() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("blocked");
    std::fs::write(&root, b"obstruction").unwrap();
    let service = TranslationService::new(root, config());
    let lifecycle = service.lifecycle.lock().await;
    let mut first = service.start_initialization();
    let mut second = service.start_initialization();
    assert_eq!(
        service
            .lock()
            .initialization
            .as_ref()
            .unwrap()
            .receiver_count(),
        2
    );
    drop(lifecycle);
    let one = bounded(async {
        first
            .wait_for(|value| value.is_some())
            .await
            .unwrap()
            .clone()
            .unwrap()
    })
    .await;
    let two = bounded(async {
        second
            .wait_for(|value| value.is_some())
            .await
            .unwrap()
            .clone()
            .unwrap()
    })
    .await;
    assert_eq!(one.unwrap_err(), two.unwrap_err());
    assert_eq!(service.snapshot().phase, ServicePhase::Unavailable);
    assert!(service.lock().initialization.is_none());
    service.request_shutdown();
}

#[tokio::test]
async fn ownership_is_checked_and_navigation_removes_only_its_requests() {
    let directory = tempfile::tempdir().unwrap();
    let service = service(directory.path()).await;
    let one = Owner::Webview("one".into());
    let two = Owner::Webview("two".into());
    let id = service.submit_owned(one.clone(), request(" ")).unwrap();
    let other = service.submit_owned(two.clone(), request(" ")).unwrap();
    assert_eq!(
        service.cancel_owned(&two, &id).unwrap_err().code,
        ErrorCode::WrongOwner
    );
    assert_eq!(
        service.result_owned(&two, &id).await.unwrap_err().code,
        ErrorCode::WrongOwner
    );
    service.forget_owner(&one);
    assert_eq!(
        service.result_owned(&one, &id).await.unwrap_err().code,
        ErrorCode::OperationNotFound
    );
    assert!(service.result_owned(&two, &other).await.is_ok());
    assert!(service.lock().requests.is_empty());
    service.request_shutdown();
}

#[derive(Default)]
struct Gate {
    blocked: Mutex<bool>,
    released: Condvar,
    entered: tokio::sync::Notify,
}

impl Gate {
    fn open(&self) {
        *self.blocked.lock().unwrap() = false;
        self.released.notify_all();
    }
}

struct Loader(Arc<Gate>);
struct Engine(Arc<Gate>);

impl EngineLoader for Loader {
    fn load(
        &self,
        _: &ModelDescriptor,
        _: CoreOptions,
    ) -> Result<Arc<dyn InferenceEngine>, String> {
        Ok(Arc::new(Engine(self.0.clone())))
    }
}

impl InferenceEngine for Engine {
    fn source_token_count(&self, _: &str) -> usize {
        2
    }
    fn translate_batch(&self, texts: &[&str]) -> Result<Vec<String>, String> {
        self.0.entered.notify_one();
        let mut blocked = self.0.blocked.lock().unwrap();
        while *blocked {
            blocked = self.0.released.wait(blocked).unwrap();
        }
        if texts.contains(&"failure") {
            return Err("fixture inference failure".into());
        }
        Ok(texts
            .iter()
            .map(|text| format!("translated:{text}"))
            .collect())
    }
}

struct Loaded {
    service: Arc<TranslationService>,
    gate: Arc<Gate>,
    directory: tempfile::TempDir,
}

impl Drop for Loaded {
    fn drop(&mut self) {
        self.gate.open();
        self.service.request_shutdown();
    }
}

async fn loaded(policy: OverflowPolicy) -> Loaded {
    let directory = tempfile::tempdir().unwrap();
    let service = service(directory.path()).await;
    service.release().await.unwrap();
    let gate = Arc::new(Gate::default());
    let store = service.resources().unwrap().store.clone();
    crate::store::tests::install(&store, &snapshot(&wire()), RU_EN);
    let options = CoreOptions::default();
    let core = TranslationCore::with_backends(
        options,
        Arc::new(WhatlangDetector::default()),
        Arc::new(Loader(gate.clone())),
    );
    let manager = WorkloadManager::start(
        store,
        core,
        WorkloadOptions {
            capacity: 1,
            overflow_policy: policy,
            ..WorkloadOptions::default()
        },
        options.native_batch_size,
    )
    .unwrap();
    {
        let mut state = service.lock();
        state.manager = Some(Arc::new(manager));
        state.phase = ServicePhase::Ready;
    }
    Loaded {
        service,
        gate,
        directory,
    }
}

#[tokio::test]
async fn native_success_failure_and_completed_results_release_their_records() {
    let fixture = loaded(OverflowPolicy::RejectNew).await;
    let outcome = fixture
        .service
        .submit(request("example"))
        .await
        .unwrap()
        .result()
        .await
        .unwrap();
    assert!(
        matches!(outcome, TranslationOutcome::Translated(Translation { text, .. }) if text == "translated:example")
    );
    assert_eq!(
        fixture
            .service
            .submit(request("failure"))
            .await
            .unwrap()
            .result()
            .await
            .unwrap_err()
            .code,
        ErrorCode::Inference
    );
    assert!(fixture.service.lock().requests.is_empty());
    let ticket = fixture.service.submit(request(" ")).await.unwrap();
    let id = ticket.id.clone();
    drop(ticket);
    assert!(!fixture.service.lock().requests.contains_key(&id));
}

#[tokio::test]
async fn release_waits_for_leases_then_requires_explicit_resume() {
    let fixture = loaded(OverflowPolicy::RejectNew).await;
    fixture
        .service
        .submit(request("resident"))
        .await
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(
        fixture.service.remove_model(RU_EN).await.unwrap_err().code,
        ErrorCode::Busy
    );
    assert_eq!(
        fixture.service.release().await.unwrap().phase,
        ServicePhase::Paused
    );
    assert!(fixture.service.remove_model(RU_EN).await.unwrap());
    assert_eq!(
        fixture
            .service
            .submit(request("paused"))
            .await
            .err()
            .unwrap()
            .code,
        ErrorCode::Paused
    );
    assert_eq!(
        fixture.service.resume().await.unwrap().phase,
        ServicePhase::Ready
    );
    assert!(fixture.directory.path().exists());
}

#[tokio::test]
async fn cancelling_a_waiting_caller_does_not_cancel_active_inference() {
    let fixture = loaded(OverflowPolicy::Wait).await;
    *fixture.gate.blocked.lock().unwrap() = true;
    let active = fixture.service.submit(request("active")).await.unwrap();
    bounded(fixture.gate.entered.notified()).await;
    let waiting = fixture.service.submit(request("waiting")).await.unwrap();
    waiting.cancel().unwrap();
    assert_eq!(
        bounded(waiting.result()).await.unwrap_err().code,
        ErrorCode::Cancelled
    );
    fixture.gate.open();
    assert!(bounded(active.result()).await.is_ok());
}

#[tokio::test]
async fn dropping_release_waiter_still_finishes_pause_and_signal_shutdown_does_not_wait() {
    let fixture = loaded(OverflowPolicy::RejectNew).await;
    *fixture.gate.blocked.lock().unwrap() = true;
    let active = fixture.service.submit(request("active")).await.unwrap();
    bounded(fixture.gate.entered.notified()).await;
    let service = fixture.service.clone();
    let waiter = tokio::spawn(async move { service.release().await });
    changed(&fixture.service, |s| s.phase == ServicePhase::Releasing).await;
    waiter.abort();
    fixture.gate.open();
    changed(&fixture.service, |s| s.phase == ServicePhase::Paused).await;
    assert!(active.result().await.is_ok());
    fixture.service.resume().await.unwrap();
    fixture.service.request_shutdown();
    assert_eq!(fixture.service.snapshot().phase, ServicePhase::Shutdown);
}

#[tokio::test]
async fn exit_signal_returns_while_active_inference_keeps_its_lease() {
    let fixture = loaded(OverflowPolicy::RejectNew).await;
    *fixture.gate.blocked.lock().unwrap() = true;
    let active = fixture.service.submit(request("active")).await.unwrap();
    bounded(fixture.gate.entered.notified()).await;
    let manager = fixture.service.lock().manager.clone().unwrap();
    let store = fixture.service.resources().unwrap().store.clone();
    let service = fixture.service.clone();
    bounded(tokio::task::spawn_blocking(move || {
        service.request_shutdown()
    }))
    .await
    .unwrap();
    assert_eq!(fixture.service.snapshot().phase, ServicePhase::Shutdown);
    assert!(matches!(store.exclusive(RU_EN), Err(StoreError::Busy(_))));
    drop(active);
    fixture.gate.open();
    bounded(manager.shutdown()).await.unwrap();
    assert!(store.exclusive(RU_EN).is_ok());
}

fn hosted_fixture() -> (tempfile::TempDir, CatalogSnapshot, Server) {
    let directory = tempfile::tempdir().unwrap();
    let (catalog, mut objects) = compressed_catalog();
    objects.insert(
        "/latest.json".into(),
        serde_json::to_vec(&catalog.pointer()).unwrap(),
    );
    let path = catalog
        .pointer()
        .catalog_url()
        .strip_prefix("https://models.jerod.me")
        .unwrap()
        .to_owned();
    objects.insert(path, catalog.as_bytes().to_vec());
    let server = Server::new(&objects);
    (directory, catalog, server)
}

async fn hosted_service(root: &std::path::Path, server: &Server) -> Arc<TranslationService> {
    let service = TranslationService::new(
        root.into(),
        PluginConfig {
            test_origin: Some(server.origin.clone()),
            ..PluginConfig::default()
        },
    );
    service.initialize().await.unwrap();
    changed(&service, |s| {
        s.catalog.catalog.is_some() && !s.catalog.refreshing
    })
    .await;
    service
}

#[tokio::test]
async fn startup_fetches_metadata_only_and_installation_survives_its_owner() {
    let (directory, catalog, server) = hosted_fixture();
    let service = hosted_service(directory.path(), &server).await;
    assert_eq!(server.total(), 2);
    let plan = service.plan_install(RU_EN).await.unwrap();
    assert_eq!(plan.model.id, catalog.model(RU_EN).unwrap().id());
    assert_eq!(server.total(), 2);
    let operation = service.start_install(plan).await.unwrap();
    service.forget_owner(&Owner::Webview("closed".into()));
    let state = changed(&service, |s| {
        s.installation.as_ref().is_some_and(|v| v.finished)
    })
    .await;
    let reconnect = service.installation(&operation.id).unwrap();
    assert!(state.installation.unwrap().result.is_some());
    assert!(reconnect.result.is_some());
    assert!(service.verify_model(RU_EN).await.is_ok());
    service.cancel_install(&operation.id).unwrap();
    service.request_shutdown();
}

#[tokio::test]
async fn installation_selection_stays_pinned_across_catalog_refresh() {
    let (directory, catalog, server) = hosted_fixture();
    let service = hosted_service(directory.path(), &server).await;
    let plan = service.plan_install(RU_EN).await.unwrap();
    let mut wire: serde_json::Value = serde_json::from_slice(catalog.as_bytes()).unwrap();
    wire["models"][0]["version"] = serde_json::json!("future");
    let updated = snapshot(&wire);
    CatalogCache::open(directory.path())
        .unwrap()
        .commit(&updated)
        .unwrap();
    let operation = service.start_install(plan).await.unwrap();
    changed(&service, |s| {
        s.installation.as_ref().is_some_and(|v| v.finished)
    })
    .await;
    assert_eq!(
        service
            .installation(&operation.id)
            .unwrap()
            .result
            .unwrap()
            .model
            .id,
        catalog.model(RU_EN).unwrap().id()
    );
    let mut invalid = service.plan_install(RU_EN).await.unwrap();
    invalid.model_id = "wrong".into();
    assert_eq!(
        service.start_install(invalid).await.unwrap_err().code,
        ErrorCode::InvalidPlan
    );
    service.request_shutdown();
}

#[tokio::test]
async fn installation_cancellation_and_busy_are_visible_after_reconnect() {
    let (directory, catalog, server) = hosted_fixture();
    let file = catalog.model(RU_EN).unwrap().required_files()[0].clone();
    let path = file.url.strip_prefix("https://models.jerod.me").unwrap();
    server.replies(path, vec![Reply::Stalled(vec![0; file.size as usize])]);
    let service = hosted_service(directory.path(), &server).await;
    let plan = service.plan_install(RU_EN).await.unwrap();
    let operation = service.start_install(plan.clone()).await.unwrap();
    changed(&service, |s| {
        s.installation
            .as_ref()
            .is_some_and(|v| v.progress.phase == InstallPhaseDto::Downloading)
    })
    .await;
    assert_eq!(
        service.start_install(plan).await.unwrap_err().code,
        ErrorCode::Busy
    );
    service.cancel_install(&operation.id).unwrap();
    changed(&service, |s| {
        s.installation.as_ref().is_some_and(|v| v.finished)
    })
    .await;
    let terminal = service.installation(&operation.id).unwrap();
    assert_eq!(terminal.error.unwrap().code, ErrorCode::Cancelled);
    assert!(terminal.result.is_none());
    assert!(service.discard_downloads(RU_EN).await.is_ok());
    service.request_shutdown();
}
