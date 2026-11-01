//! Minimal consuming application used to verify build registration and real command routing.

use synth_translate::plugin::{self, PluginConfig};
use tauri::{App, Runtime};

pub fn build<R: Runtime>(builder: tauri::Builder<R>, config: PluginConfig) -> App<R> {
    builder
        .plugin(plugin::init(config))
        .build(tauri::generate_context!())
        .expect("build translation test consumer")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use synth_translate::{
        plugin::{dto::ServicePhase, TranslationExt},
        Language, TranslationRequest,
    };
    use tauri::{
        test::{get_ipc_response, mock_builder, MockRuntime, INVOKE_KEY},
        webview::InvokeRequest,
        WebviewWindow,
    };

    fn fixture() -> (App<MockRuntime>, tempfile::TempDir) {
        let directory = tempfile::tempdir().unwrap();
        let app = build(
            mock_builder(),
            PluginConfig {
                store_root: Some(directory.path().into()),
                test_origin: Some("http://127.0.0.1:0".into()),
                ..PluginConfig::default()
            },
        );
        tauri::async_runtime::block_on(app.translation().initialize()).unwrap();
        (app, directory)
    }

    fn window(
        app: &App<MockRuntime>,
        label: &str,
        root: &std::path::Path,
    ) -> WebviewWindow<MockRuntime> {
        tauri::WebviewWindowBuilder::new(app, label, tauri::WebviewUrl::default())
            .data_directory(root.join("webviews").join(label))
            .build()
            .unwrap()
    }

    fn invoke(
        window: &WebviewWindow<MockRuntime>,
        command: &str,
        args: Value,
    ) -> Result<Value, Value> {
        get_ipc_response(
            window,
            InvokeRequest {
                cmd: format!("plugin:translate|{command}"),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "http://tauri.localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.into(),
            },
        )
        .map(|body| body.deserialize().unwrap())
    }

    #[test]
    fn permissions_route_commands_and_reject_unprivileged_windows() {
        let (app, directory) = fixture();
        let main = window(&app, "main", directory.path());
        let denied = window(&app, "unprivileged", directory.path());
        assert_eq!(
            invoke(&main, "status", json!({})).unwrap()["phase"],
            "ready"
        );
        assert!(invoke(&denied, "status", json!({}))
            .unwrap_err()
            .as_str()
            .unwrap()
            .contains("not allowed"));
        assert!(std::sync::Arc::ptr_eq(
            &app.translation(),
            &main.translation()
        ));
        app.translation().request_shutdown();
    }

    #[test]
    fn native_and_frontend_calls_share_release_resume_and_translation() {
        let (app, directory) = fixture();
        let main = window(&app, "main", directory.path());
        let id = invoke(
            &main,
            "submit_translation",
            json!({"request":{"text":" ","target":"eng"}}),
        )
        .unwrap();
        assert_eq!(
            invoke(&main, "translation_result", json!({"id":id})).unwrap(),
            json!({"kind":"skipped","reason":{"kind":"blank_input"}})
        );
        assert_eq!(
            invoke(&main, "release", json!({})).unwrap()["phase"],
            "paused"
        );
        assert_eq!(app.translation().snapshot().phase, ServicePhase::Paused);
        assert_eq!(
            invoke(
                &main,
                "submit_translation",
                json!({"request":{"text":"x","target":"eng"}})
            )
            .unwrap_err()["code"],
            "paused"
        );
        invoke(&main, "resume", json!({})).unwrap();
        let outcome = tauri::async_runtime::block_on(async {
            app.translation()
                .submit(TranslationRequest::new(" ", Language::ENGLISH))
                .await
                .unwrap()
                .result()
                .await
        })
        .unwrap();
        assert!(matches!(
            outcome,
            synth_translate::TranslationOutcome::Skipped(_)
        ));
        app.translation().request_shutdown();
    }

    #[test]
    fn missing_arguments_reject_as_strings_and_backend_validation_is_structured() {
        let (app, directory) = fixture();
        let main = window(&app, "main", directory.path());
        assert!(invoke(&main, "submit_translation", json!({}))
            .unwrap_err()
            .is_string());
        let error = invoke(
            &main,
            "submit_translation",
            json!({"request":{"text":"x","target":"en"}}),
        )
        .unwrap_err();
        assert_eq!(error["code"], "invalid_language");
        assert!(error["message"].is_string());
        app.translation().request_shutdown();
    }

    #[test]
    fn unavailable_store_does_not_fail_application_setup() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("blocked");
        std::fs::write(&root, b"obstruction").unwrap();
        let app = build(
            mock_builder(),
            PluginConfig {
                store_root: Some(root),
                test_origin: Some("http://127.0.0.1:0".into()),
                ..PluginConfig::default()
            },
        );
        assert!(tauri::async_runtime::block_on(app.translation().initialize()).is_err());
        assert_eq!(
            app.translation().snapshot().phase,
            ServicePhase::Unavailable
        );
        let main = window(&app, "main", directory.path());
        assert_eq!(
            invoke(&main, "status", json!({})).unwrap()["phase"],
            "unavailable"
        );
        app.translation().request_shutdown();
    }
}
