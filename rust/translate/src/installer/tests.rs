use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::Instant,
};

use serde_json::{json, Value};

use super::*;
use crate::{
    catalog::{
        hash,
        tests::{snapshot, wire, EN_RU, RU_EN},
    },
    InstallationState, LoadMode, TranslationCore,
};

#[derive(Clone)]
enum Reply {
    Body(u16, Vec<u8>),
    Chunked(Vec<u8>),
    Length(Vec<u8>, u64),
    Stalled(Vec<u8>),
}

struct Server {
    origin: String,
    routes: Arc<Mutex<HashMap<String, VecDeque<Reply>>>>,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Server {
    fn new(objects: &HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let routes = Arc::new(Mutex::new(
            objects
                .iter()
                .map(|(path, bytes)| {
                    (
                        path.clone(),
                        VecDeque::from([Reply::Body(200, bytes.clone())]),
                    )
                })
                .collect::<HashMap<_, _>>(),
        ));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (worker_routes, worker_requests, worker_stop) =
            (routes.clone(), requests.clone(), stop.clone());
        let worker = thread::spawn(move || {
            let mut children = Vec::new();
            while !worker_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let (routes, requests, stop) = (
                            worker_routes.clone(),
                            worker_requests.clone(),
                            worker_stop.clone(),
                        );
                        children.push(thread::spawn(move || {
                            let mut headers = Vec::new();
                            let mut byte = [0];
                            while !headers.ends_with(b"\r\n\r\n") {
                                if headers.len() > 16384 || stream.read_exact(&mut byte).is_err() { return; }
                                headers.push(byte[0]);
                            }
                            let request = String::from_utf8_lossy(&headers);
                            let path = request.split_whitespace().nth(1).unwrap().to_owned();
                            assert!(request.to_ascii_lowercase().contains("accept-encoding: identity"));
                            requests.lock().unwrap().push(path.clone());
                            let reply = {
                                let mut all = routes.lock().unwrap();
                                let replies = all.get_mut(&path);
                                match replies {
                                    Some(replies) if replies.len() > 1 => replies.pop_front().unwrap(),
                                    Some(replies) => replies.front().unwrap().clone(),
                                    None => Reply::Body(404, Vec::new()),
                                }
                            };
                            match reply {
                                Reply::Body(status, body) => {
                                    let _ = write!(stream, "HTTP/1.1 {status} fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                                    let _ = stream.write_all(&body);
                                }
                                Reply::Chunked(body) => {
                                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");
                                    let _ = write!(stream, "{:x}\r\n", body.len());
                                    let _ = stream.write_all(&body);
                                    let _ = stream.write_all(b"\r\n0\r\n\r\n");
                                }
                                Reply::Length(body, size) => {
                                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {size}\r\nConnection: close\r\n\r\n");
                                    let _ = stream.write_all(&body);
                                }
                                Reply::Stalled(body) => {
                                    let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                                    let _ = stream.write_all(&body[..1]);
                                    let _ = stream.flush();
                                    while !stop.load(Ordering::SeqCst) { thread::sleep(Duration::from_millis(2)); }
                                }
                            }
                        }));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1))
                    }
                    Err(error) => panic!("fixture accept: {error}"),
                }
            }
            for child in children {
                child.join().unwrap();
            }
        });
        Self {
            origin,
            routes,
            requests,
            stop,
            thread: Some(worker),
        }
    }
    fn replies(&self, path: &str, replies: Vec<Reply>) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.into(), replies.into());
    }
    fn count(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.as_str() == path)
            .count()
    }
    fn total(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

pub(crate) fn compressed_catalog() -> (CatalogSnapshot, HashMap<String, Vec<u8>>) {
    let mut value = wire();
    let mut objects = HashMap::new();
    for model in value["models"].as_array_mut().unwrap().iter_mut().take(2) {
        for file in model["files"].as_array_mut().unwrap() {
            let contents: &[u8] = match file["role"].as_str().unwrap() {
                "model" => b"weights",
                "vocab" => b"vocabulary",
                _ => b"lexicon",
            };
            let compressed = zstd::stream::encode_all(contents, 1).unwrap();
            let sha = hash(&compressed);
            let path = format!("/objects/sha256/{sha}.zst");
            file["size"] = json!(compressed.len());
            file["sha256"] = json!(sha.to_string());
            file["url"] = json!(format!("https://models.jerod.me{path}"));
            objects.insert(path, compressed);
        }
    }
    (snapshot(&value), objects)
}

fn setup() -> (
    tempfile::TempDir,
    ModelStore,
    CatalogSnapshot,
    Server,
    ModelInstaller,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = ModelStore::open(directory.path()).unwrap();
    let (catalog, objects) = compressed_catalog();
    let server = Server::new(&objects);
    let mut installer = ModelInstaller::new(store.clone()).unwrap();
    installer.client = reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .read_timeout(Duration::from_millis(100))
        .no_proxy()
        .build()
        .unwrap();
    installer.test_origin = Some(server.origin.clone());
    installer.retry_unit = Duration::from_millis(1);
    (directory, store, catalog, server, installer)
}

fn path(file: &CatalogArtifact) -> String {
    file.url
        .strip_prefix("https://models.jerod.me")
        .unwrap()
        .to_owned()
}
fn plan(catalog: &CatalogSnapshot) -> InstallPlan {
    InstallPlan::new(catalog, RU_EN).unwrap()
}

#[test]
fn construction_and_plan_creation_need_no_runtime_and_make_no_requests() {
    let (_directory, store, catalog, server, _installer) = setup();
    let selected = plan(&catalog);
    assert_eq!(selected.model().pair(), RU_EN);
    assert_eq!(InstallControl::new().progress().phase, InstallPhase::Idle);
    assert_eq!(server.total(), 0);
    assert!(!store.root().join("models/rus-eng").exists());
}

async fn until(mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "condition timed out");
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

#[tokio::test]
async fn explicit_install_both_pairs_pins_metadata_deduplicates_and_cleans_compressed_files() {
    let (_directory, store, catalog, server, installer) = setup();
    let selected = plan(&catalog);
    let mut newer = wire();
    newer["models"][0]["version"] = json!("new");
    crate::CatalogCache::open(store.root())
        .unwrap()
        .commit(&snapshot(&newer))
        .unwrap();
    assert_eq!(server.total(), 0);
    let control = InstallControl::new();
    let result = installer.install(selected, control.clone()).await.unwrap();
    assert_eq!(result.status, InstallStatus::Installed);
    assert_eq!(result.model.id(), catalog.model(RU_EN).unwrap().id());
    assert_eq!(server.total(), 2);
    assert_eq!(store.verify(RU_EN).unwrap().id(), result.model.id());
    let progress = control.progress();
    assert_eq!(progress.phase, InstallPhase::Completed);
    assert_eq!(progress.completed_download_bytes, progress.download_total);
    assert_eq!(progress.completed_installed_bytes, progress.installed_total);
    assert_eq!(progress.network_bytes, progress.download_total);
    assert!(!store
        .root()
        .join(installation::area(".downloads", RU_EN))
        .exists());
    let no_op = installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap();
    assert_eq!(no_op.status, InstallStatus::AlreadyInstalled);
    assert_eq!(server.total(), 2);
    installer
        .install(
            InstallPlan::new(&catalog, EN_RU).unwrap(),
            InstallControl::new(),
        )
        .await
        .unwrap();
    store.verify(EN_RU).unwrap();
    assert_eq!(server.total(), 4);
}

#[tokio::test]
async fn normal_local_operations_and_failed_inference_never_fetch_models() {
    let (_directory, store, catalog, server, installer) = setup();
    let _plan = plan(&catalog);
    store.inventory(Some(&catalog)).unwrap();
    assert!(store.acquire(RU_EN).is_err());
    assert!(store.verify(RU_EN).is_err());
    let mut core = TranslationCore::default();
    core.translate(&crate::TranslationRequest::new(
        "hello",
        crate::Language::RUSSIAN,
    ))
    .unwrap();
    assert_eq!(server.total(), 0);
    installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap();
    let requests = server.total();
    core.register_stored_model(store.acquire(RU_EN).unwrap(), LoadMode::Lazy)
        .unwrap();
    let mut request = crate::TranslationRequest::new("hello", crate::Language::ENGLISH);
    request.source = crate::SourceLanguage::Explicit(crate::Language::RUSSIAN);
    assert!(core.translate(&request).is_err());
    assert_eq!(server.total(), requests);
}

#[tokio::test]
async fn busy_models_and_store_wide_lock_reject_before_http() {
    let (_directory, store, catalog, server, installer) = setup();
    let global = installation::global_lock(&store, RU_EN).unwrap();
    assert!(matches!(
        installer
            .install(
                InstallPlan::new(&catalog, EN_RU).unwrap(),
                InstallControl::new()
            )
            .await,
        Err(InstallError::Store(StoreError::Busy(_)))
    ));
    drop(global);
    let pair_guard = store.exclusive(RU_EN).unwrap();
    assert!(matches!(
        installer
            .install(plan(&catalog), InstallControl::new())
            .await,
        Err(InstallError::Store(StoreError::Busy(_)))
    ));
    drop(pair_guard);
    assert_eq!(server.total(), 0);
    installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap();
    let lease = store.acquire(RU_EN).unwrap();
    let count = server.total();
    assert!(matches!(
        installer
            .install(plan(&catalog), InstallControl::new())
            .await,
        Err(InstallError::Store(StoreError::Busy(_)))
    ));
    assert_eq!(server.total(), count);
    drop(lease);
}

#[tokio::test]
async fn retries_are_bounded_and_complete_objects_survive_failure_and_restart() {
    let (_directory, store, catalog, server, installer) = setup();
    let model = catalog.model(RU_EN).unwrap();
    let vocab = path(model.source_tokenizer());
    server.replies(&vocab, vec![Reply::Body(503, Vec::new())]);
    assert!(matches!(
        installer
            .install(plan(&catalog), InstallControl::new())
            .await,
        Err(InstallError::Http(503))
    ));
    assert_eq!(server.count(&vocab), 3);
    assert_eq!(server.count(&path(model.model())), 1);
    let reopened = ModelStore::open(store.root()).unwrap();
    assert!(matches!(
        reopened.verify(RU_EN),
        Err(StoreError::NotInstalled(_))
    ));
    let (_, objects) = compressed_catalog();
    server.replies(&vocab, vec![Reply::Body(200, objects[&vocab].clone())]);
    let control = InstallControl::new();
    installer
        .install(plan(&catalog), control.clone())
        .await
        .unwrap();
    assert_eq!(server.count(&path(model.model())), 1);
    assert_eq!(control.progress().reused_bytes, model.model().size);
    assert_eq!(
        control.progress().network_bytes,
        model.source_tokenizer().size
    );
}

#[tokio::test]
async fn transient_failure_recovers_but_permanent_http_and_integrity_failures_do_not_retry() {
    let (_directory, store, catalog, server, installer) = setup();
    let file = catalog.model(RU_EN).unwrap().model();
    let target = path(file);
    let (_, objects) = compressed_catalog();
    server.replies(
        &target,
        vec![
            Reply::Body(502, Vec::new()),
            Reply::Body(200, objects[&target].clone()),
        ],
    );
    installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap();
    assert_eq!(server.count(&target), 2);
    store.remove(RU_EN).unwrap();
    for status in [301, 401, 403, 404, 429] {
        let count = server.count(&target);
        server.replies(&target, vec![Reply::Body(status, Vec::new())]);
        assert!(
            matches!(installer.install(plan(&catalog), InstallControl::new()).await, Err(InstallError::Http(actual)) if actual == status)
        );
        assert_eq!(server.count(&target), count + 1);
    }
    let count = server.count(&target);
    let mut corrupted = objects[&target].clone();
    corrupted[0] ^= 1;
    server.replies(&target, vec![Reply::Body(200, corrupted)]);
    assert!(matches!(
        installer
            .install(plan(&catalog), InstallControl::new())
            .await,
        Err(InstallError::Integrity { .. })
    ));
    assert_eq!(server.count(&target), count + 1);
    assert!(
        fs::read_dir(store.root().join(installation::area(".downloads", RU_EN)))
            .unwrap()
            .all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".partial-"))
    );
}

#[tokio::test]
async fn content_length_chunked_limits_and_premature_disconnect_are_checked() {
    let (_directory, _store, catalog, server, installer) = setup();
    let file = catalog.model(RU_EN).unwrap().model();
    let target = path(file);
    let (_, objects) = compressed_catalog();
    for reply in [
        Reply::Length(objects[&target].clone(), file.size + 1),
        Reply::Chunked(vec![0; file.size as usize + 1]),
        Reply::Chunked(vec![0; file.size as usize - 1]),
    ] {
        let count = server.count(&target);
        server.replies(&target, vec![reply]);
        assert!(matches!(
            installer
                .install(plan(&catalog), InstallControl::new())
                .await,
            Err(InstallError::Integrity { .. })
        ));
        assert_eq!(server.count(&target), count + 1);
    }
    let count = server.count(&target);
    server.replies(&target, vec![Reply::Length(vec![0], file.size)]);
    assert!(matches!(
        installer
            .install(plan(&catalog), InstallControl::new())
            .await,
        Err(InstallError::Transport(_))
    ));
    assert_eq!(server.count(&target), count + 3);
}

#[tokio::test]
async fn stalled_reads_retry_within_budget() {
    let (_directory, _store, catalog, server, installer) = setup();
    let file = catalog.model(RU_EN).unwrap().model();
    let target = path(file);
    let (_, objects) = compressed_catalog();
    server.replies(&target, vec![Reply::Stalled(objects[&target].clone())]);
    assert!(
        matches!(installer.install(plan(&catalog), InstallControl::new()).await, Err(InstallError::Transport(error)) if error.is_timeout())
    );
    assert_eq!(server.count(&target), 3);
}

#[tokio::test]
async fn cancellation_preserves_complete_downloads_and_dropping_future_stops_requests() {
    let (_directory, store, catalog, server, installer) = setup();
    let installer = Arc::new(installer);
    let vocab = path(catalog.model(RU_EN).unwrap().source_tokenizer());
    let (_, objects) = compressed_catalog();
    server.replies(&vocab, vec![Reply::Stalled(objects[&vocab].clone())]);
    let control = InstallControl::new();
    let (task_installer, task_control, task_plan) =
        (installer.clone(), control.clone(), plan(&catalog));
    let task = tokio::spawn(async move { task_installer.install(task_plan, task_control).await });
    until(|| server.count(&vocab) == 1).await;
    control.cancel();
    assert!(matches!(task.await.unwrap(), Err(InstallError::Cancelled)));
    assert_eq!(control.progress().phase, InstallPhase::Cancelled);
    assert_eq!(server.count(&vocab), 1);
    assert!(store
        .root()
        .join(installation::area(".downloads", RU_EN))
        .join(format!(
            "{}.zst",
            catalog.model(RU_EN).unwrap().model().sha256
        ))
        .exists());
    let (task_installer, task_plan) = (installer.clone(), plan(&catalog));
    let task = tokio::spawn(async move {
        task_installer
            .install(task_plan, InstallControl::new())
            .await
    });
    until(|| server.count(&vocab) == 2).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    until(|| installation::global_lock(&store, RU_EN).is_ok()).await;
    let reopened = ModelStore::open(store.root()).unwrap();
    assert_eq!(reopened.recovery_report().rolled_back, vec![RU_EN]);
    assert_eq!(server.count(&vocab), 2);
    assert!(installer.discard_downloads(RU_EN).unwrap());
    assert!(!installer.discard_downloads(RU_EN).unwrap());
}

#[tokio::test]
async fn cancellation_interrupts_retry_wait_and_control_cannot_be_reused() {
    let (_directory, _store, catalog, server, mut installer) = setup();
    installer.retry_unit = Duration::from_secs(30);
    let installer = Arc::new(installer);
    let target = path(catalog.model(RU_EN).unwrap().model());
    server.replies(&target, vec![Reply::Body(503, Vec::new())]);
    let control = InstallControl::new();
    let (task_installer, task_control, task_plan) =
        (installer.clone(), control.clone(), plan(&catalog));
    let task = tokio::spawn(async move { task_installer.install(task_plan, task_control).await });
    until(|| control.progress().phase == InstallPhase::RetryWaiting).await;
    control.cancel();
    assert!(matches!(task.await.unwrap(), Err(InstallError::Cancelled)));
    assert_eq!(server.count(&target), 1);
    assert!(matches!(
        installer.install(plan(&catalog), control).await,
        Err(InstallError::ControlUsed)
    ));
    let cancelled = InstallControl::new();
    cancelled.cancel();
    assert!(matches!(
        installer.install(plan(&catalog), cancelled).await,
        Err(InstallError::Cancelled)
    ));
    assert_eq!(server.count(&target), 1);
}

#[tokio::test]
async fn damaged_installations_are_repaired_and_unmanaged_directories_are_preserved() {
    let (_directory, store, catalog, server, installer) = setup();
    installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap();
    let weights = store.root().join("models/rus-eng/weights.bin");
    fs::write(&weights, b"broken!").unwrap();
    installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap();
    assert_eq!(fs::read(&weights).unwrap(), b"weights");
    let count = server.total();
    fs::write(store.root().join("models/rus-eng/install.json"), b"invalid").unwrap();
    assert!(matches!(
        installer
            .install(plan(&catalog), InstallControl::new())
            .await,
        Err(InstallError::Store(StoreError::Unmanaged(_)))
    ));
    assert_eq!(server.total(), count);
    assert_eq!(fs::read(weights).unwrap(), b"weights");
}

#[tokio::test]
async fn corrupt_retained_objects_are_rehashed_and_replaced() {
    let (_directory, store, catalog, server, installer) = setup();
    let model = catalog.model(RU_EN).unwrap();
    let vocab = path(model.source_tokenizer());
    server.replies(&vocab, vec![Reply::Body(404, Vec::new())]);
    installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap_err();
    let retained = store
        .root()
        .join(installation::area(".downloads", RU_EN))
        .join(format!("{}.zst", model.model().sha256));
    fs::write(&retained, vec![0; model.model().size as usize]).unwrap();
    let (_, objects) = compressed_catalog();
    server.replies(&vocab, vec![Reply::Body(200, objects[&vocab].clone())]);
    installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap();
    assert_eq!(server.count(&path(model.model())), 2);
}

#[tokio::test]
async fn invalid_compression_and_decompressed_integrity_fail_without_publication() {
    for mutation in 0..4 {
        let (_directory, store, original, server, installer) = setup();
        let mut value: Value = serde_json::from_slice(original.as_bytes()).unwrap();
        let file = &mut value["models"][0]["files"][0];
        match mutation {
            0 => file["decompressed_size"] = json!(1),
            1 => file["decompressed_size"] = json!(100),
            2 => file["decompressed_sha256"] = json!(hash(b"wrong").to_string()),
            _ => {
                let bytes = b"not a zstandard stream".to_vec();
                let sha = hash(&bytes);
                let target = format!("/objects/sha256/{sha}.zst");
                file["size"] = json!(bytes.len());
                file["sha256"] = json!(sha.to_string());
                file["url"] = json!(format!("https://models.jerod.me{target}"));
                server.replies(&target, vec![Reply::Body(200, bytes)]);
            }
        }
        let catalog = snapshot(&value);
        let error = installer
            .install(plan(&catalog), InstallControl::new())
            .await
            .unwrap_err();
        assert!(
            matches!(
                error,
                InstallError::Integrity { .. } | InstallError::Decompression { .. }
            ),
            "{error}"
        );
        assert!(matches!(
            store.verify(RU_EN),
            Err(StoreError::NotInstalled(_))
        ));
        assert_eq!(
            store
                .inventory(None)
                .unwrap()
                .iter()
                .find(|e| e.pair == Some(RU_EN))
                .unwrap()
                .state,
            InstallationState::NotInstalled
        );
    }
}

#[tokio::test]
async fn verification_and_decompression_workers_cancel_before_publication() {
    for phase in [InstallPhase::Verifying, InstallPhase::Decompressing] {
        let (_directory, store, catalog, _server, installer) = setup();
        let control = InstallControl::new();
        *control.0.worker_pause.lock().unwrap() = Some(phase);
        let task_control = control.clone();
        let task_plan = plan(&catalog);
        let task = tokio::spawn(async move { installer.install(task_plan, task_control).await });
        until(|| control.0.paused.load(Ordering::SeqCst)).await;
        assert!(matches!(
            installation::global_lock(&store, EN_RU),
            Err(StoreError::Busy(_))
        ));
        control.cancel();
        assert!(matches!(task.await.unwrap(), Err(InstallError::Cancelled)));
        installation::global_lock(&store, EN_RU).unwrap();
        assert!(!store.root().join("models/rus-eng").exists());
        assert_eq!(control.progress().phase, InstallPhase::Cancelled);
    }
}

#[tokio::test]
async fn cancellation_after_publication_boundary_returns_installed_result() {
    let (_directory, store, catalog, _server, installer) = setup();
    let control = InstallControl::new();
    control.0.cancel_on_publish.store(true, Ordering::SeqCst);
    let result = installer
        .install(plan(&catalog), control.clone())
        .await
        .unwrap();
    assert_eq!(result.status, InstallStatus::Installed);
    assert_eq!(control.progress().phase, InstallPhase::Completed);
    store.verify(RU_EN).unwrap();
}

#[tokio::test]
async fn replacement_failure_preserves_old_revision_then_explicit_retry_publishes_new() {
    let (_directory, store, original, server, installer) = setup();
    installer
        .install(plan(&original), InstallControl::new())
        .await
        .unwrap();
    let original_manifest = fs::read(store.root().join("models/rus-eng/install.json")).unwrap();
    let mut value: Value = serde_json::from_slice(original.as_bytes()).unwrap();
    value["models"][0]["version"] = json!("4.0");
    let next = snapshot(&value);
    let vocab = path(next.model(RU_EN).unwrap().source_tokenizer());
    server.replies(&vocab, vec![Reply::Body(404, Vec::new())]);
    assert!(installer
        .install(plan(&next), InstallControl::new())
        .await
        .is_err());
    assert_eq!(
        fs::read(store.root().join("models/rus-eng/install.json")).unwrap(),
        original_manifest
    );
    assert_eq!(
        store.verify(RU_EN).unwrap().id(),
        original.model(RU_EN).unwrap().id()
    );
    let (_, objects) = compressed_catalog();
    server.replies(&vocab, vec![Reply::Body(200, objects[&vocab].clone())]);
    installer
        .install(plan(&next), InstallControl::new())
        .await
        .unwrap();
    assert_eq!(
        store.verify(RU_EN).unwrap().id(),
        next.model(RU_EN).unwrap().id()
    );
}

#[cfg(windows)]
#[tokio::test]
async fn restored_or_committed_cleanup_warns_for_existing_models_and_defers_replacement() {
    use std::os::windows::fs::OpenOptionsExt;
    for committed in [false, true] {
        let (_directory, store, original, server, installer) = setup();
        installer
            .install(plan(&original), InstallControl::new())
            .await
            .unwrap();
        let mut value: Value = serde_json::from_slice(original.as_bytes()).unwrap();
        value["models"][0]["version"] = json!("4.0");
        let next = snapshot(&value);
        store.retain_catalog(&next).unwrap();
        let held = {
            let _global = installation::global_lock(&store, RU_EN).unwrap();
            let _direction = store.exclusive(RU_EN).unwrap();
            let model = next.model(RU_EN).unwrap();
            let staged = installation::begin(&store, model, original.model(RU_EN)).unwrap();
            for file in model.required_files() {
                fs::copy(
                    store.root().join("models/rus-eng").join(&file.filename),
                    store.root().join(&staged).join(&file.filename),
                )
                .unwrap();
            }
            fs::write(
                store
                    .root()
                    .join(&staged)
                    .join(crate::INSTALL_MANIFEST_FILENAME),
                serde_json::to_vec(&crate::InstallationManifest::for_model(model)).unwrap(),
            )
            .unwrap();
            let blocked = if committed {
                let downloads = installation::downloads(&store, RU_EN).unwrap();
                let path = store.root().join(downloads).join("retained.zst");
                fs::write(&path, b"retained").unwrap();
                path
            } else {
                store.root().join(staged).join("weights.bin")
            };
            let held = fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(blocked)
                .unwrap();
            if committed {
                assert!(!installation::publish(&store, RU_EN).unwrap().is_empty());
            }
            held
        };
        let (current, replacement) = if committed {
            (&next, &original)
        } else {
            (&original, &next)
        };
        let reopened = ModelStore::open(store.root()).unwrap();
        assert!(!reopened.recovery_report().warnings.is_empty());
        let requests = server.total();
        let result = installer
            .install(plan(current), InstallControl::new())
            .await
            .unwrap();
        assert_eq!(result.status, InstallStatus::AlreadyInstalled);
        assert!(!result.cleanup_warnings.is_empty());
        assert_eq!(server.total(), requests);
        assert!(matches!(
            installer
                .install(plan(replacement), InstallControl::new())
                .await,
            Err(InstallError::Store(StoreError::CleanupPending { .. }))
        ));
        assert_eq!(server.total(), requests);
        assert!(matches!(
            store.remove(RU_EN),
            Err(StoreError::CleanupPending { .. })
        ));
        assert_eq!(
            store.verify(RU_EN).unwrap().id(),
            current.model(RU_EN).unwrap().id()
        );
        drop(held);
        let result = installer
            .install(plan(replacement), InstallControl::new())
            .await
            .unwrap();
        assert_eq!(result.status, InstallStatus::Installed);
        assert!(result.cleanup_warnings.is_empty());
        assert_eq!(
            store.verify(RU_EN).unwrap().id(),
            replacement.model(RU_EN).unwrap().id()
        );
    }
}

#[tokio::test]
async fn missing_download_ownership_is_structured_preserved_and_manually_recoverable() {
    let (_directory, store, catalog, server, installer) = setup();
    let relative = installation::downloads(&store, RU_EN).unwrap();
    let directory = store.root().join(&relative);
    fs::write(directory.join("unknown.zst"), b"preserve me").unwrap();
    fs::remove_file(directory.join("owner.json")).unwrap();
    fs::remove_file(store.root().join(relative.with_extension("json"))).unwrap();
    let error = installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap_err();
    match &error {
        InstallError::Store(StoreError::UnmanagedDownloads {
            pair,
            directory: actual,
            detail,
        }) => {
            assert_eq!(*pair, RU_EN);
            assert_eq!(actual, &directory);
            assert!(detail.contains("owner.json"));
        }
        error => panic!("unexpected error: {error}"),
    }
    assert!(error
        .to_string()
        .contains("move this directory outside the store"));
    assert!(matches!(
        installer.discard_downloads(RU_EN),
        Err(InstallError::Store(StoreError::UnmanagedDownloads { .. }))
    ));
    assert_eq!(server.total(), 0);
    let reopened = ModelStore::open(store.root()).unwrap();
    assert!(reopened
        .recovery_report()
        .warnings
        .iter()
        .any(|warning| warning.contains(&directory.display().to_string())));
    assert_eq!(
        fs::read(directory.join("unknown.zst")).unwrap(),
        b"preserve me"
    );
    assert!(!directory.join("owner.json").exists());
    let outside = tempfile::tempdir().unwrap();
    fs::rename(&directory, outside.path().join("saved-downloads")).unwrap();
    let reopened = ModelStore::open(store.root()).unwrap();
    assert!(reopened.recovery_report().warnings.is_empty());
    let result = installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap();
    assert_eq!(result.status, InstallStatus::Installed);
    assert!(server.total() > 0);
    assert_eq!(
        fs::read(outside.path().join("saved-downloads/unknown.zst")).unwrap(),
        b"preserve me"
    );
}

#[tokio::test]
async fn linked_retained_objects_are_rejected_without_touching_outside_files() {
    let (_directory, store, catalog, server, installer) = setup();
    let model = catalog.model(RU_EN).unwrap();
    let vocab = path(model.source_tokenizer());
    server.replies(&vocab, vec![Reply::Body(404, Vec::new())]);
    installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .unwrap_err();
    let retained = store
        .root()
        .join(installation::area(".downloads", RU_EN))
        .join(format!("{}.zst", model.model().sha256));
    fs::remove_file(&retained).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("outside.zst");
    fs::write(&target, b"untouched").unwrap();
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&target, &retained);
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&target, &retained);
    if let Err(error) = linked {
        if error.kind() == std::io::ErrorKind::PermissionDenied
            || error.raw_os_error() == Some(1314)
        {
            return;
        }
        panic!("cannot create test symlink: {error}");
    }
    let count = server.total();
    assert!(installer
        .install(plan(&catalog), InstallControl::new())
        .await
        .is_err());
    assert_eq!(server.total(), count);
    assert!(installer.discard_downloads(RU_EN).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"untouched");
}

#[tokio::test]
async fn filesystem_write_failures_stop_transfers_without_retry_and_allow_explicit_retry() {
    for phase in [InstallPhase::Downloading, InstallPhase::Decompressing] {
        let (_directory, store, catalog, server, installer) = setup();
        let model = catalog.model(RU_EN).unwrap();
        let target = path(model.model());
        let (_, objects) = compressed_catalog();
        if phase == InstallPhase::Downloading {
            server.replies(&target, vec![Reply::Stalled(objects[&target].clone())]);
        }
        let control = InstallControl::new();
        *control.0.write_failure.lock().unwrap() = Some(phase);
        assert!(
            matches!(installer.install(plan(&catalog), control).await, Err(InstallError::Io(error)) if error.to_string().contains("injected storage write failure"))
        );
        assert_eq!(server.count(&target), 1);
        assert!(!store.root().join("models/rus-eng").exists());
        server.replies(&target, vec![Reply::Body(200, objects[&target].clone())]);
        installer
            .install(plan(&catalog), InstallControl::new())
            .await
            .unwrap();
        store.verify(RU_EN).unwrap();
    }
}
