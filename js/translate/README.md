# Shared translation integration

Framework-neutral client for the optional `synth-translate` Tauri plugin. Import
through the consuming application's utils alias; this directory's private npm
package supplies verification dependencies, not a published runtime package.

## Register the plugin

Add these dependencies to the consuming app's `src-tauri/Cargo.toml`, adjusting
paths for its checkout:

```toml
[dependencies]
synth-translate = { path = "../utils/rust/translate", features = ["tauri-plugin"] }

[build-dependencies]
translate-build = { path = "../utils/rust/translate-build" }
```

Add the inlined plugin to the app's existing build attributes:

```rust
use tauri_build::{Attributes, DefaultPermissionRule, InlinedPlugin};

let attributes = Attributes::new().plugin(
    "translate",
    InlinedPlugin::new()
        .commands(translate_build::COMMANDS)
        .default_permission(DefaultPermissionRule::AllowAllCommands),
);
tauri_build::try_build(attributes).expect("build application permissions");
```

Grant `translate:default` to the appropriate application windows in the app's
capabilities, alongside its existing permissions. Register the runtime plugin:

```rust
use synth_translate::plugin::{init, PluginConfig};

let builder = tauri::Builder::default().plugin(init(PluginConfig::default()));
```

The default store is `app_data_dir()/translate`. Rust hosts can override
`PluginConfig.store_root`, `core_options`, and `workload_options`. Initialization
opens the store on a blocking worker, then refreshes catalog metadata once in the
background. It does not download models. A local initialization error leaves the
service `unavailable` without failing app setup. Catalog failures remain separate
from local-service readiness, including when no catalog is cached.

The model-free consumer at `rust/translate/tests/consumer` verifies these build,
permission, and runtime registration steps using Tauri's mock runtime.

## Use the frontend client

```ts
import { createTranslationClient, TranslationClientError } from '$utils/translate';

const client = createTranslationClient();
await client.initialize(); // Await local readiness; retry an unavailable store.
const stop = await client.watchState((snapshot) => {
	console.log(snapshot.phase, snapshot.installation);
});

const abort = new AbortController();
try {
	const outcome = await client.translate(
		{ text: 'Привет, мир!', source: null, target: 'eng' },
		{ signal: abort.signal }
	);
	if (outcome.kind === 'translated') console.log(outcome.translation.text);
} catch (error) {
	if (error instanceof TranslationClientError) {
		console.log(error.code, error.message, error.details);
	}
}

// When the consuming lifecycle ends:
stop();
await client.dispose();
```

Language identifiers use ISO 639-3 (`eng`, `rus`). A null source enables automatic
detection. `detectionPolicy` defaults to `reliable_only`; `best_guess` accepts an
unreliable leading candidate. Translated and skipped outcomes resolve normally.
Failures reject with `TranslationClientError`. Known backend codes and diagnostic
details are preserved; raw strings, JavaScript errors, and malformed rejections
become `ipc` errors with the original value in `cause`. The client does not present
notifications or log failures.

`translate` registers an operation before waiting for its result. Abort signals
and client disposal cancel that caller's interest, including a request still
waiting for workload admission. Window destruction or page navigation cancels
frontend requests owned by that window. Active native inference may finish, as in
the underlying workload contract. A client should always be disposed on unmount.

`watchState` subscribes before querying and ignores older/equal revisions. It
returns an idempotent cleanup function and also cleans up when disposed during
asynchronous subscription setup. `workload` is a point-in-time snapshot; queue
counts are available through `status()` rather than a workload event stream.

If subscription setup and listener cleanup both fail, the original normalized
setup error takes precedence. Disposal during listener registration preserves the
`disposed` error even when cleanup fails. Standalone cleanup failures are normalized
as `TranslationClientError`, and cleanup remains idempotent.

## Manage models

```ts
const catalog = await client.catalog();
// catalog.error exposes a failed refresh; catalog.catalog may still be cached.
const inventory = await client.inventory();
const plan = await client.planInstall({ source: 'rus', target: 'eng' });

// Present plan.model and obtain explicit download consent before this call.
const installation = await client.startInstall(plan);
const latest = await client.installation(installation.id);
// Explicit cancellation:
await client.cancelInstall(installation.id);
```

Planning reads a verified local catalog and makes no requests. Installation pins
the reviewed `catalogSha256`, pair, and model identity even if the current catalog
changes. The backend reconstructs the actual plan from its retained snapshot;
display fields supplied by the caller are not download authority. Starting an
installation acknowledges the operation, not successful completion. Observe its
`finished`, `result`, and `error` fields through `watchState` or `installation(id)`.

Installations belong to the application and continue when a client is disposed
or its window closes/reloads. A new client can read `status().installation` and
reconnect by ID. Only the active or latest terminal installation is retained;
starting the next installation replaces that record. Installation history and
automatic resumption after application exit are not provided.

Progress preserves the core's completed-file/current-file counters, retries,
reuse, and cumulative network traffic. All byte counts and revisions are decimal
strings, preserving their exact values in JavaScript. State notifications are
sampled at 100 ms; observed phase/terminal changes emit immediately, and
intermediate watch updates may coalesce. `finished` and the
terminal result are authoritative; a publishing phase is not completion. Cleanup
warnings remain on successful installation results.

Loaded models retain leases. To remove or replace one, explicitly release the
workload, perform the change, and resume:

```ts
await client.release();
try {
	await client.removeModel({ source: 'rus', target: 'eng' });
} finally {
	await client.resume();
}
```

Release closes admission, cancels queued translations, waits for active work, and
leaves the service paused. Resume starts a fresh workload using the configured
options. Release continues even if its IPC waiter disappears. Store/installer
operations preserve `busy`, recovery errors, and unmanaged-data protections.
Also available: `verifyModel(pair)`, `discardDownloads(pair)`, and explicit
`refreshCatalog()`. An explicit refresh rejects when no usable catalog is available;
cached fallback resolves with its refresh error attached.

For native callers, dropping or aborting a `refresh_catalog()` future cancels its
active refresh, clears `refreshing`, and retains the previous catalog, freshness,
and error for an explicit retry. Dropping a caller waiting for the refresh lock
leaves the active refresh unchanged. Shutdown clears refresh status and remains
terminal.

## Native Rust access and exit

Import `synth_translate::plugin::TranslationExt` and call `app.translation()` to
obtain the same `Arc<TranslationService>` used by IPC. `submit(request).await`
returns a `TranslationTicket`; `result().await` returns the native
`TranslationOutcome`. Canceling/dropping a ticket cancels its caller. Native and
frontend consumers share the queue, cache, model store, and release/resume state.

On final application exit or plugin teardown the service signals workload and
installation cancellation without preventing or delaying exit. Cleanup may be
interrupted; store recovery runs on the next open. Explicit workload release
awaits engine/lease release. `request_shutdown()` is synchronous and permanent.

## Generate and verify

From utils, explicitly regenerate the committed contract with:

```text
cargo run --locked --manifest-path rust/translate/Cargo.toml --features tauri-plugin --example export_translate_types -- --write
```

Normal app builds and verification do not rewrite it. Windows plugin checks:

```text
npm ci --ignore-scripts --prefix js/translate
node scripts/check-translate-tauri.mjs
```

`--quick` runs plugin unit tests, generated-contract drift, client regressions, and
TypeScript checks. `--rust-only` excludes frontend/code-generation checks.
The default runner also checks formatting/Clippy and debug/release core and
consumer tests. The consumer's Rust build cache stays under this private
verification package's `node_modules/.cache`, alongside its test dependencies.
All routine fixtures are synthetic, use loopback HTTP, and require
no model downloads. `tauri-test` is a verification feature for mock-runtime and
loopback fixtures; consuming applications enable `tauri-plugin`.

The independent `node scripts/check-translate.mjs` matrix continues to verify all
Tauri-free feature combinations on Windows, Linux, and macOS. Packaged application
and real-model release validation remain separate work.
