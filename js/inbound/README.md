# Inbound error reporting

`ErrorToast` and `ReportWizard` retain their default entry points. Applications can
opt into custom presentation with `ErrorController` and `connectErrorCapture`.

```ts
const errors = new ErrorController({
  log: error => logHandledError(error.message, error.target ?? 'app')
});
const unsubscribe = errors.subscribe(render);
const stopTimers = errors.start();
const disconnect = connectErrorCapture(errors, window);
```

Call all three cleanup functions when the presentation host unmounts. Passing
`window` enables uncaught-exception and unhandled-rejection capture; omit it for
native capture only. Native capture subscribes before replaying the most recent
16 records and deduplicates overlapping records by ID. The continuous event is
`inbound://error-captured`; `inbound://error-detected` keeps its legacy session limit.

Use `errors.capture(error, { source, title, recovery, action })` for caught failures.
`source` is a stable action identifier, independent of translated copy. `title`,
`recovery`, and `action.label` accept strings or functions returning localized text.
Passing `false` as the third argument records an inline error without a toast;
the returned incident can still be passed to `errors.openReport(incident)`.

The controller owns grouping and expiry. Present `snapshot.notifications` with
an infinite library timeout, call `pause(id, reason)` / `resume(id, reason)` for
hover and focus, and call `dismiss(id)` when dismissed. Repeats do not extend the
deadline. Opening a report freezes its error context, removes the selected toast,
and pauses other notifications until `closeReport()`.

Do not also call an unmarked error logger for a handled failure. `logHandledError`
preserves its diagnostic log with `inbound.handled=true`, which Inbound excludes
from automatic capture. Native diagnostics whose outcomes have another owner can
use `capture::HANDLED_TARGET`; filtering an application's capture target leaves its
other log targets intact. Panics are captured independently.

## Custom report UI

`ReportWizard` accepts optional `copy`, `class`, `header`, `action`, and
`includeToggle` props. The snippets receive the types exported from `copy.ts`.
Supply an `onerror` callback using `logHandledError` when the host owns failures;
the wizard displays collection/submission failures with retry controls.

Set `submitSnapshot` to send the prefilled error that the user reviewed. The
optional `ReportInput.error` is preferred over the latest native error; omitted
or null values preserve the legacy lookup. `include.error=false` always omits the
error from the outgoing report. Inclusion defaults and the existing transport are
unchanged. Mount a new wizard for each report; later captures do not replace its
selection or entered information.

## Verification

Run `node --test utils/js/inbound/controller.test.mjs` from a consuming app with
TypeScript installed, or `node --test js/inbound/controller.test.mjs` from utils.
Native tests run with `cargo test -p inbound --lib` from the consuming workspace.
