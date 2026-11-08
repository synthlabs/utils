use std::{
    collections::VecDeque,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
};

use super::*;
use crate::catalog::tests::{snapshot, wire, RU_EN};

struct Server {
    origin: String,
    requests: Arc<Mutex<Vec<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

fn response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

impl Server {
    fn new(responses: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        listener.set_nonblocking(true).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let handle = thread::spawn(move || {
            let mut responses = VecDeque::from(responses);
            while !stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // Winsock inherits the listener's nonblocking mode.
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        stream
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut request = Vec::new();
                        while !request.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            match stream.read(&mut byte) {
                                Ok(0) => break,
                                Ok(_) => {}
                                Err(error) => {
                                    seen.lock()
                                        .unwrap()
                                        .push(format!("fixture read error: {error:?}"));
                                    break;
                                }
                            }
                            request.push(byte[0]);
                        }
                        let text = String::from_utf8_lossy(&request).into_owned();
                        seen.lock().unwrap().push(text);
                        let reply = responses
                            .pop_front()
                            .unwrap_or_else(|| response("500 Unexpected request", b""));
                        let _ = stream.write_all(&reply);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("fixture server: {error}"),
                }
            }
        });
        Self {
            origin,
            requests,
            stop,
            thread: Some(handle),
        }
    }

    fn client(&self, root: &Path) -> CatalogClient {
        CatalogClient {
            retry_wait: Default::default(),
            request_gate: None,
            cache: CatalogCache::open(root).unwrap(),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .no_proxy()
                .build()
                .unwrap(),
            test_origin: Some(self.origin.clone()),
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}

#[tokio::test]
async fn initialization_fetches_metadata_only_and_local_operations_do_not_fetch() {
    let directory = tempfile::tempdir().unwrap();
    let catalog = snapshot(&wire());
    let pointer = serde_json::to_vec(&catalog.pointer()).unwrap();
    let server = Server::new(vec![
        response("200 OK", &pointer),
        response("200 OK", catalog.as_bytes()),
        response("200 OK", &pointer),
    ]);
    let client = server.client(directory.path());
    let loaded = client.initialize().await.unwrap_or_else(|error| {
        panic!(
            "{error:?}; fixture requests: {:?}",
            server.requests.lock().unwrap()
        )
    });
    assert_eq!(loaded.freshness, CatalogFreshness::Fresh);
    assert!(loaded.refresh_error.is_none());
    let store = crate::ModelStore::open(directory.path()).unwrap();
    crate::store::tests::install(&store, &catalog, RU_EN);
    store.inventory(Some(&loaded.snapshot)).unwrap();
    store.verify(RU_EN).unwrap();
    let lease = store.acquire(RU_EN).unwrap();
    let mut core = crate::TranslationCore::default();
    core.register_stored_model(lease, crate::LoadMode::Lazy)
        .unwrap();
    // Synthetic weights fail locally; inference must never try to download replacements.
    let mut request = crate::TranslationRequest::new("hello", Language::ENGLISH);
    request.source = crate::SourceLanguage::Explicit(Language::RUSSIAN);
    assert!(core.translate(&request).is_err());
    assert_eq!(client.cached().unwrap().sha256(), catalog.sha256());
    assert_eq!(server.requests.lock().unwrap().len(), 2);
    client.refresh().await.unwrap();
    let requests = server.requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET /latest.json "));
    assert!(requests[1].starts_with("GET /snapshots/"));
    assert!(requests[2].starts_with("GET /latest.json "));
    assert!(requests
        .iter()
        .all(|r| r.to_ascii_lowercase().contains("cache-control: no-cache")));
    assert!(requests.iter().all(|r| !r.contains("/objects/")));
}

#[tokio::test]
async fn refresh_errors_fall_back_only_to_verified_cache() {
    let directory = tempfile::tempdir().unwrap();
    let catalog = snapshot(&wire());
    let server = Server::new(vec![
        response("503 Unavailable", b""),
        response("503 Unavailable", b""),
        response("503 Unavailable", b""),
    ]);
    let client = server.client(directory.path());
    assert!(client.initialize().await.is_err());
    client.cache.commit(&catalog).unwrap();
    let load = client.refresh().await.unwrap();
    assert_eq!(load.freshness, CatalogFreshness::Cached);
    assert!(load.refresh_error.is_some());
    assert_eq!(load.snapshot.sha256(), catalog.sha256());
    std::fs::write(
        directory
            .path()
            .join("catalog")
            .join(format!("{}.json", catalog.sha256())),
        b"tampered",
    )
    .unwrap();
    assert!(client.refresh().await.is_err());
}

#[tokio::test]
async fn invalid_responses_never_replace_the_current_catalog() {
    let initial = snapshot(&wire());
    let mut changed = wire();
    changed["models"][0]["version"] = serde_json::json!("4.0");
    let next = snapshot(&changed);
    let pointer = serde_json::to_vec(&next.pointer()).unwrap();
    let oversized_chunk = vec![b' '; POINTER_LIMIT + 1];
    let mut chunked = format!(
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n",
        oversized_chunk.len()
    )
    .into_bytes();
    chunked.extend_from_slice(&oversized_chunk);
    chunked.extend_from_slice(b"\r\n0\r\n\r\n");
    let cases = vec![
        vec![response("200 OK", b"invalid json")],
        vec![response("302 Redirect", b"")],
        vec![format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            POINTER_LIMIT + 1
        )
        .into_bytes()],
        vec![chunked],
        vec![
            response("200 OK", &pointer),
            response("200 OK", b"wrong snapshot"),
        ],
    ];
    for replies in cases {
        let directory = tempfile::tempdir().unwrap();
        let server = Server::new(replies);
        let client = server.client(directory.path());
        client.cache.commit(&initial).unwrap();
        let result = client.refresh().await.unwrap();
        assert_eq!(result.freshness, CatalogFreshness::Cached);
        assert!(result.refresh_error.is_some());
        assert_eq!(client.cached().unwrap().sha256(), initial.sha256());
    }
}

#[tokio::test]
async fn failed_publication_keeps_prior_pointer_and_reports_refresh_error() {
    let directory = tempfile::tempdir().unwrap();
    let first = snapshot(&wire());
    let mut value = wire();
    value["models"][0]["version"] = serde_json::json!("4.0");
    let second = snapshot(&value);
    let server = Server::new(vec![
        response("200 OK", &serde_json::to_vec(&second.pointer()).unwrap()),
        response("200 OK", second.as_bytes()),
    ]);
    let client = server.client(directory.path());
    client.cache.commit(&first).unwrap();
    let _lock = crate::filesystem::FileLock::try_lock(
        client.cache.root.lock_file("catalog.lock").unwrap(),
        true,
    )
    .unwrap();
    let result = client.refresh().await.unwrap();
    assert_eq!(result.freshness, CatalogFreshness::Cached);
    assert!(result.refresh_error.is_some());
    assert_eq!(result.snapshot.sha256(), first.sha256());
}

#[tokio::test]
async fn retryable_catalog_responses_keep_server_waits_separate_from_bad_catalogs() {
    let directory = tempfile::tempdir().unwrap();
    let server = Server::new(vec![
        b"HTTP/1.1 429 Busy\r\nRetry-After: 120\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            .to_vec(),
        response("404 Not found", b""),
        response("200 OK", b"invalid catalog"),
    ]);
    let client = server.client(directory.path());
    assert!(client.refresh().await.is_err());
    assert_eq!(client.retry_wait(), Some(Duration::from_secs(120)));
    assert!(client.refresh().await.is_err());
    assert_eq!(client.retry_wait(), None);
    assert!(client.refresh().await.is_err());
    assert_eq!(client.retry_wait(), None);
}
