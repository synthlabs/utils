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
mod store;
mod types;

pub use catalog::*;
pub use core::TranslationCore;
pub use installation::RecoveryReport;
#[cfg(feature = "model-downloads")]
pub use installer::*;
pub use store::*;
pub use types::*;
