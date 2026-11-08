//! Offline translation, a local model store, and optional catalog metadata fetching.
//!
//! See the crate README for artifact lifetime requirements and model fixtures.

#![doc = include_str!("../README.md")]

mod catalog;
mod core;
mod detect;
mod engine;
mod filesystem;
mod installation;
#[cfg(feature = "model-downloads")]
mod installer;
#[cfg(feature = "tauri-plugin")]
pub mod plugin;
mod store;
mod types;
#[cfg(feature = "workload")]
mod workload;

pub use catalog::*;
pub use core::TranslationCore;
pub use installation::RecoveryReport;
#[cfg(feature = "model-downloads")]
pub use installer::*;
pub use store::*;
pub use types::*;
#[cfg(feature = "workload")]
pub use workload::*;
#[cfg(any(feature = "catalog-http", feature = "model-downloads"))]
fn http_retry_after(headers: &reqwest::header::HeaderMap) -> std::time::Duration {
    use std::time::{Duration, SystemTime};
    headers
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| {
            value
                .parse::<u64>()
                .ok()
                .map(Duration::from_secs)
                .or_else(|| {
                    httpdate::parse_http_date(value)
                        .ok()
                        .map(|date| date.duration_since(SystemTime::now()).unwrap_or_default())
                })
        })
        .filter(|delay| std::time::Instant::now().checked_add(*delay).is_some())
        .unwrap_or_default()
}
