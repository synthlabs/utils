//! Offline translation, a local model store, and optional catalog metadata fetching.
//!
//! See the crate README for artifact lifetime requirements and model fixtures.

#![doc = include_str!("../README.md")]

mod catalog;
mod core;
mod detect;
mod engine;
mod filesystem;
mod store;
mod types;

pub use catalog::*;
pub use core::TranslationCore;
pub use store::*;
pub use types::*;
