//! Optional app-owned Tauri integration and native service.

mod commands;
pub mod dto;
mod error;
mod service;

pub use error::{ErrorCode, ServiceError};
pub use service::{PluginConfig, TranslationService, TranslationTicket};

use std::sync::Arc;

use tauri::{plugin::TauriPlugin, AppHandle, Emitter, Manager, RunEvent, Runtime, WindowEvent};

pub const STATE_EVENT: &str = "translate://state-changed";
pub const MODELS_EVENT: &str = "translate://models-changed";

pub trait TranslationExt<R: Runtime>: Manager<R> {
    fn translation(&self) -> Arc<TranslationService> {
        self.state::<Arc<TranslationService>>().inner().clone()
    }
}

impl<R: Runtime, T: Manager<R>> TranslationExt<R> for T {}

pub fn init<R: Runtime>(config: PluginConfig) -> TauriPlugin<R> {
    tauri::plugin::Builder::new("translate")
        .invoke_handler(commands::handler())
        .setup(move |app, _| {
            let root = config.store_root.clone().map(Ok).unwrap_or_else(|| {
                app.path()
                    .app_data_dir()
                    .map(|path| path.join("translate"))
                    .map_err(|error| ServiceError::new(ErrorCode::Io, error.to_string()))
            });
            let service = TranslationService::with_root(root, config);
            app.manage(service.clone());
            tauri::async_runtime::spawn(bridge(app.clone(), service.subscribe()));
            service.start_initialization();
            Ok(())
        })
        .on_page_load(|webview, payload| {
            if payload.event() == tauri::webview::PageLoadEvent::Started {
                webview
                    .translation()
                    .forget_owner(&service::Owner::Webview(webview.window().label().into()));
            }
        })
        .on_event(|app, event| match event {
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::Destroyed,
                ..
            } => {
                app.translation()
                    .forget_owner(&service::Owner::Webview(label.clone()));
            }
            RunEvent::Exit => app.translation().request_shutdown(),
            _ => {}
        })
        .on_drop(|app| app.translation().request_shutdown())
        .build()
}

async fn bridge<R: Runtime>(
    app: AppHandle<R>,
    mut changes: tokio::sync::watch::Receiver<dto::ServiceSnapshot>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_millis(100));
    let mut revision = String::new();
    let mut models_revision = String::new();
    let mut previous_phase = None;
    loop {
        let tick = tokio::select! {
            _ = interval.tick() => true,
            changed = changes.changed() => {
                if changed.is_err() { break; }
                false
            }
        };
        let snapshot = changes.borrow_and_update().clone();
        let phase = (
            snapshot.phase,
            snapshot
                .installation
                .as_ref()
                .map(|v| (v.progress.phase, v.finished)),
        );
        if revision != snapshot.revision && (tick || previous_phase != Some(phase)) {
            let _ = app.emit(STATE_EVENT, &snapshot);
            if models_revision != snapshot.models_revision {
                let _ = app.emit(MODELS_EVENT, &snapshot.models_revision);
                models_revision.clone_from(&snapshot.models_revision);
            }
            revision.clone_from(&snapshot.revision);
            previous_phase = Some(phase);
        }
        if snapshot.phase == dto::ServicePhase::Shutdown {
            break;
        }
    }
}

#[cfg(test)]
mod tests;
