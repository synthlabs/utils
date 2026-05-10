use std::sync::Arc;

use serde_json::Value;
use tauri::{plugin::TauriPlugin, AppHandle, Manager, Runtime, State};

pub mod capture;
mod collect;
mod report;
mod transport;

pub use report::{
    AppId, BuildInfo, ErrorContext, IncludeFlags, LogPreview, Report, ReportInput, SystemInfo,
};
pub use transport::{Transport, WebhookTransport};

pub trait Scrubber<R: Runtime>: Send + Sync {
    fn scrub(&self, app: &AppHandle<R>) -> Result<Option<Value>, String>;
}

pub struct Config<R: Runtime> {
    pub app: AppId,
    pub build: BuildInfo,
    pub scrubber: Option<Arc<dyn Scrubber<R>>>,
}

pub struct InboundState<R: Runtime> {
    app: AppId,
    build: BuildInfo,
    transport: Arc<dyn Transport>,
    scrubber: Option<Arc<dyn Scrubber<R>>>,
}

impl<R: Runtime> InboundState<R> {
    pub fn new(config: Config<R>) -> Self {
        let transport = Arc::new(WebhookTransport::new(config.app));

        Self {
            app: config.app,
            build: config.build,
            transport,
            scrubber: config.scrubber,
        }
    }

    fn has_config(&self) -> bool {
        self.scrubber.is_some()
    }
}

pub fn init<R: Runtime>(config: Config<R>) -> TauriPlugin<R> {
    tauri::plugin::Builder::<R>::new("inbound")
        .invoke_handler(tauri::generate_handler![
            capabilities,
            preview_system,
            preview_build,
            preview_log_tail,
            preview_config,
            preview_error,
            submit,
        ])
        .setup(move |app, _api| {
            app.manage(InboundState::new(config));
            capture::install(app.clone());
            Ok(())
        })
        .build()
}

#[derive(Clone, serde::Serialize)]
pub struct Capabilities {
    pub has_config: bool,
}

#[tauri::command]
fn capabilities<R: Runtime>(_app: AppHandle<R>, state: State<'_, InboundState<R>>) -> Capabilities {
    Capabilities {
        has_config: state.has_config(),
    }
}

#[tauri::command]
fn preview_system() -> SystemInfo {
    collect::system()
}

#[tauri::command]
fn preview_build<R: Runtime>(_app: AppHandle<R>, state: State<'_, InboundState<R>>) -> BuildInfo {
    state.build.clone()
}

#[tauri::command]
fn preview_log_tail<R: Runtime>(
    app: AppHandle<R>,
    max_bytes: Option<usize>,
) -> Result<LogPreview, String> {
    collect::preview_log_tail(&app, max_bytes.unwrap_or(2 * 1024 * 1024))
}

#[tauri::command]
fn preview_config<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, InboundState<R>>,
) -> Result<Option<String>, String> {
    scrub_config(&app, &state).map(|config| {
        config
            .map(|value| serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()))
    })
}

#[tauri::command]
fn preview_error() -> Option<ErrorContext> {
    capture::latest_error()
}

#[tauri::command]
async fn submit<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, InboundState<R>>,
    input: ReportInput,
) -> Result<(), String> {
    let system = input.include.system.then(collect::system);
    let config = if input.include.config {
        scrub_config(&app, &state)?
    } else {
        None
    };
    let error = if input.include.error {
        capture::latest_error()
    } else {
        None
    };

    let mut attachments = Vec::new();
    if input.include.log {
        if let Some(attachment) = collect::log_tail_attachment(&app, 2 * 1024 * 1024)? {
            attachments.push(attachment);
        }
    }

    let report = Report::new(
        state.app,
        state.build.clone(),
        collect::submitted_at(),
        input.discord_user,
        input.message,
        input.include,
        system,
        config,
        error,
    );

    state
        .transport
        .submit(&report, attachments)
        .await
        .map_err(|err| err.to_string())
}

fn scrub_config<R: Runtime>(
    app: &AppHandle<R>,
    state: &InboundState<R>,
) -> Result<Option<Value>, String> {
    match state.scrubber.as_ref() {
        Some(scrubber) => scrubber.scrub(app),
        None => Ok(None),
    }
}
