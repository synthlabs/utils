use std::collections::VecDeque;
use std::panic;
use std::sync::{Arc, Mutex, Once, OnceLock};

use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_log::{fern, log, Target, TargetKind};

use crate::collect::submitted_at;
use crate::report::ErrorContext;

const MAX_ERRORS: usize = 16;
const EVENT_NAME: &str = "inbound://error-detected";

static INSTALL_HOOK: Once = Once::new();
static CAPTURE: OnceLock<Mutex<CaptureState>> = OnceLock::new();

type EmitError = Arc<dyn Fn(ErrorContext) + Send + Sync>;

#[derive(Default)]
struct CaptureState {
    emit_error: Option<EmitError>,
    errors: VecDeque<ErrorContext>,
    panic: Option<ErrorContext>,
    emitted: bool,
}

pub fn log_target() -> Target {
    let dispatch = fern::Dispatch::new().chain(fern::Output::call(|record| {
        if record.level() == log::Level::Error {
            record_error(ErrorContext {
                kind: "error".to_owned(),
                message: record.args().to_string(),
                target: Some(record.target().to_owned()),
                timestamp: submitted_at(),
            });
        }
    }));

    Target::new(TargetKind::Dispatch(dispatch))
}

pub fn install<R: Runtime>(app: AppHandle<R>) {
    let emit_error = Arc::new(move |payload| {
        let _ = app.emit(EVENT_NAME, payload);
    });

    {
        let mut state = state().lock().unwrap();
        state.emit_error = Some(emit_error);
    }

    INSTALL_HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            let location = info
                .location()
                .map(|location| format!("{}:{}", location.file(), location.line()));
            let payload = info
                .payload()
                .downcast_ref::<&str>()
                .map(|value| (*value).to_owned())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".to_owned());

            record_panic(ErrorContext {
                kind: "panic".to_owned(),
                message: match location {
                    Some(location) => format!("{payload} at {location}"),
                    None => payload,
                },
                target: None,
                timestamp: submitted_at(),
            });

            previous(info);
        }));
    });

    maybe_emit();
}

pub fn latest_error() -> Option<ErrorContext> {
    let state = state().lock().unwrap();
    state.panic.clone().or_else(|| state.errors.back().cloned())
}

fn record_error(error: ErrorContext) {
    {
        let mut state = state().lock().unwrap();
        if state.errors.len() == MAX_ERRORS {
            state.errors.pop_front();
        }
        state.errors.push_back(error);
    }

    maybe_emit();
}

fn record_panic(error: ErrorContext) {
    {
        let mut state = state().lock().unwrap();
        state.panic = Some(error);
    }

    maybe_emit();
}

fn maybe_emit() {
    let (emit_error, payload) = {
        let mut state = state().lock().unwrap();
        if state.emitted {
            return;
        }
        let Some(payload) = state.panic.clone().or_else(|| state.errors.back().cloned()) else {
            return;
        };
        let Some(emit_error) = state.emit_error.clone() else {
            return;
        };
        state.emitted = true;
        (emit_error, payload)
    };

    emit_error(payload);
}

fn state() -> &'static Mutex<CaptureState> {
    CAPTURE.get_or_init(|| Mutex::new(CaptureState::default()))
}
