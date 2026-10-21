use plasma_core::{AppEnv, AppPaths, EventEmitter};
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, Runtime};

/// Forwards domain events to the Tauri event system.
///
/// The event names and payloads are unchanged, so the current React frontend
/// keeps working while the Rust UI is built in parallel.
pub struct TauriEmitter<R: Runtime> {
    handle: AppHandle<R>,
}

impl<R: Runtime> EventEmitter for TauriEmitter<R> {
    fn emit_json(&self, event: &str, payload: Value) {
        let _ = self.handle.emit(event, payload);
    }
}

/// Builds the shared [`AppEnv`] from Tauri's path resolver.
pub fn build_env<R: Runtime>(app: &AppHandle<R>) -> Result<AppEnv, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    let local_data_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?;
    let download_dir = app.path().download_dir().ok();

    let paths = AppPaths::new(data_dir, local_data_dir, cache_dir, download_dir)
        .map_err(|error| error.to_string())?;

    Ok(AppEnv::new(
        paths,
        Arc::new(TauriEmitter {
            handle: app.clone(),
        }),
    ))
}

/// Resolves the managed [`AppEnv`] for a command wrapper.
pub fn env<R: Runtime>(app: &AppHandle<R>) -> AppEnv {
    app.state::<AppEnv>().inner().clone()
}
