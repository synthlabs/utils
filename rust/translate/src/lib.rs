//! Synchronous, offline translation with caller-owned model files.
//!
//! See the crate README for artifact lifetime requirements and model fixtures.

#![doc = include_str!("../README.md")]

mod core;
mod detect;
mod engine;
mod types;

pub use core::TranslationCore;
pub use types::*;
