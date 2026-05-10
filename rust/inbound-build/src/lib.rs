use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub const COMMANDS: &[&str] = &[
    "capabilities",
    "preview_system",
    "preview_build",
    "preview_log_tail",
    "preview_config",
    "preview_error",
    "submit",
];

pub fn stamp() {
    let sha = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_owned());

    let build_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_owned());

    println!("cargo:rustc-env=INBOUND_GIT_SHA={sha}");
    println!("cargo:rustc-env=INBOUND_BUILD_TIME={build_time}");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
}
