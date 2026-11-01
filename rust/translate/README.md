# Synth translate

Synchronous, offline language detection and plain-text translation. This crate has
no dependency on Tauri, Pepo, an async runtime, a catalog, or a downloader.

The initial backend is `fxtranslate = 0.5.0` with default features disabled and
`portable`, `lean-embed`, `mmap`, and `threads` enabled. The initial validated
models are Mozilla v3.1 base-memory Russian↔English. Other compatible direct-pair
models can be registered; they have not been validated here. There is no pivoting
or remote fallback. See [third-party notices](THIRD_PARTY_NOTICES.md).

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
Future model-store code must enforce this ownership contract.

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

## Verification

From the utils repository:

```text
node scripts/check-translate.mjs
node scripts/check-translate.mjs --models /absolute/path/to/models
```

From a consuming Pepo checkout, prefix the script path with `utils/`.
The crate is its own Cargo workspace with a committed verification lockfile;
verification does not change a consuming application's workspace or lockfile.
The runner checks Rust formatting, Clippy, and model-free unit/integration tests
and doctests in both debug and release profiles. Required regressions use mocks
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
