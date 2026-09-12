use std::collections::VecDeque;
use std::panic;
use std::sync::{Arc, Mutex, Once, OnceLock};

use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_log::{fern, log, Target, TargetKind};

use crate::collect::submitted_at;
use crate::report::ErrorContext;

const MAX_ERRORS: usize = 16;
const CAPTURED_EVENT_NAME: &str = "inbound://error-captured";
pub const HANDLED_TARGET: &str = "inbound::handled";
const EVENT_NAME: &str = "inbound://error-detected";

static INSTALL_HOOK: Once = Once::new();
static CAPTURE: OnceLock<Mutex<CaptureState>> = OnceLock::new();

#[derive(Clone, Debug, serde::Serialize)]
pub struct CapturedError {
    pub id: String,
    pub error: ErrorContext,
}

type EmitCaptured = Arc<dyn Fn(CapturedError) + Send + Sync>;
type EmitError = Arc<dyn Fn(ErrorContext) + Send + Sync>;

#[derive(Default)]
struct CaptureState {
    emit_error: Option<EmitError>,
    emit_captured: Option<EmitCaptured>,
    captures: VecDeque<CapturedError>,
    sequence: u64,
    errors: VecDeque<ErrorContext>,
    panic: Option<ErrorContext>,
    emitted: bool,
}

pub fn log_target() -> Target {
    let dispatch = fern::Dispatch::new().chain(fern::Output::call(|record| {
        if should_capture(record) {
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

fn should_capture(record: &log::Record<'_>) -> bool {
    record.level() == log::Level::Error
        && record.target() != HANDLED_TARGET
        && !record
            .key_values()
            .get(log::kv::Key::from("inbound.handled"))
            .is_some_and(|value| value.to_string() == "true")
}

pub fn captured_errors() -> Vec<CapturedError> {
    state().lock().unwrap().captures.iter().cloned().collect()
}

pub fn install<R: Runtime>(app: AppHandle<R>) {
    let capture_app = app.clone();
    let emit_captured = Arc::new(move |payload| {
        let _ = capture_app.emit(CAPTURED_EVENT_NAME, payload);
    });
    let emit_error = Arc::new(move |payload| {
        let _ = app.emit(EVENT_NAME, payload);
    });

    {
        let mut state = state().lock().unwrap();
        state.emit_error = Some(emit_error);
        state.emit_captured = Some(emit_captured);
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
        state.errors.push_back(error.clone());
    }

    emit_capture(error);
    maybe_emit();
}

fn record_panic(error: ErrorContext) {
    {
        let mut state = state().lock().unwrap();
        state.panic = Some(error.clone());
    }

    emit_capture(error);
    maybe_emit();
}

impl CaptureState {
    fn capture(&mut self, error: ErrorContext) -> CapturedError {
        self.sequence += 1;
        let captured = CapturedError {
            id: self.sequence.to_string(),
            error,
        };
        if self.captures.len() == MAX_ERRORS {
            self.captures.pop_front();
        }
        self.captures.push_back(captured.clone());
        captured
    }
}

fn emit_capture(error: ErrorContext) {
    let (emit, captured) = {
        let mut state = state().lock().unwrap();
        let captured = state.capture(error);
        (state.emit_captured.clone(), captured)
    };
    if let Some(emit) = emit {
        emit(captured);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn error(message: &str) -> ErrorContext {
        ErrorContext {
            kind: "error".into(),
            message: message.into(),
            target: None,
            timestamp: "1".into(),
        }
    }

    #[test]
    fn captures_have_stable_ids_and_retain_the_latest_sixteen() {
        let mut state = CaptureState::default();
        let first = state.capture(error("first"));
        assert_eq!(first.id, "1");
        for _ in 0..20 {
            state.capture(error("next"));
        }
        assert_eq!(state.captures.len(), 16);
        assert_eq!(state.captures.front().unwrap().id, "6");
        assert_eq!(state.captures.back().unwrap().id, "21");
        assert!(!state.emitted);
    }

    #[test]
    fn handled_diagnostics_and_warnings_do_not_trigger_capture() {
        let mut record = log::Record::builder();
        record.level(log::Level::Error).target("webview");
        assert!(should_capture(&record.build()));
        let metadata = [("inbound.handled", "true")];
        record.key_values(&metadata);
        assert!(!should_capture(&record.build()));
        let empty: [(&str, &str); 0] = [];
        record.key_values(&empty).target(HANDLED_TARGET);
        assert!(!should_capture(&record.build()));
        record.target("native").level(log::Level::Warn);
        assert!(!should_capture(&record.build()));
    }
}
