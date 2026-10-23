//! Plasma Hub Rust app shell.
//!
//! This is the Rust entry point used alongside the React/Tauri app during the
//! migration. It resolves the same filesystem layout as Tauri, opens the shared
//! SQLite database, starts the loopback media server and launches the Dioxus
//! WebView with a Tokio runtime for TDLib.

use plasma_core::{AppEnv, AppEvent, AppPaths, EventBus, EventEmitter};
use plasma_store::SettingsStore;
use plasma_ui::state::{AppServices, TelegramCredentials, TelegramHandle};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

fn main() {
    if let Err(error) = run() {
        eprintln!("plasma: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let paths = build_paths()?;

    let events = EventBus::default();
    let emitter: Arc<dyn EventEmitter> = Arc::new(BusEmitter {
        bus: events.clone(),
    });
    let env = AppEnv::new(paths, emitter);
    install_panic_logger(&env);

    // WebKitGTK keeps its disk cache in `$XDG_CACHE_HOME/<app-name>`. A cache
    // left behind by an interrupted run can leave the web process spinning
    // without ever painting the page, which shows up as an empty window. The
    // shell serves embedded assets and does not need a persistent web cache,
    // so isolate it in the app cache and start every launch from a clean one.
    #[cfg(target_os = "linux")]
    prepare_webview_cache(&env)?;

    let settings = SettingsStore::open(&env).map_err(|error| error.to_string())?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("não foi possível iniciar o runtime Tokio: {error}"))?;
    let telegram = TelegramHandle::new(env.clone(), runtime.handle().clone());
    let credentials = telegram_credentials();
    #[cfg(debug_assertions)]
    eprintln!(
        "plasma: telegram credentials {}",
        if credentials.is_some() {
            "configured"
        } else {
            "missing"
        }
    );
    let mut services = AppServices::new(env.clone(), settings, events, telegram, credentials);
    services.initial_shared_url = plasma_platform::take_initial_shared_url()
        .or_else(plasma_platform::take_pending_shared_url);

    let share_events = services.events.clone();
    plasma_platform::set_share_handler(Box::new(move |url| {
        share_events.publish(AppEvent::SharedUrl { url });
    }));

    if std::env::args().any(|argument| argument == "--telegram-smoke") {
        let Some(credentials) = services.credentials.clone() else {
            return Err("credenciais do Telegram ausentes".to_string());
        };
        return run_telegram_smoke(&services, &credentials, &runtime);
    }

    plasma_telegram::start_plasma_media_http_server(env)
        .map_err(|error| format!("não foi possível iniciar o servidor de mídia: {error}"))?;

    // Apply the configured media cache limit even when the user did not change
    // the setting in this session.
    let cache_services = services.clone();
    runtime.spawn(async move {
        let _ = cache_services.enforce_media_cache_limit().await;
    });

    // Keep the runtime alive for the whole WebView lifetime. TDLib's receiver
    // and the UI's async calls run on its worker threads.
    let _runtime = runtime;

    #[cfg(not(target_os = "android"))]
    {
        use dioxus::desktop::{Config, LogicalSize, WindowBuilder};

        let window = WindowBuilder::new()
            .with_title("Plasma Hub")
            .with_inner_size(LogicalSize::new(1280.0, 820.0))
            .with_min_inner_size(LogicalSize::new(960.0, 640.0))
            .with_always_on_top(false);
        // The default Dioxus menu bar (Window/Edit/Help) is a native GTK menu
        // that does not belong to the Plasma Hub interface, so it is removed.
        let config = Config::new().with_window(window).with_menu(None);

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

/// Prepares an app-owned WebKitGTK cache directory for this launch.
///
/// Clearing the directory is intentional: a partial or stale cache can make
/// WebKitGTK never finish its first paint, leaving the Dioxus window blank.
/// The content is embedded in the binary, so losing this cache has no user
/// data impact beyond regenerating it on the next page load.
#[cfg(target_os = "linux")]
fn prepare_webview_cache(env: &AppEnv) -> Result<(), String> {
    let root = env.paths().cache_dir().join("webview");

    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|error| {
            format!(
                "não foi possível limpar o cache do WebView em {}: {error}",
                root.display()
            )
        })?;
    }

    std::fs::create_dir_all(&root).map_err(|error| {
        format!(
            "não foi possível criar o cache do WebView em {}: {error}",
            root.display()
        )
    })?;

    // Must be set before WebKitGTK is initialized by Dioxus.
    std::env::set_var("XDG_CACHE_HOME", &root);
    Ok(())
}

fn run_telegram_smoke(
    services: &AppServices,
    credentials: &TelegramCredentials,
    runtime: &tokio::runtime::Runtime,
) -> Result<(), String> {
    let status = runtime.block_on(
        services
            .telegram
            .init(credentials.api_id, credentials.api_hash.clone()),
    )?;
    println!(
        "tdlib-smoke: success={} ready={} state={} error={}",
        status.success,
        status.ready,
        status.state,
        status.error.unwrap_or_default()
    );

    if status.ready {
        let chats = runtime.block_on(services.telegram.get_chats(20))?;
        println!(
            "tdlib-smoke: chats={} has_more={} error={}",
            chats.dialogs.len(),
            chats.has_more,
            chats.error.unwrap_or_default()
        );
    }

    Ok(())
}

/// Appends panic details to `crash.log` in the app's local data directory.
///
/// Dioxus/WebKit panics otherwise only reach stderr, which is usually lost when
/// the app is launched from a desktop menu or from an APK.
fn install_panic_logger(env: &AppEnv) {
    let path = env.paths().local_data_dir().join("crash.log");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or_default();
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let _ = writeln!(file, "[{timestamp}] {info}");
        }
        eprintln!("plasma: panic: {info}");
    }));
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

    #[cfg(target_os = "android")]
    {
        let dirs = plasma_platform::android_app_dirs()?;
        AppPaths::new(
            dirs.data_dir,
            dirs.local_data_dir,
            dirs.cache_dir,
            Some(dirs.download_dir),
        )
        .map_err(|error| format!("não foi possível criar os diretórios Android: {error}"))
    }

    #[cfg(not(target_os = "android"))]
    {
        const IDENTIFIER: &str = "com.camacho.plasmahub";
        let fallback = std::env::temp_dir();
        let data_base = dirs::data_dir().unwrap_or_else(|| fallback.clone());
        let local_base = dirs::data_local_dir().unwrap_or_else(|| fallback.clone());
        let cache_base = dirs::cache_dir().unwrap_or_else(|| fallback.clone());

        AppPaths::new(
            data_base.join(IDENTIFIER),
            local_base.join(IDENTIFIER),
            cache_base.join(IDENTIFIER),
            dirs::download_dir(),
        )
        .map_err(|error| format!("não foi possível criar os diretórios do app: {error}"))
    }
}

/// Reads the Telegram credentials embedded by `build.rs`.
///
/// The build script copies `VITE_API_ID`/`VITE_API_HASH` from `.env` (or the
/// `PLASMA_TELEGRAM_API_*` environment variables) so the Rust shell can reuse
/// the same Telegram application as the Tauri frontend without committing
/// secrets.
fn telegram_credentials() -> Option<TelegramCredentials> {
    let api_id = option_env!("PLASMA_TELEGRAM_API_ID")
        .unwrap_or_default()
        .trim()
        .parse::<i32>()
        .ok()?;
    let api_hash = option_env!("PLASMA_TELEGRAM_API_HASH")
        .unwrap_or_default()
        .trim()
        .to_string();

    if api_id <= 0 || api_hash.is_empty() {
        None
    } else {
        Some(TelegramCredentials { api_id, api_hash })
    }
}

/// Forwards the legacy named events emitted by `plasma-telegram` to the
/// UI-agnostic [`EventBus`]. The Tauri bridge keeps using its own emitter.
struct BusEmitter {
    bus: EventBus,
}

impl EventEmitter for BusEmitter {
    fn emit_json(&self, event: &str, payload: Value) {
        match event {
            "tdlib-auth-state" => {
                if let Some(state) = payload.as_str() {
                    self.bus.publish(AppEvent::TelegramAuth {
                        state: state.to_string(),
                    });
                }
            }
            "tdlib-new-message" => {
                let Some(chat_id) = payload.get("chatId").and_then(Value::as_i64) else {
                    return;
                };
                self.bus.publish(AppEvent::TelegramNewMessage {
                    chat_id,
                    topic_id: payload.get("topicId").and_then(Value::as_i64),
                    message: payload.get("message").cloned().unwrap_or(Value::Null),
                });
            }
            "tdlib-messages-deleted" => {
                let Some(chat_id) = payload.get("chatId").and_then(Value::as_i64) else {
                    return;
                };
                let message_ids = payload
                    .get("messageIds")
                    .and_then(Value::as_array)
                    .map(|ids| ids.iter().filter_map(Value::as_i64).collect())
                    .unwrap_or_default();
                self.bus.publish(AppEvent::TelegramMessagesDeleted {
                    chat_id,
                    message_ids,
                });
            }
            "tdlib-download-progress" => {
                let Some(chat_id) = payload.get("chatId").and_then(Value::as_i64) else {
                    return;
                };
                self.bus.publish(AppEvent::TelegramDownloadProgress {
                    chat_id,
                    total: payload
                        .get("total")
                        .and_then(Value::as_u64)
                        .unwrap_or_default() as usize,
                    downloaded: payload
                        .get("downloaded")
                        .and_then(Value::as_f64)
                        .unwrap_or_default(),
                    current_file: payload
                        .get("currentFile")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                    is_scanning: payload
                        .get("isScanning")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    items: payload
                        .get("items")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                });
            }
            "duplicate-media-search-progress" => {
                let Some(run_id) = payload.get("runId").and_then(Value::as_str) else {
                    return;
                };
                self.bus.publish(AppEvent::DuplicateSearchProgress {
                    run_id: run_id.to_string(),
                    scanned: payload
                        .get("scanned")
                        .and_then(Value::as_u64)
                        .unwrap_or_default(),
                    analyzed: payload
                        .get("analyzed")
                        .and_then(Value::as_u64)
                        .unwrap_or_default(),
                    matches: payload
                        .get("matches")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                    unreadable: payload
                        .get("unreadable")
                        .and_then(Value::as_u64)
                        .unwrap_or_default(),
                });
            }
            "telegram-media-progress" => {
                let Some(chat_id) = payload.get("chatId").and_then(Value::as_i64) else {
                    return;
                };
                let Some(message_id) = payload.get("messageId").and_then(Value::as_i64) else {
                    return;
                };
                let downloaded = payload
                    .get("downloadedBytes")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                let total = payload
                    .get("totalBytes")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                let state = payload
                    .get("stage")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.bus.publish(AppEvent::MediaProgress {
                    chat_id,
                    message_id,
                    downloaded_bytes: downloaded,
                    total_bytes: total,
                    state,
                });
            }
            "native-file-download-progress" => {
                let Some(id) = payload.get("id").and_then(Value::as_str) else {
                    return;
                };
                let downloaded = payload
                    .get("downloadedBytes")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                let total = payload
                    .get("totalBytes")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                let percent = payload.get("percent").and_then(Value::as_f64);
                self.bus.publish(AppEvent::DownloadProgress {
                    id: id.to_string(),
                    downloaded_bytes: downloaded,
                    total_bytes: total,
                    percent,
                });
            }
            "twitter-download-progress"
            | "youtube-download-progress"
            | "tiktok-download-progress" => {
                let Some(id) = payload.get("id").and_then(Value::as_str) else {
                    return;
                };
                let progress = payload
                    .get("progress")
                    .and_then(Value::as_f64)
                    .unwrap_or_default()
                    .clamp(0.0, 100.0);
                self.bus.publish(AppEvent::DownloadProgress {
                    id: id.to_string(),
                    downloaded_bytes: 0,
                    total_bytes: 0,
                    percent: Some(progress),
                });
            }
            "twitter-download-done" | "youtube-download-done" | "tiktok-download-done" => {
                if let Some(id) = payload.get("id").and_then(Value::as_str) {
                    self.bus
                        .publish(AppEvent::DownloadDone { id: id.to_string() });
                }
            }
            "twitter-download-error" | "youtube-download-error" | "tiktok-download-error" => {
                let Some(id) = payload.get("id").and_then(Value::as_str) else {
                    return;
                };
                let error = payload
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("Falha no download da plataforma.")
                    .to_string();
                self.bus.publish(AppEvent::DownloadError {
                    id: id.to_string(),
                    error,
                });
            }
            _ => {}
        }
    }
}
