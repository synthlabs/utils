use tauri_build::{Attributes, DefaultPermissionRule, InlinedPlugin};

fn main() {
    let attributes = Attributes::new().plugin(
        "translate",
        InlinedPlugin::new()
            .commands(translate_build::COMMANDS)
            .default_permission(DefaultPermissionRule::AllowAllCommands),
    );
    tauri_build::try_build(attributes).expect("build translation consumer permissions");
}
