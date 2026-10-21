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
    let services = AppServices::new(env.clone(), settings, events, telegram, credentials);

    if std::env::args().any(|argument| argument == "--telegram-smoke") {
        let Some(credentials) = services.credentials.clone() else {
            return Err("credenciais do Telegram ausentes".to_string());
        };
        return run_telegram_smoke(&services, &credentials, &runtime);
    }

    plasma_telegram::start_plasma_media_http_server(env)
        .map_err(|error| format!("não foi possível iniciar o servidor de mídia: {error}"))?;

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
            _ => {}
        }
    }
}
