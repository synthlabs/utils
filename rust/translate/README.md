# Synth translate

Synchronous, offline language detection, plain-text translation, and a local model
store. The optional `catalog-http` feature fetches model catalog metadata. Core
inference and the local store require neither Tauri nor an async runtime.
**Model files are never downloaded by this crate.**

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

Loading and inference block the calling thread. There is no queue, result cache,
automatic eviction, or global concurrency budget. Callers select execution threads
and own model lifetimes. Backend interfaces remain private.

Two performance findings remain deferred with unmodified `fxtranslate 0.5.0`:
source text is tokenized for the limit check and again for inference, and shared
source/target vocabularies are parsed and stored separately. Its encoded-batch
API does not expose target detokenization; using that API here would require an
extra target vocabulary. Sharing parsed vocabulary storage requires dependency
changes. This wrapper adds neither an extra vocabulary copy nor a dependency patch.

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
actions in the future installer.** Inventory, verification, registration, and
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
`Incomplete`, `Invalid`, `Unmanaged`, or `Busy`. Supplying a current catalog also
reports whether each installed revision matches it. A quick inventory does not
claim SHA-256 verification. `verify(pair)` hashes every required artifact and
returns its pinned metadata; this result describes that point in time.

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

`remove(pair)` takes a nonblocking exclusive lock. It returns false for an absent
installation, refuses unmanaged metadata, and can remove an identified model with
missing/corrupt artifacts. Symlinks, Windows reparse points, traversal, and deletion
outside the canonical root are rejected. Unknown directories are reported and
left untouched. Locks coordinate cooperating processes; callers must not modify
managed files outside the store's mutation protocol.

Installer handoff (installation itself is not implemented here):

1. Resolve the desired revision from the catalog after an explicit user action.
2. Retain its verified snapshot using `retain_catalog`.
3. Acquire `exclusive(pair)` before any installation mutation or replacement;
   hold this `ModelMutationGuard` through publication.
4. Publish verified decompressed required files and serialize
   `InstallationManifest::for_model(model)` as `install.json` in the managed
   directory. The manifest schema is 1 and pins normalized source/target codes,
   catalog digest, and immutable model ID. Download/staging/failure recovery belong
   to the future installer; do not treat the example layout as a copying API.

An older installed revision remains usable through its retained snapshot, even
when the latest catalog changes or drops that entry. No automatic upgrade or
version-selection interface is provided. Lock files must not be unlinked while
the store is in use. The filesystem-lock API requires Rust 1.89 or newer.

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
and doctests in both debug and release profiles, with HTTP disabled and enabled.
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
