use std::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
    time::Duration,
};

use tokio::{sync::mpsc, time::timeout};

use super::*;
use crate::{
    catalog::tests::{snapshot, wire, EN_RU, RU_EN},
    detect::LanguageDetector,
    engine::{EngineLoader, InferenceEngine},
    store::tests::install,
    Detection, DetectionPolicy, Language, ModelDescriptor, SkipReason, SourceLanguage,
};

struct Detector;

impl LanguageDetector for Detector {
    fn detect(&self, text: &str) -> Option<Detection> {
        assert_ne!(text, "worker-panic", "injected worker failure");
        (text != "unknown").then_some(Detection {
            language: Language::RUSSIAN,
            confidence: 0.9,
            reliable: text != "uncertain",
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Batch {
    pair: LanguagePair,
    texts: Vec<String>,
}

#[derive(Default)]
struct Gate {
    blocked: bool,
    permits: usize,
}

struct Control {
    events: mpsc::UnboundedSender<Batch>,
    gate: Mutex<Gate>,
    ready: Condvar,
    store: ModelStore,
    loads: AtomicUsize,
    calls: AtomicUsize,
    drops: AtomicUsize,
    active: AtomicUsize,
    peak_active: AtomicUsize,
    fail_load: AtomicBool,
}

impl Control {
    fn block(&self) {
        *self.gate.lock().unwrap() = Gate {
            blocked: true,
            permits: 0,
        };
    }

    fn release(&self) {
        self.gate.lock().unwrap().permits += 1;
        self.ready.notify_all();
    }

    fn open(&self) {
        self.gate.lock().unwrap().blocked = false;
        self.ready.notify_all();
    }
}

struct Loader(Arc<Control>);
struct Engine(LanguagePair, Arc<Control>);

impl EngineLoader for Loader {
    fn load(
        &self,
        descriptor: &ModelDescriptor,
        _: CoreOptions,
    ) -> Result<Arc<dyn InferenceEngine>, String> {
        self.0.loads.fetch_add(1, Ordering::SeqCst);
        if self.0.fail_load.swap(false, Ordering::SeqCst) {
            return Err("injected load failure".into());
        }
        Ok(Arc::new(Engine(descriptor.pair, self.0.clone())))
    }
}

impl InferenceEngine for Engine {
    fn source_token_count(&self, text: &str) -> usize {
        text.split_whitespace().count() + 1
    }

    fn translate_batch(&self, texts: &[&str]) -> Result<Vec<String>, String> {
        let active = self.1.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.1.peak_active.fetch_max(active, Ordering::SeqCst);
        self.1.calls.fetch_add(1, Ordering::SeqCst);
        let _ = self.1.events.send(Batch {
            pair: self.0,
            texts: texts.iter().map(|text| (*text).to_owned()).collect(),
        });
        let mut gate = self.1.gate.lock().unwrap();
        while gate.blocked && gate.permits == 0 {
            gate = self.1.ready.wait(gate).unwrap();
        }
        if gate.blocked {
            gate.permits -= 1;
        }
        drop(gate);
        self.1.active.fetch_sub(1, Ordering::SeqCst);
        assert!(!texts.contains(&"native-panic"), "injected native panic");
        if texts.contains(&"inference-error") {
            return Err("injected inference failure".into());
        }
        Ok(texts
            .iter()
            .map(|text| format!("translated:{text}"))
            .collect())
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        assert!(matches!(
            self.1.store.exclusive(self.0),
            Err(StoreError::Busy(_))
        ));
        self.1.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct Harness {
    control: Arc<Control>,
    events: mpsc::UnboundedReceiver<Batch>,
    directory: tempfile::TempDir,
}

impl Harness {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let store = ModelStore::open(directory.path()).unwrap();
        let catalog = snapshot(&wire());
        install(&store, &catalog, RU_EN);
        install(&store, &catalog, EN_RU);
        let (events, receiver) = mpsc::unbounded_channel();
        Self {
            control: Arc::new(Control {
                events,
                gate: Mutex::new(Gate::default()),
                ready: Condvar::new(),
                store,
                loads: AtomicUsize::new(0),
                calls: AtomicUsize::new(0),
                drops: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                peak_active: AtomicUsize::new(0),
                fail_load: AtomicBool::new(false),
            }),
            events: receiver,
            directory,
        }
    }

    fn manager(&self, options: WorkloadOptions, batch_size: usize) -> WorkloadManager {
        let core = TranslationCore::with_backends(
            CoreOptions {
                native_batch_size: batch_size,
                ..CoreOptions::default()
            },
            Arc::new(Detector),
            Arc::new(Loader(self.control.clone())),
        );
        WorkloadManager::start(self.control.store.clone(), core, options, batch_size).unwrap()
    }

    async fn batch(&mut self, pair: LanguagePair, texts: &[&str]) {
        let event = timeout(Duration::from_secs(10), self.events.recv())
            .await
            .expect("worker must reach inference")
            .expect("event sender stays alive");
        assert_eq!(
            event,
            Batch {
                pair,
                texts: texts.iter().map(|text| (*text).to_owned()).collect(),
            }
        );
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        self.control.open();
    }
}

fn options(capacity: usize, overflow_policy: OverflowPolicy) -> WorkloadOptions {
    WorkloadOptions {
        capacity,
        overflow_policy,
        ..WorkloadOptions::default()
    }
}

fn request(text: &str, pair: LanguagePair) -> TranslationRequest {
    let mut request = TranslationRequest::new(text, pair.target);
    request.source = SourceLanguage::Explicit(pair.source);
    request
}

fn assert_pending<F: Future>(future: Pin<&mut F>) {
    assert!(matches!(
        future.poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
}

async fn result(job: TranslationJob) -> WorkloadResult {
    timeout(Duration::from_secs(10), job.result())
        .await
        .expect("every admitted caller must resolve")
}

async fn translated(job: TranslationJob) -> Translation {
    match result(job).await.unwrap() {
        TranslationOutcome::Translated(translation) => translation,
        other => panic!("expected translation, got {other:?}"),
    }
}

#[test]
fn construction_needs_no_runtime_and_validates_configuration() {
    let harness = Harness::new();
    let manager = WorkloadManager::new(
        harness.control.store.clone(),
        CoreOptions::default(),
        WorkloadOptions::default(),
    )
    .unwrap();
    assert_eq!(manager.snapshot().running_requests, 0);
    assert!(manager.snapshot().resident_models.is_empty());
    for options in [
        WorkloadOptions {
            capacity: 0,
            ..WorkloadOptions::default()
        },
        WorkloadOptions {
            cache_capacity: 0,
            ..WorkloadOptions::default()
        },
    ] {
        assert!(matches!(
            WorkloadManager::new(
                harness.control.store.clone(),
                CoreOptions::default(),
                options
            ),
            Err(WorkloadError::InvalidOptions(_))
        ));
    }
    assert!(matches!(
        WorkloadManager::new(
            harness.control.store.clone(),
            CoreOptions {
                threads: 0,
                ..CoreOptions::default()
            },
            WorkloadOptions::default()
        ),
        Err(WorkloadError::Core(TranslationError::InvalidOptions(_)))
    ));
    assert_eq!(WorkloadOptions::default().capacity, 256);
    assert_eq!(WorkloadOptions::default().cache_capacity, 1024);
    assert_eq!(
        WorkloadOptions::default().overflow_policy,
        OverflowPolicy::RejectNew
    );
}

#[tokio::test]
async fn reject_new_counts_each_active_and_queued_subscriber() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(options(3, OverflowPolicy::RejectNew), 16);
    let first = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let duplicate = manager.submit(request("active", RU_EN)).await.unwrap();
    let queued = manager.submit(request("queued", RU_EN)).await.unwrap();
    assert_eq!(manager.snapshot().running_requests, 2);
    assert_eq!(manager.snapshot().queued_requests, 1);
    assert!(matches!(
        manager.submit(request("overflow", RU_EN)).await,
        Err(WorkloadError::Capacity)
    ));
    drop(duplicate);
    let replacement = manager.submit(request("queued", RU_EN)).await.unwrap();
    assert_eq!(manager.snapshot().queued_requests, 2);
    harness.control.open();
    translated(first).await;
    translated(queued).await;
    translated(replacement).await;
    manager.shutdown().await.unwrap();
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn drop_oldest_evicts_callers_without_discarding_other_shared_interest() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(options(3, OverflowPolicy::DropOldest), 16);
    let first = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let oldest = manager.submit(request("shared", RU_EN)).await.unwrap();
    let survivor = manager.submit(request("shared", RU_EN)).await.unwrap();
    let newest = manager.submit(request("newest", RU_EN)).await.unwrap();
    assert!(matches!(result(oldest).await, Err(WorkloadError::Evicted)));
    assert_eq!(manager.snapshot().queued_requests, 2);
    harness.control.open();
    translated(first).await;
    translated(survivor).await;
    translated(newest).await;
    harness.batch(RU_EN, &["shared", "newest"]).await;
    manager.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_submitters_cannot_exceed_capacity_or_duplicate_shared_inference() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = Arc::new(harness.manager(options(17, OverflowPolicy::RejectNew), 16));
    let active = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(65));
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..64 {
        let manager = manager.clone();
        let barrier = barrier.clone();
        tasks.spawn(async move {
            barrier.wait().await;
            manager.submit(request("shared", RU_EN)).await
        });
    }
    barrier.wait().await;
    let mut admitted = Vec::new();
    let mut rejected = 0;
    while let Some(result) = tasks.join_next().await {
        match result.unwrap() {
            Ok(job) => admitted.push(job),
            Err(WorkloadError::Capacity) => rejected += 1,
            Err(error) => panic!("unexpected admission error: {error:?}"),
        }
    }
    assert_eq!(admitted.len(), 16);
    assert_eq!(rejected, 48);
    assert_eq!(manager.snapshot().queued_requests, 16);
    assert_eq!(manager.snapshot().running_requests, 1);
    harness.control.open();
    translated(active).await;
    for job in admitted {
        translated(job).await;
    }
    harness.batch(RU_EN, &["shared"]).await;
    manager.shutdown().await.unwrap();
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 2);
    assert_eq!(harness.control.peak_active.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn drop_oldest_rejects_when_every_slot_is_active() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(options(2, OverflowPolicy::DropOldest), 1);
    let first = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let shared = manager.submit(request("active", RU_EN)).await.unwrap();
    assert!(matches!(
        manager.submit(request("new", RU_EN)).await,
        Err(WorkloadError::Capacity)
    ));
    harness.control.open();
    translated(first).await;
    translated(shared).await;
    manager.shutdown().await.unwrap();
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn wait_submission_can_be_cancelled_and_wakes_when_a_slot_is_freed() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(options(1, OverflowPolicy::Wait), 1);
    let active = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let mut abandoned = Box::pin(manager.submit(request("abandoned", RU_EN)));
    assert_pending(abandoned.as_mut());
    drop(abandoned);
    let mut waiting = Box::pin(manager.submit(request("next", RU_EN)));
    assert_pending(waiting.as_mut());
    active.cancel();
    assert!(matches!(
        result(active).await,
        Err(WorkloadError::Cancelled)
    ));
    let next = timeout(Duration::from_secs(10), waiting)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(manager.snapshot().queued_requests, 1);
    harness.control.open();
    translated(next).await;
    harness.batch(RU_EN, &["next"]).await;
    manager.shutdown().await.unwrap();
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn fifo_batches_only_consecutive_directions_and_bounds_native_concurrency() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(WorkloadOptions::default(), 2);
    let first = manager.submit(request("blocker", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["blocker"]).await;
    let mut jobs = vec![first];
    for (text, pair) in [
        ("a", RU_EN),
        ("b", RU_EN),
        ("c", RU_EN),
        ("d", EN_RU),
        ("e", RU_EN),
    ] {
        jobs.push(manager.submit(request(text, pair)).await.unwrap());
    }
    for (pair, texts) in [
        (RU_EN, vec!["a", "b"]),
        (RU_EN, vec!["c"]),
        (EN_RU, vec!["d"]),
        (RU_EN, vec!["e"]),
    ] {
        harness.control.release();
        harness.batch(pair, &texts).await;
    }
    harness.control.open();
    for job in jobs {
        translated(job).await;
    }
    manager.shutdown().await.unwrap();
    assert_eq!(harness.control.peak_active.load(Ordering::SeqCst), 1);
    assert_eq!(harness.control.loads.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn batch_size_one_selects_individual_execution() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(WorkloadOptions::default(), 1);
    let first = manager.submit(request("first", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["first"]).await;
    let second = manager.submit(request("second", RU_EN)).await.unwrap();
    let third = manager.submit(request("third", RU_EN)).await.unwrap();
    harness.control.release();
    harness.batch(RU_EN, &["second"]).await;
    harness.control.release();
    harness.batch(RU_EN, &["third"]).await;
    harness.control.open();
    for job in [first, second, third] {
        translated(job).await;
    }
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_removes_last_queued_interest_but_keeps_shared_work() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(WorkloadOptions::default(), 16);
    let active = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let cancelled = manager.submit(request("never-run", RU_EN)).await.unwrap();
    cancelled.cancel();
    assert!(matches!(
        result(cancelled).await,
        Err(WorkloadError::Cancelled)
    ));
    let shared = manager.submit(request("survivor", RU_EN)).await.unwrap();
    let survivor = manager.submit(request("survivor", RU_EN)).await.unwrap();
    drop(shared);
    assert_eq!(manager.snapshot().queued_requests, 1);
    harness.control.open();
    translated(active).await;
    translated(survivor).await;
    harness.batch(RU_EN, &["survivor"]).await;
    manager.shutdown().await.unwrap();
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn dropping_result_future_cancels_its_caller() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(WorkloadOptions::default(), 1);
    let job = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let mut result = Box::pin(job.result());
    assert_pending(result.as_mut());
    drop(result);
    assert_eq!(manager.snapshot().running_requests, 0);
    let replacement = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.control.open();
    translated(replacement).await;
    manager.shutdown().await.unwrap();
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn lru_hits_bypass_full_capacity_and_completed_cancellation_is_a_noop() {
    let mut harness = Harness::new();
    let manager = harness.manager(
        WorkloadOptions {
            capacity: 1,
            cache_capacity: 2,
            ..WorkloadOptions::default()
        },
        1,
    );
    for text in ["a", "b", "a", "c", "b"] {
        translated(manager.submit(request(text, RU_EN)).await.unwrap()).await;
    }
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 4);
    assert_eq!(manager.snapshot().cached_results, 2);
    harness.control.block();
    let active = manager.submit(request("active", RU_EN)).await.unwrap();
    for text in ["a", "b", "c", "b", "active"] {
        harness.batch(RU_EN, &[text]).await;
    }
    let cached = manager.submit(request("b", RU_EN)).await.unwrap();
    cached.cancel();
    assert_eq!(translated(cached).await.text, "translated:b");
    assert_eq!(manager.snapshot().running_requests, 1);
    harness.control.open();
    translated(active).await;
    manager.shutdown().await.unwrap();
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn cache_matches_exact_request_and_preserves_detection_metadata() {
    let harness = Harness::new();
    let manager = harness.manager(WorkloadOptions::default(), 16);
    let original = request("plain", RU_EN);
    let mut best = original.clone();
    best.detection_policy = DetectionPolicy::BestGuess;
    let auto = TranslationRequest::new("plain", Language::ENGLISH);
    let spaced = request(" plain ", RU_EN);
    for request in [original.clone(), best, auto, spaced] {
        let expected_detection = request.source == SourceLanguage::Auto;
        let translation = translated(manager.submit(request).await.unwrap()).await;
        assert_eq!(translation.detection.is_some(), expected_detection);
    }
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 4);
    let mut changed_target = original.clone();
    changed_target.target = Language::RUSSIAN;
    assert!(matches!(
        result(manager.submit(changed_target).await.unwrap()).await,
        Ok(TranslationOutcome::Skipped(SkipReason::SameAsTarget(_)))
    ));
    let mut changed_source = original.clone();
    changed_source.source = SourceLanguage::Explicit(Language::ENGLISH);
    assert!(matches!(
        result(manager.submit(changed_source).await.unwrap()).await,
        Ok(TranslationOutcome::Skipped(SkipReason::SameAsTarget(_)))
    ));
    {
        let mut state = manager.shared.lock();
        let model_id = state.residents.insert(RU_EN, "different-revision".into());
        assert!(state.cached(&original).is_none());
        state.residents.insert(RU_EN, model_id.unwrap());
    }
    translated(manager.submit(original).await.unwrap()).await;
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 4);
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn expected_skips_do_not_load_models_or_enter_cache() {
    let harness = Harness::new();
    let manager = harness.manager(WorkloadOptions::default(), 16);
    let mut same = request("same", RU_EN);
    same.target = Language::RUSSIAN;
    let unsupported = request(
        "bonjour",
        LanguagePair {
            source: Language::new("fra").unwrap(),
            target: Language::ENGLISH,
        },
    );
    for (request, expected) in [
        (request(" ", RU_EN), SkipReason::BlankInput),
        (
            TranslationRequest::new("unknown", Language::ENGLISH),
            SkipReason::LanguageUndetermined,
        ),
        (same, SkipReason::SameAsTarget(Language::RUSSIAN)),
        (
            unsupported.clone(),
            SkipReason::UnsupportedPair(LanguagePair {
                source: Language::new("fra").unwrap(),
                target: Language::ENGLISH,
            }),
        ),
    ] {
        assert_eq!(
            result(manager.submit(request).await.unwrap())
                .await
                .unwrap(),
            TranslationOutcome::Skipped(expected)
        );
    }
    assert!(matches!(
        result(
            manager
                .submit(TranslationRequest::new("uncertain", Language::ENGLISH))
                .await
                .unwrap()
        )
        .await,
        Ok(TranslationOutcome::Skipped(
            SkipReason::UnreliableDetection(_)
        ))
    ));
    assert_eq!(harness.control.loads.load(Ordering::SeqCst), 0);
    assert_eq!(manager.snapshot().cached_results, 0);
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_model_skip_is_not_cached_and_installation_remains_explicit() {
    let harness = Harness::new();
    harness.control.store.remove(RU_EN).unwrap();
    let manager = harness.manager(WorkloadOptions::default(), 16);
    assert_eq!(
        result(manager.submit(request("text", RU_EN)).await.unwrap())
            .await
            .unwrap(),
        TranslationOutcome::Skipped(SkipReason::ModelNotInstalled(RU_EN))
    );
    assert_eq!(harness.control.loads.load(Ordering::SeqCst), 0);
    assert_eq!(manager.snapshot().cached_results, 0);
    assert!(!harness.directory.path().join(".downloads").exists());
    install(&harness.control.store, &snapshot(&wire()), RU_EN);
    translated(manager.submit(request("text", RU_EN)).await.unwrap()).await;
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn store_failures_preserve_typed_causes_and_retry_after_repair() {
    let harness = Harness::new();
    let manager = harness.manager(WorkloadOptions::default(), 16);
    let guard = harness.control.store.exclusive(RU_EN).unwrap();
    assert!(matches!(
        result(manager.submit(request("text", RU_EN)).await.unwrap()).await,
        Err(WorkloadError::Store(error)) if matches!(*error, StoreError::Busy(RU_EN))
    ));
    std::fs::write(guard.directory().join("weights.bin"), b"WRONG!!").unwrap();
    drop(guard);
    assert!(matches!(
        result(manager.submit(request("text", RU_EN)).await.unwrap()).await,
        Err(WorkloadError::Store(error)) if matches!(*error,
            StoreError::Verification(TranslationError::ChecksumMismatch { .. }))
    ));
    assert_eq!(harness.control.loads.load(Ordering::SeqCst), 0);
    assert_eq!(manager.snapshot().cached_results, 0);
    let guard = harness.control.store.exclusive(RU_EN).unwrap();
    std::fs::write(guard.directory().join("weights.bin"), b"weights").unwrap();
    drop(guard);
    translated(manager.submit(request("text", RU_EN)).await.unwrap()).await;
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn load_and_inference_errors_are_not_cached_and_do_not_stop_worker() {
    let harness = Harness::new();
    harness.control.fail_load.store(true, Ordering::SeqCst);
    let manager = harness.manager(WorkloadOptions::default(), 16);
    assert!(matches!(
        result(manager.submit(request("text", RU_EN)).await.unwrap()).await,
        Err(WorkloadError::Core(TranslationError::ModelLoad { .. }))
    ));
    assert!(manager.snapshot().resident_models.is_empty());
    assert!(harness.control.store.exclusive(RU_EN).is_ok());
    for _ in 0..2 {
        assert!(matches!(
            result(
                manager
                    .submit(request("inference-error", RU_EN))
                    .await
                    .unwrap()
            )
            .await,
            Err(WorkloadError::Core(TranslationError::Inference { .. }))
        ));
    }
    assert!(matches!(
        result(
            manager
                .submit(request("native-panic", RU_EN))
                .await
                .unwrap()
        )
        .await,
        Err(WorkloadError::Core(TranslationError::Inference { .. }))
    ));
    let oversized = "word ".repeat(128);
    assert!(matches!(
        result(manager.submit(request(&oversized, RU_EN)).await.unwrap()).await,
        Err(WorkloadError::Core(TranslationError::InputTooLong { .. }))
    ));
    assert_eq!(manager.snapshot().cached_results, 0);
    translated(manager.submit(request("text", RU_EN)).await.unwrap()).await;
    manager.shutdown().await.unwrap();
    assert_eq!(harness.control.loads.load(Ordering::SeqCst), 2);
    assert_eq!(harness.control.calls.load(Ordering::SeqCst), 4);
}

#[tokio::test]
async fn shutdown_cancels_queued_and_waiting_callers_but_finishes_active_work() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(options(2, OverflowPolicy::Wait), 16);
    let active = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let queued = manager.submit(request("queued", RU_EN)).await.unwrap();
    let mut waiting = Box::pin(manager.submit(request("waiting", RU_EN)));
    assert_pending(waiting.as_mut());
    let mut shutdown = Box::pin(manager.shutdown());
    assert_pending(shutdown.as_mut());
    assert!(matches!(result(queued).await, Err(WorkloadError::Shutdown)));
    assert!(matches!(waiting.await, Err(WorkloadError::Shutdown)));
    assert_eq!(manager.snapshot().queued_requests, 0);
    assert!(matches!(
        manager.submit(request("closed", RU_EN)).await,
        Err(WorkloadError::Shutdown)
    ));
    assert!(matches!(
        harness.control.store.exclusive(RU_EN),
        Err(StoreError::Busy(_))
    ));
    harness.control.open();
    translated(active).await;
    timeout(Duration::from_secs(10), shutdown)
        .await
        .unwrap()
        .unwrap();
    manager.shutdown().await.unwrap();
    assert!(harness.control.store.exclusive(RU_EN).is_ok());
    assert_eq!(harness.control.drops.load(Ordering::SeqCst), 1);
    assert_eq!(manager.snapshot().cached_results, 0);
    assert!(manager.snapshot().resident_models.is_empty());
}

#[tokio::test]
async fn dropping_shutdown_future_does_not_reopen_admission() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(WorkloadOptions::default(), 16);
    let active = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let mut shutdown = Box::pin(manager.shutdown());
    assert_pending(shutdown.as_mut());
    drop(shutdown);
    assert!(matches!(
        manager.submit(request("closed", RU_EN)).await,
        Err(WorkloadError::Shutdown)
    ));
    harness.control.open();
    translated(active).await;
    manager.shutdown().await.unwrap();
}

#[tokio::test]
async fn manager_drop_signals_shutdown_without_waiting_for_native_work() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(WorkloadOptions::default(), 16);
    let active = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let queued = manager.submit(request("queued", RU_EN)).await.unwrap();
    let shared = manager.shared.clone();
    let mut stopped = shared.stopped.subscribe();
    drop(manager);
    assert!(matches!(result(queued).await, Err(WorkloadError::Shutdown)));
    assert!(stopped.borrow().is_none());
    harness.control.open();
    translated(active).await;
    timeout(
        Duration::from_secs(10),
        stopped.wait_for(|value| value.is_some()),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(harness.control.store.exclusive(RU_EN).is_ok());
}

#[tokio::test]
async fn worker_failure_resolves_all_handles_and_wakes_blocked_submissions() {
    let mut harness = Harness::new();
    harness.control.block();
    let manager = harness.manager(options(3, OverflowPolicy::Wait), 16);
    let active = manager.submit(request("active", RU_EN)).await.unwrap();
    harness.batch(RU_EN, &["active"]).await;
    let panic = manager
        .submit(TranslationRequest::new("worker-panic", Language::ENGLISH))
        .await
        .unwrap();
    let queued = manager.submit(request("queued", RU_EN)).await.unwrap();
    let mut waiting = Box::pin(manager.submit(request("waiting", RU_EN)));
    assert_pending(waiting.as_mut());
    harness.control.open();
    translated(active).await;
    for job in [panic, queued] {
        assert!(matches!(
            result(job).await,
            Err(WorkloadError::WorkerFailed)
        ));
    }
    assert!(matches!(waiting.await, Err(WorkloadError::WorkerFailed)));
    assert!(matches!(
        manager.shutdown().await,
        Err(WorkloadError::WorkerFailed)
    ));
    assert!(harness.control.store.exclusive(RU_EN).is_ok());
    assert_eq!(manager.snapshot().queued_requests, 0);
    assert_eq!(manager.snapshot().running_requests, 0);
}

#[tokio::test]
async fn shutdown_releases_resident_revisions_before_replacement() {
    let harness = Harness::new();
    let manager = harness.manager(WorkloadOptions::default(), 16);
    let first = translated(manager.submit(request("text", RU_EN)).await.unwrap()).await;
    let resident = manager.snapshot().resident_models;
    assert_eq!(resident.len(), 1);
    assert_eq!(resident[0].model_id, first.model_id);
    assert!(matches!(
        harness.control.store.remove(RU_EN),
        Err(StoreError::Busy(_))
    ));
    manager.shutdown().await.unwrap();
    harness.control.store.remove(RU_EN).unwrap();
    let mut next = wire();
    next["models"][0]["version"] = serde_json::json!("4.0");
    install(&harness.control.store, &snapshot(&next), RU_EN);
    let next_manager = harness.manager(WorkloadOptions::default(), 16);
    let next = translated(next_manager.submit(request("text", RU_EN)).await.unwrap()).await;
    assert_ne!(next.model_id, first.model_id);
    next_manager.shutdown().await.unwrap();
    assert_eq!(harness.control.loads.load(Ordering::SeqCst), 2);
}
