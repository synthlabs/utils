use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Condvar, Mutex, MutexGuard, Weak},
    thread,
};

use tokio::sync::{oneshot, watch, Notify};

use crate::{
    CoreOptions, LanguagePair, ModelStore, StoreError, Translation, TranslationCore,
    TranslationError, TranslationOutcome, TranslationRequest,
};

mod worker;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverflowPolicy {
    #[default]
    RejectNew,
    DropOldest,
    Wait,
}

#[derive(Clone, Copy, Debug)]
pub struct WorkloadOptions {
    /// Accepted unfinished callers, including callers sharing active work.
    pub capacity: usize,
    pub cache_capacity: usize,
    pub overflow_policy: OverflowPolicy,
}

impl Default for WorkloadOptions {
    fn default() -> Self {
        Self {
            capacity: 256,
            cache_capacity: 1024,
            overflow_policy: OverflowPolicy::RejectNew,
        }
    }
}

#[derive(Clone, Debug, thiserror::Error)]
pub enum WorkloadError {
    #[error("invalid workload configuration: {0}")]
    InvalidOptions(String),
    #[error("translation workload is at capacity")]
    Capacity,
    #[error("queued request was evicted")]
    Evicted,
    #[error("translation request was cancelled")]
    Cancelled,
    #[error("translation manager is shutting down")]
    Shutdown,
    #[error("translation worker stopped unexpectedly")]
    WorkerFailed,
    #[error("cannot start translation worker: {0}")]
    WorkerStart(#[source] Arc<std::io::Error>),
    #[error(transparent)]
    Core(#[from] TranslationError),
    #[error(transparent)]
    Store(#[from] Arc<StoreError>),
}

pub type WorkloadResult = Result<TranslationOutcome, WorkloadError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResidentModel {
    pub pair: LanguagePair,
    pub model_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkloadSnapshot {
    pub queued_requests: usize,
    pub running_requests: usize,
    pub cached_results: usize,
    pub resident_models: Vec<ResidentModel>,
}

/// Share with `Arc` when several callers submit concurrently.
pub struct WorkloadManager {
    shared: Arc<Shared>,
}

impl WorkloadManager {
    /// Starts one dedicated worker. Construction performs no model or network I/O.
    pub fn new(
        store: ModelStore,
        core_options: CoreOptions,
        options: WorkloadOptions,
    ) -> Result<Self, WorkloadError> {
        let core = TranslationCore::new(core_options)?;
        Self::start(store, core, options, core_options.native_batch_size)
    }

    fn start(
        store: ModelStore,
        core: TranslationCore,
        options: WorkloadOptions,
        batch_size: usize,
    ) -> Result<Self, WorkloadError> {
        if options.capacity == 0 || options.cache_capacity == 0 {
            return Err(WorkloadError::InvalidOptions(
                "request and cache capacities must be positive".into(),
            ));
        }
        let shared = Arc::new(Shared {
            options,
            state: Mutex::new(State::default()),
            ready: Condvar::new(),
            capacity: Notify::new(),
            stopped: watch::channel(None).0,
        });
        let worker_shared = shared.clone();
        thread::Builder::new()
            .name("synth-translate".into())
            .spawn(move || {
                let outcome = catch_unwind(AssertUnwindSafe(|| {
                    worker::run(store, core, &worker_shared, batch_size);
                }));
                // run owns the core: engines and leases drop before completion is signalled.
                worker_shared.finish(outcome.is_err());
            })
            .map_err(|error| WorkloadError::WorkerStart(Arc::new(error)))?;
        Ok(Self { shared })
    }

    /// Under Wait, dropping this future before admission leaves no queued request.
    pub async fn submit(
        &self,
        request: TranslationRequest,
    ) -> Result<TranslationJob, WorkloadError> {
        let request = Arc::new(request);
        loop {
            let available = self.shared.capacity.notified();
            tokio::pin!(available);
            available.as_mut().enable();
            {
                let mut state = self.shared.lock();
                if !state.accepting {
                    return Err(state.closed_error());
                }
                if let Some(translation) = state.cached(&request) {
                    return Ok(TranslationJob::completed(Ok(
                        TranslationOutcome::Translated(translation),
                    )));
                }
                if state.callers.len() >= self.shared.options.capacity {
                    match self.shared.options.overflow_policy {
                        OverflowPolicy::RejectNew => return Err(WorkloadError::Capacity),
                        OverflowPolicy::DropOldest => {
                            let oldest = state.callers.iter().find_map(|(id, caller)| {
                                (state.works[&caller.work_id].phase == Phase::Queued).then_some(*id)
                            });
                            let Some(oldest) = oldest else {
                                return Err(WorkloadError::Capacity);
                            };
                            state.cancel(oldest, WorkloadError::Evicted);
                        }
                        OverflowPolicy::Wait => {}
                    }
                }
                if state.callers.len() < self.shared.options.capacity {
                    let job = state.admit(request, &self.shared);
                    self.shared.ready.notify_one();
                    return Ok(job);
                }
            }
            available.await;
        }
    }

    pub fn snapshot(&self) -> WorkloadSnapshot {
        let state = self.shared.lock();
        let running_requests = state
            .callers
            .values()
            .filter(|caller| state.works[&caller.work_id].phase == Phase::Running)
            .count();
        WorkloadSnapshot {
            queued_requests: state.callers.len() - running_requests,
            running_requests,
            cached_results: state.cache.len(),
            resident_models: state
                .residents
                .iter()
                .map(|(pair, model_id)| ResidentModel {
                    pair: *pair,
                    model_id: model_id.clone(),
                })
                .collect(),
        }
    }

    /// Idempotently close admission and await release of worker-owned resources.
    /// Cancelling this future does not reverse shutdown.
    pub async fn shutdown(&self) -> Result<(), WorkloadError> {
        self.shared.close();
        let mut stopped = self.shared.stopped.subscribe();
        loop {
            if let Some(result) = stopped.borrow().clone() {
                return result;
            }
            stopped
                .changed()
                .await
                .map_err(|_| WorkloadError::WorkerFailed)?;
        }
    }
}

impl Drop for WorkloadManager {
    fn drop(&mut self) {
        self.shared.close();
    }
}

#[must_use = "dropping a job cancels that caller's interest"]
pub struct TranslationJob {
    id: Option<u64>,
    shared: Weak<Shared>,
    receiver: Option<oneshot::Receiver<WorkloadResult>>,
}

impl TranslationJob {
    fn completed(result: WorkloadResult) -> Self {
        let (sender, receiver) = oneshot::channel();
        let _ = sender.send(result);
        Self {
            id: None,
            shared: Weak::new(),
            receiver: Some(receiver),
        }
    }

    /// Dropping the result future also drops the job and cancels its interest.
    pub async fn result(mut self) -> WorkloadResult {
        let result = self
            .receiver
            .take()
            .expect("a job owns one result receiver")
            .await
            .unwrap_or(Err(WorkloadError::WorkerFailed));
        self.id = None;
        result
    }

    /// Terminal requests are unaffected. Active native work is never interrupted.
    pub fn cancel(&self) {
        if let (Some(id), Some(shared)) = (self.id, self.shared.upgrade()) {
            if shared.lock().cancel(id, WorkloadError::Cancelled) {
                shared.capacity.notify_waiters();
            }
        }
    }
}

impl Drop for TranslationJob {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct Shared {
    options: WorkloadOptions,
    state: Mutex<State>,
    ready: Condvar,
    capacity: Notify,
    stopped: watch::Sender<Option<Result<(), WorkloadError>>>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn close(&self) {
        let mut state = self.lock();
        state.accepting = false;
        let queued: Vec<_> = state
            .callers
            .iter()
            .filter_map(|(id, caller)| {
                (state.works[&caller.work_id].phase == Phase::Queued).then_some(*id)
            })
            .collect();
        for id in queued {
            state.cancel(id, WorkloadError::Shutdown);
        }
        drop(state);
        self.ready.notify_all();
        self.capacity.notify_waiters();
    }

    fn complete(&self, work_id: u64, result: WorkloadResult) {
        let mut state = self.lock();
        let Some(work) = state.works.remove(&work_id) else {
            return;
        };
        state.by_request.remove(&work.request);
        if state.accepting {
            if let Ok(TranslationOutcome::Translated(translation)) = &result {
                state.cache.push_back(CacheEntry {
                    request: work.request,
                    translation: translation.clone(),
                });
                if state.cache.len() > self.options.cache_capacity {
                    state.cache.pop_front();
                }
            }
        }
        for id in work.callers {
            if let Some(caller) = state.callers.remove(&id) {
                let _ = caller.sender.send(result.clone());
            }
        }
        drop(state);
        self.capacity.notify_waiters();
    }

    fn finish(&self, failed: bool) {
        let mut state = self.lock();
        state.accepting = false;
        state.failed = failed;
        let error = state.closed_error();
        for (_, caller) in std::mem::take(&mut state.callers) {
            let _ = caller.sender.send(Err(error.clone()));
        }
        state.works.clear();
        state.queue.clear();
        state.by_request.clear();
        state.cache.clear();
        state.residents.clear();
        drop(state);
        self.capacity.notify_waiters();
        self.stopped.send_replace(Some(if failed {
            Err(WorkloadError::WorkerFailed)
        } else {
            Ok(())
        }));
    }
}

struct State {
    accepting: bool,
    failed: bool,
    next_caller: u64,
    next_work: u64,
    callers: BTreeMap<u64, Caller>,
    works: BTreeMap<u64, Work>,
    by_request: HashMap<Arc<TranslationRequest>, u64>,
    queue: VecDeque<u64>,
    cache: VecDeque<CacheEntry>,
    residents: BTreeMap<LanguagePair, String>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            accepting: true,
            failed: false,
            next_caller: 0,
            next_work: 0,
            callers: BTreeMap::new(),
            works: BTreeMap::new(),
            by_request: HashMap::new(),
            queue: VecDeque::new(),
            cache: VecDeque::new(),
            residents: BTreeMap::new(),
        }
    }
}

impl State {
    fn closed_error(&self) -> WorkloadError {
        if self.failed {
            WorkloadError::WorkerFailed
        } else {
            WorkloadError::Shutdown
        }
    }

    fn cached(&mut self, request: &TranslationRequest) -> Option<Translation> {
        let position = self.cache.iter().position(|entry| {
            entry.request.as_ref() == request
                && self.residents.get(&LanguagePair {
                    source: entry.translation.source,
                    target: entry.translation.target,
                }) == Some(&entry.translation.model_id)
        })?;
        let entry = self.cache.remove(position).expect("cache position exists");
        let translation = entry.translation.clone();
        self.cache.push_back(entry);
        Some(translation)
    }

    fn admit(&mut self, request: Arc<TranslationRequest>, shared: &Arc<Shared>) -> TranslationJob {
        let id = self.next_caller;
        self.next_caller += 1;
        let work_id = if let Some(work_id) = self.by_request.get(&request) {
            *work_id
        } else {
            let work_id = self.next_work;
            self.next_work += 1;
            self.works.insert(
                work_id,
                Work {
                    request: request.clone(),
                    phase: Phase::Queued,
                    callers: BTreeSet::new(),
                },
            );
            self.by_request.insert(request, work_id);
            self.queue.push_back(work_id);
            work_id
        };
        self.works.get_mut(&work_id).unwrap().callers.insert(id);
        let (sender, receiver) = oneshot::channel();
        self.callers.insert(id, Caller { work_id, sender });
        TranslationJob {
            id: Some(id),
            shared: Arc::downgrade(shared),
            receiver: Some(receiver),
        }
    }

    fn cancel(&mut self, id: u64, error: WorkloadError) -> bool {
        let Some(caller) = self.callers.remove(&id) else {
            return false;
        };
        let work = self.works.get_mut(&caller.work_id).unwrap();
        work.callers.remove(&id);
        if work.callers.is_empty() && work.phase == Phase::Queued {
            let work = self.works.remove(&caller.work_id).unwrap();
            self.by_request.remove(&work.request);
            self.queue.retain(|work_id| *work_id != caller.work_id);
        }
        let _ = caller.sender.send(Err(error));
        true
    }

    fn take_front(&mut self) -> Option<(u64, Arc<TranslationRequest>)> {
        let id = self.queue.pop_front()?;
        let work = self.works.get_mut(&id).unwrap();
        work.phase = Phase::Running;
        Some((id, work.request.clone()))
    }
}

struct Caller {
    work_id: u64,
    sender: oneshot::Sender<WorkloadResult>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Queued,
    Running,
}

struct Work {
    request: Arc<TranslationRequest>,
    phase: Phase,
    callers: BTreeSet<u64>,
}

struct CacheEntry {
    request: Arc<TranslationRequest>,
    translation: Translation,
}

#[cfg(test)]
mod tests;
