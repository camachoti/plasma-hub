//! Plasma Hub Rust UI shell (Dioxus 0.7 WebView renderer).
//!
//! The shell owns presentation and local UI state. Data, downloads and
//! Telegram live in the UI-agnostic crates and reach this layer through
//! [`state::AppServices`] and [`plasma_core::EventBus`].

pub mod context;
pub mod downloads;
pub mod icons;
pub mod state;
pub mod theme;
pub mod twitter;
pub mod views;

use dioxus::prelude::*;
use downloads::{
    apply_download_done, apply_download_error, apply_download_progress, persist_downloads,
    DownloadRecord,
};
use plasma_core::AppEvent;
use plasma_duplicates::SearchMatch;
use plasma_store::SettingKey;
use plasma_telegram::{TdlibChatInfo, TdlibDownloadItem, TdlibForumTopicInfo, TdlibMessageInfo};
use state::{
    apply_auth_error, apply_auth_status, bump_telegram_tick, mark_deleted_messages, AppServices,
    DuplicateSearchProgress, ForwardState, LightboxState, MassDownloadProgress, Tab, UiState,
};
use theme::{Density, Palette};
use views::{
    context_menu::ContextMenu,
    downloads::DownloadsView,
    login::LoginView,
    palette::CommandPalette,
    select::{SelectMenu, SelectPortal},
    settings::SettingsView,
    telegram::TelegramView,
    toasts::ToastStack,
    twitter::TwitterView,
};

const SHELL_CSS: &str = include_str!("../assets/shell.css");

/// Root component. State is created once and shared through context.
#[component]
pub fn App() -> Element {
    let services = use_context::<AppServices>();
    let settings = services.settings.clone();
    let skip_login = settings.get_bool(SettingKey::SKIP_LOGIN).unwrap_or(false);

    let tab = use_signal(|| {
        if services.initial_shared_url.is_some() {
            Tab::Downloads
        } else {
            Tab::Telegram
        }
    });
    let palette = use_signal(|| {
        Palette::from_stored(settings.get(SettingKey::APPEARANCE_PALETTE).ok().flatten())
    });
    let density = use_signal(|| {
        Density::from_stored(settings.get(SettingKey::APPEARANCE_DENSITY).ok().flatten())
    });
    let logged_in = use_signal(|| skip_login);
    let show_spike = use_signal(|| false);
    let show_palette = use_signal(|| false);
    let toasts = use_signal(Vec::new);

    let auth_state = use_signal(|| "not_initialized".to_string());
    let auth_error = use_signal(|| None::<String>);
    let auth_user = use_signal(|| None::<String>);
    let booting = use_signal(|| !skip_login);
    let preview_mode = use_signal(|| skip_login);
    let avatar = use_signal(|| None::<String>);

    let chats = use_signal(Vec::<TdlibChatInfo>::new);
    let chats_loading = use_signal(|| false);
    let chats_error = use_signal(|| None::<String>);
    let selected_chat = use_signal(|| None::<TdlibChatInfo>);

    let messages = use_signal(Vec::<TdlibMessageInfo>::new);
    let messages_loading = use_signal(|| false);
    let messages_error = use_signal(|| None::<String>);
    let messages_has_more = use_signal(|| false);
    let messages_key = use_signal(|| None::<String>);

    let topics = use_signal(Vec::<TdlibForumTopicInfo>::new);
    let selected_topic = use_signal(|| None::<TdlibForumTopicInfo>);
    let hidden_topics = use_signal(Vec::<String>::new);
    let topic_browser_open = use_signal(|| false);
    let thumbnails = use_signal(std::collections::HashMap::<(i64, i64), String>::new);

    let search_results = use_signal(Vec::<TdlibMessageInfo>::new);
    let search_loading = use_signal(|| false);
    let search_error = use_signal(|| None::<String>);

    let telegram_tick = use_signal(|| 0u64);
    let downloads = use_signal(Vec::<DownloadRecord>::new);
    let mass_download_running = use_signal(|| false);
    let mass_download_stopping = use_signal(|| false);
    let mass_download_progress = use_signal(|| None::<MassDownloadProgress>);
    let mass_download_open = use_signal(|| false);
    let duplicate_search_progress = use_signal(|| None::<DuplicateSearchProgress>);
    let context_menu = use_signal(|| None::<context::ContextMenuState>);
    let select_portal = use_signal(|| None::<SelectPortal>);
    let reply_to = use_signal(|| None::<TdlibMessageInfo>);
    let forward_state = use_signal(|| None::<ForwardState>);
    let lightbox = use_signal(|| None::<LightboxState>);
    let shared_url = use_signal(|| services.initial_shared_url.clone());

    let mut ui = use_context_provider(|| UiState {
        tab,
        palette,
        density,
        logged_in,
        show_spike,
        show_palette,
        toasts,
        auth_state,
        auth_error,
        auth_user,
        booting,
        preview_mode,
        avatar,
        chats,
        chats_loading,
        chats_error,
        selected_chat,
        messages,
        messages_loading,
        messages_error,
        messages_has_more,
        messages_key,
        topics,
        selected_topic,
        hidden_topics,
        topic_browser_open,
        thumbnails,
        search_results,
        search_loading,
        search_error,
        telegram_tick,
        downloads,
        mass_download_running,
        mass_download_stopping,
        mass_download_progress,
        mass_download_open,
        duplicate_search_progress,
        context_menu,
        select_portal,
        reply_to,
        forward_state,
        lightbox,
        shared_url,
    });

    // Restore the persisted download history once. The React frontend uses the
    // same SQLite row, so downloads started by either shell remain visible.
    let history_services = services.clone();
    use_future(move || {
        let history_services = history_services.clone();
        async move {
            if let Ok(items) = history_services.downloads.load_history().await {
                let mut existing = ui.downloads();
                for item in items {
                    if !existing.iter().any(|entry| entry.id == item.id) {
                        existing.push(item);
                    }
                }
                existing.sort_by_key(|item| item.created_at.unwrap_or_default());
                ui.downloads.set(existing);
            }
        }
    });

    // Bootstrap TDLib once. The login view reflects `auth_state` while this
    // runs; skip-login users keep the preview dashboard available immediately.
    let init_services = services.clone();
    use_future(move || {
        let init_services = init_services.clone();
        async move {
            let Some(credentials) = init_services.credentials.clone() else {
                apply_auth_error(
                    ui,
                    "Credenciais do Telegram ausentes. Defina VITE_API_ID e VITE_API_HASH no .env \
                 e recompile o shell (npm run dev:ui), ou use a prévia sem conta."
                        .to_string(),
                );
                ui.booting.set(false);
                return;
            };

            ui.auth_state.set("initializing".to_string());
            match init_services
                .telegram
                .init(credentials.api_id, credentials.api_hash)
                .await
            {
                Ok(status) => apply_auth_status(ui, status),
                Err(error) => apply_auth_error(ui, error),
            }
            ui.booting.set(false);

            if ui.auth_state() == "ready" {
                refresh_identity(init_services.clone(), ui).await;
            }
        }
    });

    // TDLib events keep auth state, chats and timelines fresh without polling.
    let event_services = services.clone();
    use_future(move || {
        let event_services = event_services.clone();
        async move {
            let mut receiver = event_services.events.subscribe();
            loop {
                match receiver.recv().await {
                    Ok(AppEvent::TelegramAuth { state }) => {
                        let ready = state == "ready";
                        ui.auth_state.set(state.clone());
                        if ready {
                            ui.logged_in.set(true);
                            ui.preview_mode.set(false);
                            bump_telegram_tick(ui);
                            refresh_identity(event_services.clone(), ui).await;
                        } else if matches!(state.as_str(), "closed" | "logging_out") {
                            ui.logged_in.set(false);
                        }
                    }
                    Ok(AppEvent::TelegramNewMessage { .. }) => {
                        bump_telegram_tick(ui);
                    }
                    Ok(AppEvent::TelegramMessagesDeleted {
                        chat_id,
                        message_ids,
                    }) => {
                        mark_deleted_messages(ui, &message_ids);
                        let services = event_services.clone();
                        spawn(async move {
                            // Persist the marker before reloading, otherwise the
                            // network page can overwrite it with the old cache.
                            let _ = services
                                .telegram
                                .mark_messages_deleted(chat_id.to_string(), message_ids)
                                .await;
                            bump_telegram_tick(ui);
                        });
                    }
                    Ok(AppEvent::TelegramDownloadProgress {
                        chat_id,
                        total,
                        downloaded,
                        current_file,
                        is_scanning,
                        items,
                    }) => {
                        let items = items
                            .into_iter()
                            .filter_map(|item| {
                                serde_json::from_value::<TdlibDownloadItem>(item).ok()
                            })
                            .collect();
                        ui.mass_download_progress.set(Some(MassDownloadProgress {
                            chat_id,
                            total,
                            downloaded,
                            current_file,
                            is_scanning,
                            items,
                        }));
                    }
                    Ok(AppEvent::DuplicateSearchProgress {
                        run_id,
                        scanned,
                        analyzed,
                        matches,
                        unreadable,
                    }) => {
                        let matches = matches
                            .into_iter()
                            .filter_map(|item| serde_json::from_value::<SearchMatch>(item).ok())
                            .collect();
                        ui.duplicate_search_progress
                            .set(Some(DuplicateSearchProgress {
                                run_id,
                                scanned,
                                analyzed,
                                matches,
                                unreadable,
                            }));
                    }
                    Ok(AppEvent::DownloadProgress {
                        id,
                        downloaded_bytes,
                        total_bytes,
                        percent,
                    }) => {
                        apply_download_progress(ui, &id, downloaded_bytes, total_bytes, percent);
                    }
                    Ok(AppEvent::DownloadDone { id }) => {
                        apply_download_done(ui, &id);
                        persist_downloads(event_services.clone(), ui);
                    }
                    Ok(AppEvent::DownloadError { id, error }) => {
                        apply_download_error(ui, &id, error);
                        persist_downloads(event_services.clone(), ui);
                    }
                    Ok(AppEvent::SharedUrl { url }) => {
                        ui.shared_url.set(Some(url));
                        ui.tab.set(Tab::Downloads);
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    });

    // Global shortcuts live in the webview, so they work regardless of which
    // element currently has focus. The palette itself handles Enter/Escape
    // while its input is focused; this listener adds Ctrl/Cmd+K from anywhere.
    use_effect(move || {
        let mut ui = ui;
        spawn(async move {
            let mut eval = document::eval(
                r#"
                document.addEventListener("keydown", (event) => {
                    const key = event.key.toLowerCase();
                    if ((event.ctrlKey || event.metaKey) && key === "k") {
                        event.preventDefault();
                        dioxus.send("toggle");
                    } else if (event.key === "Escape") {
                        dioxus.send("close");
                    }
                });
                "#,
            );

            while let Ok(command) = eval.recv::<String>().await {
                match command.as_str() {
                    "toggle" => {
                        let next = !ui.show_palette();
                        ui.show_palette.set(next);
                    }
                    "close" => {
                        ui.show_palette.set(false);
                        ui.context_menu.set(None);
                    }
                    _ => {}
                }
            }
        });
    });

    let palette = ui.palette();
    let density = ui.density();
    let open_menu = ui.context_menu();

    rsx! {
        style { dangerous_inner_html: SHELL_CSS }
        div {
            class: "plasma-root",
            "data-palette": palette.as_str(),
            "data-density": density.as_str(),
            if ui.logged_in() {
                Shell {}
            } else {
                LoginView {}
            }
            CommandPalette {}
            if let Some(menu) = open_menu {
                ContextMenu {
                    entries: menu.entries,
                    x: menu.x,
                    y: menu.y,
                    on_select: {
                        let services = services.clone();
                        move |id: String| {
                            let Some(menu) = ui.context_menu() else {
                                return;
                            };
                            context::handle_context_action(
                                services.clone(),
                                ui,
                                &menu.target,
                                &id,
                            );
                        }
                    },
                    on_close: move |_| ui.context_menu.set(None),
                }
            }
            if let Some(portal) = ui.select_portal() {
                SelectMenu {
                    portal,
                    on_close: move |_| ui.select_portal.set(None),
                }
            }
            ToastStack {}
        }
    }
}

async fn refresh_identity(services: AppServices, mut ui: UiState) {
    match services.telegram.get_me().await {
        Ok(me) if me.success => {
            let name = [me.first_name, me.last_name]
                .into_iter()
                .flatten()
                .filter(|part| !part.trim().is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let display = if name.trim().is_empty() {
                me.phone_number
            } else {
                Some(name)
            };
            ui.auth_user.set(display);
        }
        Ok(me) => {
            if let Some(error) = me.error {
                ui.auth_error.set(Some(error));
            }
        }
        Err(error) => ui.auth_error.set(Some(error)),
    }
}

/// App chrome: rail, top bar and the active view.
#[component]
fn Shell() -> Element {
    let mut ui = use_context::<UiState>();
    let tab = ui.tab();
    let auth_user = ui.auth_user();
    let preview = ui.preview_mode();

    rsx! {
        div { class: "shell",
            nav { class: "rail",
                div { class: "rail-brand", title: "Plasma Hub", "P" }
                for item in Tab::ALL {
                    button {
                        key: "{item.label()}",
                        class: if tab == item { "rail-item active" } else { "rail-item" },
                        title: "{item.label()}",
                        "data-tip": "{item.label()}",
                        onclick: move |_| ui.tab.set(item),
                        "{item.rail_label()}"
                    }
                }
                div { class: "rail-spacer" }
                button {
                    class: "rail-item",
                    title: "Paleta de comandos",
                    "data-tip": "Paleta de comandos",
                    onclick: move |_| ui.show_palette.set(true),
                    "⌘"
                }
            }
            div { class: "shell-main",
                header { class: "topbar",
                    div { class: "topbar-title",
                        strong { "Plasma Hub" }
                        span { "{tab.label()}" }
                    }
                    div { class: "topbar-actions",
                        if preview {
                            span { class: "pill", "Prévia sem conta" }
                        }
                        if let Some(name) = auth_user {
                            span { class: "muted", "{name}" }
                        }
                        button {
                            class: "btn small ghost",
                            title: "Ctrl/Cmd + K",
                            onclick: move |_| ui.show_palette.set(true),
                            "⌘K  Comandos"
                        }
                        button {
                            class: "btn small ghost",
                            onclick: move |_| ui.logged_in.set(false),
                            "Sair"
                        }
                    }
                }
                div { class: "content",
                    {match tab {
                        Tab::Telegram => rsx! { TelegramView {} },
                        Tab::Downloads => rsx! { DownloadsView {} },
                        Tab::Twitter => rsx! { TwitterView {} },
                        Tab::Settings => rsx! { SettingsView {} },
                    }}
                }
            }
        }
    }
}
