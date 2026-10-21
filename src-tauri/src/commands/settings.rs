//! Tauri command wrappers for the SQLite settings store.
//!
//! The React frontend still reads `localStorage` during the migration. These
//! commands let the Rust shell (and a one-shot migration in the existing UI)
//! read and write the same values in SQLite without changing any paths.

use plasma_store::SettingsStore;
use std::collections::BTreeMap;
use tauri::{AppHandle, Manager};

fn store(app: &AppHandle) -> SettingsStore {
    app.state::<SettingsStore>().inner().clone()
}

#[tauri::command]
pub fn settings_get(app: AppHandle, key: String) -> Result<Option<String>, String> {
    store(&app).get_raw(&key).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn settings_set(app: AppHandle, key: String, value: String) -> Result<(), String> {
    store(&app)
        .set_raw(&key, &value)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn settings_remove(app: AppHandle, key: String) -> Result<(), String> {
    store(&app)
        .remove_raw(&key)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn settings_snapshot(app: AppHandle) -> Result<BTreeMap<String, String>, String> {
    store(&app).snapshot().map_err(|error| error.to_string())
}

/// Imports values read from the legacy webview `localStorage`.
///
/// Existing SQLite values win, so this is safe to call on every start.
#[tauri::command]
pub fn settings_import_legacy(
    app: AppHandle,
    entries: Vec<(String, String)>,
) -> Result<usize, String> {
    store(&app)
        .import_legacy(entries)
        .map_err(|error| error.to_string())
}
