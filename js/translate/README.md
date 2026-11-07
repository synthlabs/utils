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

## Shared model-management UI

The framework-neutral `ModelManagerController` is exported from `$utils/translate`.
The Svelte 5 `ModelManagerPanel` and its presentation types are exported separately
from `$utils/translate/ui`. Importing the client/controller does not import Svelte.
The host owns the client, controller, confirmation UI, and lifecycle:

```ts
import { createTranslationClient, ModelManagerController } from '$utils/translate';

const client = createTranslationClient();
const models = new ModelManagerController({
	client,
	confirm: (request) => confirmationUi.open(request),
	onerror: (error, context) => errorUi.show(error, context)
});
await models.connect();
const unsubscribe = models.subscribe(renderModelState);

// At the end of the owning lifecycle, after views have unmounted:
unsubscribe();
await models.dispose();
await client.dispose();
```

`confirmationUi`, `errorUi`, and `renderModelState` above belong to the host.
`connect()` is idempotent. Connection failures reject and are also reported inline
and through `onerror`; an explicit subsequent `connect()` retries. Opening the UI
does not refresh metadata or download/verify model files. The existing plugin's
startup metadata refresh remains in effect.

```svelte
<script lang="ts">
	import { ModelManagerPanel } from '$utils/translate/ui';
	import type { ModelManagerController } from '$utils/translate';
	let { models }: { models: ModelManagerController } = $props();
</script>

<ModelManagerPanel controller={models} />
```

The panel only subscribes to the controller. Unmounting it leaves the controller,
client, and application-owned installation alone. Multiple panels can share a
controller. The host must keep its client alive until controller disposal resolves.
Disposal rejects new actions and invalidates unanswered confirmations, but awaits
accepted release/change/resume workflows and cancellation requests before
detaching. An ordinary installation continues in the backend after disposal.

### Confirmation contract

The required `confirm(request)` returns `Promise<boolean>` and must obtain an
explicit user decision; only `true` approves. No built-in dialog or browser confirm
is used. Requests are typed as `ModelConfirmationRequest`:

- `action` is `install`, `repair`, `replace`, `remove`, or `clear_downloads`.
- `pair` and `installed` identify the language direction and known local model.
- Download actions additionally carry the exact pinned `plan`, including model
  identity/version, catalog digest, download bytes, and installed bytes. Present
  these details before authorizing a download; byte values are decimal strings.
- `interruptsTranslation` means this action will release the workload. Explain
  that queued translations are cancelled and active work is awaited. A replacement
  keeps translation paused through its terminal installation result.

The controller passes a detached copy to the confirmation UI. Changes to that
copy do not change the actual installation plan. A catalog refresh does not replace
the reviewed plan. Changed local data, a newly loaded model, or a backend Busy
response requires the user to retry and review the action again; nothing is
automatically replayed. A rejected/failed confirmation performs no mutation.

### State and actions

`snapshot()` and `subscribe()` expose Installed, Available, and Needs attention
rows, service/catalog state, the active/latest installation, pending action/stage,
inline errors, and the latest action notice. Snapshots detach model/store DTOs;
`subscribe` immediately delivers current state and returns an unsubscribe function.
Unmanaged rows are read-only. Inventory's `present_unverified` remains unverified
even after an explicit point-in-time verification result.

Actions are `install(pair, 'install' | 'repair' | 'replace')`, `remove(pair)`,
`clearDownloads(pair)`, `verify(pair)`, `refreshModels()`, `refreshCatalog()`,
`retryInitialize()`, `resume()`, `cancelInstallation()`, and `dismissError(id)`.
Boolean-returning actions resolve false when blocked, declined, invalidated, or
failed; successful installation submission is acknowledgment, not completion.
Operation outcomes and restoration/refresh failures remain in the snapshot.
Action failures are handled inline and passed to optional `onerror(error, context)`
once. The library does not log them or emit toasts; a host can connect its existing
Inbound error presentation there. Catalog startup failures remain visible in
catalog state even when the local service is ready.

Loaded-model removal/replacement restores a previously running workload, including
after a failed mutation; an already-paused workload remains paused. Mutation and
resume failures are retained separately. A full window reload cannot finish that
frontend-owned restoration: a new controller shows paused state with explicit
Resume, disabled until an active installation finishes. Application exit retains
the backend's existing signal-and-exit behavior.

Cancel waits for the backend's terminal outcome before claiming cancellation and
never clears downloaded files. Clear downloads is a separate confirmed action
without retained-size accounting. It reports whether files were removed and does
not release a loaded model; Busy remains visible. Verification is explicit.
Progress defaults to phase, current-phase bar, and Cancel, with closed expandable
file/byte/retry/reuse/network details. Verification/publication are indeterminate;
`finished`/`result`/`error`, rather than phase alone, determine completion.

### Presentation overrides

The panel accepts `copy: Partial<ModelManagerCopy>`, `class`, and typed Svelte
snippets: `header(snapshot)`, `modelRow(row, controls)`, `action(control)`, and
`progress(installation, cancelControl)`. Controls expose `kind`, `label`, `disabled`,
optional `title`, and `onclick`. Custom controls must preserve disabled state,
accessible names, keyboard behavior, and the provided action callback. A custom
progress view must preserve terminal-outcome handling. Copy includes language,
pair, byte, phase, and notice formatters, including expanded diagnostics.

Default styling uses semantic Tailwind theme classes without Pepo imports. Hosts
must supply their theme and include this UI directory in Tailwind's sources. A
fully custom UI can subscribe directly to the framework-neutral controller.

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
node scripts/check-translate-ui.mjs
```

`--quick` runs plugin unit tests, generated-contract drift, client regressions, and
TypeScript checks. `--rust-only` excludes frontend/code-generation checks.
The default runner also checks formatting/Clippy and debug/release core and
consumer tests. The consumer's Rust build cache stays under this private
verification package's `node_modules/.cache`, alongside its test dependencies.
All routine fixtures are synthetic, use loopback HTTP, and require
no model downloads. `tauri-test` is a verification feature for mock-runtime and
loopback fixtures; consuming applications enable `tauri-plugin`.

The independent UI runner checks the existing client regressions, framework-neutral
TypeScript, Svelte types/accessibility diagnostics, controller regressions, and
mounted panel/custom-presentation tests. Its private Svelte config does not depend
on a consuming app. UI tests use `.test.mts` to keep them within this standalone
runner instead of a consuming app's potentially different Vitest version. The
private verification package uses Vitest 4.1 with its Vite 8 Svelte plugin.

The independent `node scripts/check-translate.mjs` matrix continues to verify all
Tauri-free feature combinations on Windows, Linux, and macOS. Packaged application
and real-model release validation remain separate work.
