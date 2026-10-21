//! Plasma Hub Rust app shell.
//!
//! This is the Phase 2 desktop entry point. It resolves the same filesystem
//! layout as the Tauri app, opens the shared SQLite database and launches the
//! Dioxus WebView. The React app remains the default executable until the
//! Phase 8 cutover.

use plasma_core::{AppEnv, AppPaths};
use plasma_store::SettingsStore;
use plasma_ui::state::AppServices;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("plasma: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let paths = build_paths()?;
    let env = AppEnv::headless(paths);
    let settings = SettingsStore::open(&env).map_err(|error| error.to_string())?;
    let services = AppServices::new(env, settings);

    #[cfg(not(target_os = "android"))]
    {
        use dioxus::desktop::{Config, LogicalSize, WindowBuilder};

        let window = WindowBuilder::new()
            .with_title("Plasma Hub")
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(LogicalSize::new(960.0, 640.0))
            .with_always_on_top(false);
        let config = Config::new().with_window(window);

        dioxus::LaunchBuilder::desktop()
            .with_context(services)
            .with_cfg(config)
            .launch(plasma_ui::App);
    }

    #[cfg(target_os = "android")]
    {
        dioxus::LaunchBuilder::mobile()
            .with_context(services)
            .launch(plasma_ui::App);
    }

    Ok(())
}

/// Resolves the app-owned directories.
///
/// The defaults match Tauri's path resolver for the
/// `com.camacho.plasmahub` identifier. `PLASMA_HOME` is a development-only
/// override so the Rust shell can be smoke-tested without touching real data.
fn build_paths() -> Result<AppPaths, String> {
    if let Some(root) = std::env::var_os("PLASMA_HOME") {
        let root = PathBuf::from(root);
        return AppPaths::new(
            root.join("data"),
            root.join("local"),
            root.join("cache"),
            Some(root.join("downloads")),
        )
        .map_err(|error| format!("não foi possível criar PLASMA_HOME: {error}"));
    }

    const IDENTIFIER: &str = "com.camacho.plasmahub";
    let fallback = std::env::temp_dir();
    let data_base = dirs::data_dir().unwrap_or_else(|| fallback.clone());
    let local_base = dirs::data_local_dir().unwrap_or_else(|| data_base.clone());
    let cache_base = dirs::cache_dir().unwrap_or_else(|| fallback.clone());

    AppPaths::new(
        data_base.join(IDENTIFIER),
        local_base.join(IDENTIFIER),
        cache_base.join(IDENTIFIER),
        dirs::download_dir(),
    )
    .map_err(|error| format!("não foi possível criar os diretórios do app: {error}"))
}
