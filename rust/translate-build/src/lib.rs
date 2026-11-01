//! Command names for a consuming application's `tauri_build::InlinedPlugin`.

#[macro_export]
macro_rules! translation_commands {
    ($callback:ident) => {
        $callback! {
            status, retry_initialize, release, resume, detect,
            submit_translation, translation_result, cancel_translation,
            catalog, refresh_catalog, inventory, verify_model, remove_model,
            discard_downloads, plan_install, start_install, installation, cancel_install
        }
    };
}

macro_rules! names {
    ($($name:ident),* $(,)?) => {
        pub const COMMANDS: &[&str] = &[$(stringify!($name)),*];
    };
}

translation_commands!(names);
