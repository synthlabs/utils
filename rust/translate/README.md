# Synth translate

Synchronous, offline language detection, plain-text translation, and a local model
store. The optional `catalog-http` feature fetches model catalog metadata. Core
inference and the local store require neither Tauri nor an async runtime.
**Model files download only through an explicitly invoked installer**, behind the
optional `model-downloads` feature. Opening the store and all inference operations
remain local.

The initial backend is `fxtranslate = 0.5.0` with default features disabled and
`portable`, `lean-embed`, `mmap`, and `threads` enabled. The initial validated
models are Mozilla v3.1 base-memory Russian↔English. Other compatible direct-pair
models can be registered; they have not been validated here. There is no pivoting
or remote fallback. See [third-party notices](THIRD_PARTY_NOTICES.md).

The hosted catalog is authoritative for model availability: its captured snapshot
selects v3.1 base-memory Russian→English and v3.0 base English→Russian. Hosted-model
smoke tests are separate from the original v3.1 regression fixtures.

## Usage

Add a path dependency on `utils/rust/translate` (package `synth-translate`).
Language identifiers always use ISO 639-3: `eng`, `rus`, `ukr`, etc. The language
type validates the three-letter form and normalizes ASCII case; it does not check
the ISO registry or restrict explicit-source requests to whatlang's coverage.

```rust,no_run
use synth_translate::{
    Language, LanguagePair, LoadMode, ModelArtifact, ModelDescriptor,
    TranslationCore, TranslationRequest,
};

# fn example() -> Result<(), synth_translate::TranslationError> {
let mut core = TranslationCore::default();
let tokenizer = ModelArtifact {
    path: "/models/rus-eng/vocab.spm".into(),
    sha256: "de8af5f87e231ee4016cec4bb482e6b584cb1ca34dcb624fc7b8398f25bb35df".parse()?,
};
core.register_model(ModelDescriptor {
    id: "mozilla-v3.1-rus-eng".into(),
    pair: LanguagePair { source: Language::RUSSIAN, target: Language::ENGLISH },
    model: ModelArtifact {
        path: "/models/rus-eng/model.bin".into(),
        sha256: "aa0efa194b3dbd2da129ba0b8db8ef139126cb0914e57072f62061294746f1e2".parse()?,
    },
    source_tokenizer: tokenizer.clone(),
    target_tokenizer: tokenizer,
    load_mode: LoadMode::Lazy,
})?;
let request = TranslationRequest::new("Сегодня прекрасная погода и светит солнце.", Language::ENGLISH);
let outcome = core.translate(&request)?;
println!("{outcome:?}");
# Ok(())
# }
```

Set `request.source = SourceLanguage::Explicit(language)` to bypass detection.
Automatic detection defaults to `DetectionPolicy::ReliableOnly`; set the request's
`detection_policy` to `BestGuess` to accept the detector's leading candidate even
when unreliable. No candidate still produces a skip. Detection is broad and
independent of registered translation pairs; a Cyrillic script alone is not
treated as Russian. Scores and reliability come from whatlang 0.18 and are not
calibrated probabilities. They are preserved on automatic translation results.

`translate_batch(&requests)` returns an ordered
`Vec<Result<TranslationOutcome, TranslationError>>`. Requests can have different
sources, targets, and policies. Both operations return typed `Translated` or
`Skipped` outcomes. Blank input, undetermined/unreliable language, source matching
target, and missing registered pairs are skips. File access, checksum, load,
token-limit, and inference failures are errors. One failed item or model does not
discard other results; a backend failure affects that native chunk.

No text cleanup is performed. URLs, emotes, mentions, whitespace, and line breaks
are passed through as supplied. Inputs are short text units; document segmentation
and recombination belong to the caller. The backend's supported source context
window is 128 tokens including EOS. The core rejects longer inputs before
inference; callers can configure a smaller ceiling. Staying within the window
does not guarantee translation quality or completeness.

## Model ownership and loading

Only one model can be registered per pair, and IDs must be unique. Use
`unregister_model(id)` before replacing it. Removing registration releases its
engine and never deletes files. Registration and removal require exclusive access
to the core; shared references can translate concurrently.

`LoadMode::Lazy` is the default. Registration makes relative paths absolute using
the process working directory at that time, without opening files. Each load
attempt canonicalizes all paths, resolving `..` and symlinks, then verifies
**all decompressed artifact SHA-256 hashes** and loads the engine using those
same resolved paths. Missing files produce retryable `FileAccess` errors on load.
`Eager` does the same during registration, with no registration retained on
failure. Lazy initialization is serialized per model; failed attempts remain
retryable. Hashes come from the trusted caller, not from the files being loaded.
Separate source and target tokenizers are supported; shared files can be supplied
for both roles. Verification deduplicates equal canonical paths with equal hashes;
conflicting hashes produce `ChecksumMismatch` before loading. Hard links are not
deduplicated by filesystem identity.

The loader rejects empty vocabularies and vocabularies missing EOS (`</s>`) or
UNK (`<unk>`) pieces. Backend panics that unwind during loading, tokenization, or
inference become `ModelLoad` or `Inference` errors. A loading panic leaves
initialization retryable; a tokenization panic affects its item, and an inference
panic affects its native chunk. These are targeted guards for trusted artifacts,
not full format validation. Process aborts and allocation failures are outside
the recovery contract. The process panic hook still runs for caught panics.

Callers own the artifacts and must prevent changes from verification through the
end of registration lifetime, including between hashing and the loader reopening
the files. The mapped model must stay present and immutable while loaded. The core
does not acquire cross-process locks or provide installation/removal coordination.
For managed files, use `ModelStore::acquire` and `register_stored_model` below.

`CoreOptions` defaults to 128 source tokens **including EOS**, native chunks of 16,
and up to four batch workers per model (bounded by available parallelism). Source
token limits must be 1–128, batch limits must be positive, and worker counts must
be 1–4. Larger token limits return `InvalidOptions`. Oversized texts
receive individual errors and are never truncated by the wrapper. Larger caller
batches are grouped by pair, processed sequentially in native chunks, and restored
to input order. Tokenization requires the corresponding model to be loaded.

The direct core's loading and inference block the calling thread. It has no queue,
result cache, automatic eviction, or global concurrency budget. Direct callers
select execution threads and own model lifetimes. The optional workload manager
below owns scheduling and managed-model lifetimes. Backend interfaces remain private.

Two performance findings remain deferred with unmodified `fxtranslate 0.5.0`:
source text is tokenized for the limit check and again for inference, and shared
source/target vocabularies are parsed and stored separately. Its encoded-batch
API does not expose target detokenization; using that API here would require an
extra target vocabulary. Sharing parsed vocabulary storage requires dependency
changes. This wrapper adds neither an extra vocabulary copy nor a dependency patch.

## Translation workload management

Enable `workload` for `WorkloadManager`, independently of `catalog-http` and
`model-downloads`. One dedicated worker owns the core and performs all language
detection, file verification, loading, and inference. Construction needs no Tokio
runtime and loads no models. Use the async API from the caller's Tokio runtime;
the crate creates no runtime. Share the manager with `Arc` for concurrent callers.

```rust,no_run
# #[cfg(feature = "workload")]
# async fn example() -> Result<(), Box<dyn std::error::Error>> {
use synth_translate::{
    CoreOptions, Language, ModelStore, TranslationRequest, WorkloadManager, WorkloadOptions,
};

let store = ModelStore::open("/application-data/translate")?;
let manager = WorkloadManager::new(store, CoreOptions::default(), WorkloadOptions::default())?;
let job = manager.submit(TranslationRequest::new("Привет, мир!", Language::ENGLISH)).await?;
let outcome = job.result().await?;
manager.shutdown().await?;
# let _ = outcome;
# Ok(())
# }
```

`WorkloadOptions` defaults to `capacity: 256`, `cache_capacity: 1024`, and
`overflow_policy: OverflowPolicy::RejectNew`. Both capacities must be positive
and all options are fixed at construction. Capacity counts every accepted,
unfinished caller, including callers sharing queued or active inference. Slots
are freed at completion or cancellation, without waiting for result retrieval.
These are entry-count limits, not a byte or process-memory ceiling. Futures
waiting for admission retain their own inputs outside the accepted-work limit.

- `RejectNew` returns `WorkloadError::Capacity` without altering accepted work.
- `DropOldest` evicts the oldest queued caller with `WorkloadError::Evicted`,
  retaining its shared work if other callers still need it. If every slot belongs
  to active work, the new submission returns `Capacity`.
- `Wait` suspends submission until capacity is available. Dropping the submission
  future leaves no accepted request or reservation. FIFO starts at acceptance;
  it does not promise ordering among submissions waiting for admission.

Distinct work dispatches FIFO. Consecutive requests for the same model drain
without a batching timer, up to `CoreOptions.native_batch_size` (default 16).
Set it to one for individual execution. Only one native batch runs per manager,
using the core's configured one-to-four native workers. Separate managers have
independent budgets. Loading and preflight count as active work in snapshots.

Identical queued or running requests share execution. Identity includes exact
text, source mode, target, and detection policy. Successful results enter an
in-memory LRU whose identity also checks the resident model revision. Text is not
normalized; skips and errors are not cached. Cache hits complete immediately,
including when outstanding capacity is full. FIFO is dispatch order, not result
delivery order. Results and source text are never persisted by the manager.

`job.cancel()` or dropping a job/result future cancels only that caller, returning
`Cancelled` if it was unfinished. Removing the last queued caller removes the
work. Native operations already active finish, and their successful output may
still enter the cache. Canceling a completed job has no effect.

Models are acquired and verified from the supplied store only when needed.
`UnsupportedPair` and `ModelNotInstalled` are expected `SkipReason` values;
corruption, access failures, and inference failures remain typed errors. No
operation downloads models. Models and their leases stay resident until shutdown:
stop the manager before replacing or removing one of its resident models.

`snapshot()` returns queued/running caller counts, cached-result count, and
resident model identities/directions. It has no event stream or cumulative metrics.
Counts exclude submissions waiting for admission and completed results retained
by callers. A canceled active operation can still finish with zero running callers.

`shutdown().await` rejects new submissions, wakes blocked submitters, cancels
queued callers, finishes active work, and releases engines, leases, and cache
before returning. It is idempotent; dropping its future does not undo shutdown.
Dropping the manager signals shutdown without waiting. Use explicit shutdown to
await resource release. Unexpected worker failure resolves outstanding requests
with `WorkerFailed`, closes admission, and releases worker-owned resources.

## Catalog metadata

Enable `catalog-http` to use `CatalogClient`. `CatalogClient::open(root)` only opens
the local cache; call `initialize().await` once when the consuming service starts.
It fetches `https://models.jerod.me/latest.json` and, when not already cached, the
referenced snapshot. Raw snapshot bytes must match the pointer's SHA-256 before
parsing. `refresh().await` performs the same operation explicitly; `cached()` and
`CatalogSnapshot::model(pair)` are local reads. There is no background polling.

```rust,no_run
# #[cfg(feature = "catalog-http")]
# async fn example() -> Result<(), synth_translate::CatalogError> {
use synth_translate::CatalogClient;
let client = CatalogClient::open("/application-data/translate")?;
let loaded = client.initialize().await?;
println!("{:?}: {:?}", loaded.freshness, loaded.refresh_error);
for model in loaded.snapshot.models() {
    println!("{}: {} bytes installed", model.id(), model.installed_bytes());
}
# Ok(())
# }
```

Fetching requires a caller-provided Tokio runtime. HTTPS requests have a 15-second
timeout each, no redirects, and byte limits of 16 KiB for the pointer and 4 MiB for
the snapshot. URLs must match the published same-origin snapshot/object layout.
Only schema 1, desktop-eligible Russian↔English entries are normalized; unrelated
language entries are ignored. Unsupported filters, ambiguous pairs, unsafe
filenames, or incomplete engine artifact roles reject the refresh. The parser
accepts a shared `vocab` or a separate `srcvocab`/`trgvocab` pair. Lexicons remain
visible in `files()` but are excluded from `required_files()` and byte totals.

Verified snapshots are retained under `catalog/<sha256>.json`; an atomic
`catalog/current.json` selects the current one. A failed request, validation, or
publication returns the last verified cache with `CatalogFreshness::Cached` and
`refresh_error`. With no valid cache it returns an error. `CatalogCache` exposes
the same local parsing/retention operations without enabling HTTP.

Catalog refresh changes availability metadata only. Installed revisions are not
updated, removed, or downloaded. **All model-file downloads must be explicit user
actions through the installer.** Inventory, verification, registration, and
translation never request model files.

## Managed model store

The consuming app chooses a per-app root. `ModelStore::open(root)` uses this layout:

```text
translate/
  catalog/<sha256>.json
  catalog/current.json
  models/rus-eng/install.json
  models/rus-eng/<decompressed artifacts>
  models/eng-rus/install.json
  models/eng-rus/<decompressed artifacts>
  .locks/rus-eng.lock
  .locks/eng-rus.lock
  .locks/catalog.lock
```

There is one installed revision per direction. `inventory(current_catalog)` checks
metadata, paths, and file sizes, returning `NotInstalled`, `PresentUnverified`,
`Incomplete`, `Invalid`, `Unavailable`, `Unmanaged`, or `Busy`. Supplying a current catalog also
reports whether each installed revision matches it. A quick inventory does not
claim SHA-256 verification. `verify(pair)` hashes every required artifact and
returns its pinned metadata; this result describes that point in time.

A valid ownership manifest with a missing pinned catalog is `Incomplete`; corrupt
or mismatched pinned metadata is `Invalid`. Metadata access failures, including
Windows sharing restrictions, are `Unavailable`. `StoreError::MetadataIo` retains
the affected `path` and underlying I/O `source`. Missing or malformed ownership
manifests remain `Unmanaged` and are preserved.

`acquire(pair)` verifies files under a shared filesystem lock and returns an opaque
`ModelLease`. Consume it with `core.register_stored_model(lease, load_mode)`; the
core retains the guard until unregister or destruction, releasing the engine
before the guard. Registration failures release their incoming guard. Existing
caller-owned descriptors remain supported by `register_model`.

```rust,no_run
use synth_translate::{Language, LanguagePair, LoadMode, ModelStore, TranslationCore};
# fn example() -> Result<(), Box<dyn std::error::Error>> {
let store = ModelStore::open("/application-data/translate")?;
let pair = LanguagePair { source: Language::RUSSIAN, target: Language::ENGLISH };
let mut core = TranslationCore::default();
let lease = store.acquire(pair)?;
let id = lease.model().id().to_owned();
core.register_stored_model(lease, LoadMode::Lazy)?;
// Removal returns Busy until registration ends.
core.unregister_model(&id);
store.remove(pair)?;
# Ok(())
# }
```

`remove(pair)` takes a nonblocking exclusive direction lock and recovers that
direction's pending transaction before deleting anything. Another direction's
installation does not block removal. Pending transaction cleanup returns
`StoreError::CleanupPending` and preserves the model until an explicit retry can
finish cleanup. It returns false for an absent installation after recovery.

Removal requires a schema-1 `install.json` matching the language pair, with valid
catalog and model identities. That manifest is sufficient ownership evidence even
if its pinned catalog or artifacts are missing or corrupt. Loading and replacement
still require verified pinned catalog metadata. Symlinks, Windows reparse points, traversal, and deletion
outside the canonical root are rejected. Unknown directories are reported and
left untouched. Locks coordinate cooperating processes; callers must not modify
managed files outside the store's mutation protocol.

An older installed revision remains usable through its retained snapshot, even
when the latest catalog changes or drops that entry. No automatic upgrade or
version-selection interface is provided. Lock files must not be unlinked while
the store is in use. The filesystem-lock API requires Rust 1.89 or newer.

## Explicit download and installation

Enable `model-downloads` for `ModelInstaller`. It is independent of `catalog-http`;
both features are optional, and default inference and local recovery need no async
runtime. An installer requires a caller-provided Tokio runtime with I/O and time
enabled. It starts no background tasks on construction. There is no automatic
download, saved consent flag, or inference-triggered repair. The consuming app must
call `install` only in response to an explicit user action.

```rust,no_run
# #[cfg(feature = "model-downloads")]
# async fn example(snapshot: &synth_translate::CatalogSnapshot) -> Result<(), Box<dyn std::error::Error>> {
use synth_translate::{InstallControl, InstallPlan, Language, LanguagePair, ModelInstaller, ModelStore};
let pair = LanguagePair { source: Language::RUSSIAN, target: Language::ENGLISH };
let plan = InstallPlan::new(snapshot, pair)?;
println!("Download {} bytes; install {} bytes", plan.download_bytes(), plan.installed_bytes());
let installer = ModelInstaller::new(ModelStore::open("/application-data/translate")?)?;
let control = InstallControl::new();
let progress = control.subscribe();
// Invoke after the user's explicit Download and install action.
let installed = installer.install(plan, control.clone()).await?;
println!("{:?}; {:?}", installed.status, progress.borrow().phase);
// Another task may call control.cancel() while install is running.
# Ok(())
# }
```

`InstallPlan` pins the selected catalog snapshot, revision, required files, and byte
totals without making requests. Catalog changes do not change an existing plan.
One explicit call downloads, checks compressed hashes/sizes, decompresses, checks
installed hashes/sizes, and publishes the installation. It does not load the
translation engine. A fully verified existing revision returns `AlreadyInstalled`
without downloading, with `cleanup_warnings` if earlier transaction cleanup is
still pending. Repair/replacement accepts recognized managed installations;
unidentifiable manifests or missing/corrupt pinned catalogs remain unmanaged.

One persistent `.locks/install.lock` serializes installations across the canonical
store root and cooperating processes. The direction's exclusive lock is also held
through the operation and worker shutdown. Competing operations, registered models,
and held leases return `Busy` before downloading. Files transfer sequentially; no
installation queue or automatic resumption exists.

Model HTTP requests are HTTPS-only, use the catalog's validated object URLs, do
not follow redirects, and request identity encoding with automatic HTTP content
decompression disabled. Downloads stream through bounded worker queues. Each file
gets at most three attempts, including the first, with 1- then 2-second waits, a
15-second connection timeout, and a 30-second read-inactivity timeout. There is no
whole-transfer deadline. Transport/read failures and HTTP 408/500/502/503/504 retry;
other statuses, integrity, decompression, and filesystem failures stop. Byte-range
resumption is not implemented.

`InstallControl` is single-use and cloneable. `subscribe()` exposes the latest
`InstallProgress` through a Tokio watch receiver; it is not an event log. Progress
includes the current phase/file/index, attempts, compressed and decompressed totals,
reused bytes, and cumulative network traffic. `completed_download_bytes` counts
whole verified compressed files; add `file_download_bytes` only while Downloading
to show the current attempt. That file counter resets on retry. Installed byte
counters follow the same completed-file/current-file convention. `network_bytes`
includes discarded attempts and is not a completion percentage. Completion is a
separate terminal phase, after publication.

Cancellation interrupts requests, retry waits, verification, and decompression.
It waits for worker shutdown before returning `Cancelled`. Dropping the install
future signals cancellation; any remaining blocking worker retains the locks until
it stops. Once Publishing begins, the transaction finishes or rolls back even if
the caller cancels or drops the future. Use a new control for each explicit Retry.

Verified compressed objects are retained in `.downloads/<pair>/<sha256>.zst` after
failure/cancellation and are rehashed before reuse. Interrupted `.partial-*` files
are discarded. `discard_downloads(pair)` explicitly removes retained downloads
without removing installed models. Success deletes the temporary compressed copies
and keeps the decompressed installed files. `cleanup_warnings` reports cleanup
that could not finish after a successful commit.

If a download directory loses both `.downloads/<pair>.json` and its `owner.json`,
installation and explicit discard return `StoreError::UnmanagedDownloads` with
the pair, directory, and ownership problem. The directory is preserved and reopen
reports a warning. For manual recovery, close all store users, move the affected
`.downloads/<pair>` directory outside the store, then reopen and explicitly retry.
Malformed ownership records are also preserved; move any corresponding external
`.downloads/<pair>.json` record outside the store when recovering those manually.
There is no automatic deletion or quarantine of unknown downloads.

Staged files and backups live under `.install/<pair>`. Publication uses a durable
schema-1 journal, with an external `.install/<pair>.json` record kept until cleanup
finishes. Existing schema-1 installation manifests and the managed model layout
remain unchanged. On failure before commitment, rollback restores the previous
installation or the absent state. `ModelStore::open` performs local rollback for
uncommitted transactions and cleanup for committed transactions; it never downloads
or publishes a staged new installation. A live installer is skipped. An unfinished
rollback or unrecognized transaction returns `RecoveryRequired`. Once the prior
model or absent state has been restored, leftover cleanup failures are warnings,
as are cleanup failures after a successful commit. Recovery records remain until
cleanup succeeds. Removal and replacement return `CleanupPending` while transaction
cleanup is blocked; verified existing models remain usable.
`recovery_report()` exposes rollback/cleanup results and warnings. Opening an
already-open store does not start a recovery loop; reopen or explicitly retry an
operation after resolving a recovery error. All mutations reject links/reparse
points and paths outside the root. Unknown journals and unmanaged models are
preserved. Local recovery remains available with both HTTP features disabled.

## Verification

From the utils repository:

```text
node scripts/check-translate.mjs
node scripts/check-translate.mjs --models /absolute/path/to/models
node scripts/check-translate.mjs --hosted-models /absolute/path/to/hosted-models
node scripts/check-translate.mjs --live-catalog
```

From a consuming Pepo checkout, prefix the script path with `utils/`.
The crate is its own Cargo workspace with a committed verification lockfile;
verification does not change a consuming application's workspace or lockfile.
The runner checks Rust formatting, Clippy, and model-free unit/integration tests
and doctests in both debug and release profiles, with default features,
`catalog-http`, `model-downloads`, `workload`, and all features enabled.
HTTP tests use deterministic local servers and never contact the public service.
Required regressions use mocks
and synthetic artifacts with matching hashes, covering token boundaries, malformed
tokenizers and weights, panic isolation/retry, and canonical paths. Symlink checks
run where symlink creation is permitted.
CI runs model-free verification on Windows, Linux, and macOS. Real-model inference
was selected for Windows validation; other platforms' model inference is not a
release assertion here.

`--models` explicitly enables the ignored release-mode integration tests. It
requires these existing decompressed files; nothing downloads them:

```text
models/
  firefox-ru-en/model.ruen.intgemm.alphas.bin
  firefox-ru-en/vocab.ruen.spm
  firefox-en-ru/model.enru.intgemm.alphas.bin
  firefox-en-ru/vocab.enru.spm
```

The paths, exact hashes, model version, and provenance are frozen in
`tests/fixtures/mozilla-v3.1.json`. These are test descriptors, not a production
catalog. No ignored evaluation scripts or local manifests are required. Real-model
checks cover both directions, reliable automatic/explicit sources, eager/lazy and
single/batch parity, repeated requests, mixed batches larger than 16, exact source
token boundaries, and malformed tokenizer rejection. They do not repeat quality
benchmarks or validate Tauri packaging.

`--hosted-models` separately enables the hosted-catalog smoke test. Supply the same
directory layout, with the v3.0 **base** English→Russian weights (42,992,955 bytes)
instead of the original v3.1 base-memory weights. Exact decompressed hashes are
frozen in `tests/fixtures/hosted-catalog-v1.json`, whose `source_snapshot_url`
records the published snapshot from which its two entries were captured. This
option copies supplied files into a temporary managed store and checks hashing,
lazy/eager registration, automatic/explicit source detection, repeated and batch
inference, removal exclusion, and guard release. It downloads nothing and leaves
the supplied files unchanged.

`--live-catalog` explicitly probes the public default metadata using the production
HTTPS client. It fetches catalog JSON only, uses a temporary cache, and is excluded
from routine CI. It can be combined with either local-model option.

`node scripts/check-translate.mjs --download-model rus-eng` explicitly authorizes
a real model download for that direction; `eng-rus` is the other accepted value.
This runs an ignored end-to-end HTTPS download/install/inference test in a temporary
store, prints the pinned model and byte totals, and cleans up after releasing the
engine. The flag is never enabled by ordinary verification or `--live-catalog`.
Installer regression tests use synthetic compressed artifacts and loopback HTTP,
including cancellation, retries, corrupt downloads, replacement/repair, dropped
futures, cross-process contention, Windows sharing violations, and process exit at
each publication boundary. No real model is required for routine tests.
