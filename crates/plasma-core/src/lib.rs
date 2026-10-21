//! Shared foundations for Plasma Hub crates.
//!
//! This crate intentionally knows nothing about the UI toolkit, Tauri or
//! Dioxus. It only exposes the app environment (paths + event sink) and the
//! error type used across the domain crates.

pub mod env;
pub mod error;
pub mod events;

pub use env::{AppEnv, AppPaths, EventEmitter, NoopEmitter};
pub use error::{Error, Result};
pub use events::{AppEvent, EventBus};
