use super::*;
use crate::{catalog::supported_pair, Detection, LoadMode, SkipReason, SourceLanguage};

struct Prepared {
    id: u64,
    request: Arc<TranslationRequest>,
    detection: Option<Detection>,
}

pub(super) fn run(
    store: ModelStore,
    mut core: TranslationCore,
    shared: &Shared,
    batch_size: usize,
) {
    loop {
        let next = {
            let mut state = shared.lock();
            while state.queue.is_empty() && state.accepting {
                state = shared
                    .ready
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
            if !state.accepting {
                return;
            }
            state.take_front()
        };
        let Some((id, request)) = next else {
            continue;
        };
        let (pair, detection) = match core.resolve(&request) {
            Ok(prepared) => prepared,
            Err(reason) => {
                shared.complete(id, Ok(TranslationOutcome::Skipped(reason)));
                continue;
            }
        };
        if !supported_pair(pair) {
            shared.complete(
                id,
                Ok(TranslationOutcome::Skipped(SkipReason::UnsupportedPair(
                    pair,
                ))),
            );
            continue;
        }
        match ensure_model(&store, &mut core, shared, pair) {
            Ok(None) => {}
            Ok(Some(reason)) => {
                shared.complete(id, Ok(TranslationOutcome::Skipped(reason)));
                continue;
            }
            Err(error) => {
                shared.complete(id, Err(error));
                continue;
            }
        }
        let mut batch = vec![Prepared {
            id,
            request,
            detection,
        }];
        while batch.len() < batch_size {
            let candidate = {
                let state = shared.lock();
                state
                    .queue
                    .front()
                    .map(|id| (*id, state.works[id].request.clone()))
            };
            let Some((id, request)) = candidate else {
                break;
            };
            let Ok((next_pair, detection)) = core.resolve(&request) else {
                break;
            };
            if next_pair != pair {
                break;
            }
            let mut state = shared.lock();
            // Cancellation or eviction can replace the front during preflight.
            if state.queue.front() != Some(&id) {
                continue;
            }
            state.take_front();
            batch.push(Prepared {
                id,
                request,
                detection,
            });
        }
        let requests: Vec<_> = batch
            .iter()
            .map(|item| {
                let mut request = (*item.request).clone();
                request.source = SourceLanguage::Explicit(pair.source);
                request
            })
            .collect();
        for (item, result) in batch.into_iter().zip(core.translate_batch(&requests)) {
            let result = result
                .map(|mut outcome| {
                    if let TranslationOutcome::Translated(translation) = &mut outcome {
                        translation.detection = item.detection;
                    }
                    outcome
                })
                .map_err(WorkloadError::Core);
            shared.complete(item.id, result);
        }
    }
}

fn ensure_model(
    store: &ModelStore,
    core: &mut TranslationCore,
    shared: &Shared,
    pair: LanguagePair,
) -> Result<Option<SkipReason>, WorkloadError> {
    if shared.lock().residents.contains_key(&pair) {
        return Ok(None);
    }
    let lease = match store.acquire(pair) {
        Ok(lease) => lease,
        Err(StoreError::NotInstalled(_)) => {
            return Ok(Some(SkipReason::ModelNotInstalled(pair)));
        }
        Err(error) => return Err(WorkloadError::Store(Arc::new(error))),
    };
    let model_id = lease.model().id().to_owned();
    core.register_stored_model(lease, LoadMode::Eager)?;
    shared.lock().residents.insert(pair, model_id);
    Ok(None)
}
