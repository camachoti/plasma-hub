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
pub mod twitter_fake;
pub mod views;

use dioxus::prelude::*;
use downloads::{
    apply_download_done, apply_download_error, apply_download_progress,
    apply_telegram_media_progress, persist_downloads, sync_telegram_downloads, DownloadRecord,
};
use plasma_core::AppEvent;
use plasma_duplicates::SearchMatch;
use plasma_store::SettingKey;
use plasma_telegram::{TdlibChatInfo, TdlibDownloadItem, TdlibForumTopicInfo, TdlibMessageInfo};
use state::{
    apply_auth_error, apply_auth_status, apply_chat_patch, apply_live_message,
    apply_message_content, bump_telegram_tick, clear_cached_chat_list, mark_deleted_messages,
    recall_cached_chat_list, set_drawer_open, AppServices, ConversationState, DrawerTab,
    DuplicateSearchMedia, DuplicateSearchProgress, ForwardState, LightboxState,
    MassDownloadProgress, PanelState, ReactionPickerState, SessionState, ShellState, Tab,
    ToastKind, TransferState, UiState, UserMediaState,
};
use theme::{ColorMode, Density, UiStyle};
use views::{
    context_menu::ContextMenu,
    login::LoginView,
    palette::CommandPalette,
    reaction_picker::ReactionPicker,
    select::{SelectMenu, SelectPortal},
    telegram::TelegramView,
    toasts::ToastStack,
};

const SHELL_CSS: &str = concat!(
    include_str!("../assets/tokens.css"),
    "\n",
    include_str!("../assets/base.css"),
    "\n",
    include_str!("../assets/layout.css"),
    "\n",
    include_str!("../assets/messenger.css"),
    "\n",
    include_str!("../assets/views.css"),
);

/// Root component. State is created once and shared through context.
#[component]
pub fn App() -> Element {
    let services = use_context::<AppServices>();
    let settings = services.settings.clone();
    let skip_login = settings.get_bool(SettingKey::SKIP_LOGIN).unwrap_or(false);

    // Android share intents must reach Downloads without waiting for Telegram login.
    let open_from_share = services.initial_shared_url.is_some();
    let tab = use_signal(|| {
        if open_from_share {
            Tab::Downloads
        } else {
            Tab::Telegram
        }
    });
    let style = use_signal(|| UiStyle::load(&settings));
    let color_mode = use_signal(|| ColorMode::load(&settings));
    let density = use_signal(|| {
        Density::from_stored(settings.get(SettingKey::APPEARANCE_DENSITY).ok().flatten())
    });
    let sidebar_open = use_signal(|| true);
    let drawer_open = use_signal(|| {
        settings
            .get_bool(SettingKey::INFO_PANEL_OPEN)
            .unwrap_or(false)
    });
    let drawer_tab = use_signal(|| DrawerTab::Queue);
    let logged_in = use_signal(|| skip_login || open_from_share);
    let show_spike = use_signal(|| false);
    let show_palette = use_signal(|| false);
    let toasts = use_signal(Vec::new);

    let auth_state = use_signal(|| "not_initialized".to_string());
    let auth_error = use_signal(|| None::<String>);
    let auth_user = use_signal(|| None::<String>);
    let booting = use_signal(|| !skip_login);
    let preview_mode = use_signal(|| skip_login || open_from_share);
    let avatar = use_signal(|| None::<String>);

    let chats = use_signal(|| {
        if skip_login {
            Vec::new()
        } else {
            recall_cached_chat_list(&settings)
        }
    });
    let chats_loading = use_signal(|| false);
    let chats_error = use_signal(|| None::<String>);
    let selected_chat = use_signal(|| None::<TdlibChatInfo>);
    let mobile_show_list = use_signal(|| true);

    let messages = use_signal(Vec::<TdlibMessageInfo>::new);
    let messages_loading = use_signal(|| false);
    let messages_error = use_signal(|| None::<String>);
    let messages_has_more = use_signal(|| false);
    let messages_key = use_signal(|| None::<String>);

    let topics = use_signal(Vec::<TdlibForumTopicInfo>::new);
    let selected_topic = use_signal(|| None::<TdlibForumTopicInfo>);
    let hidden_topics = use_signal(Vec::<String>::new);
    let topic_browser_open = use_signal(|| false);
    // Per-chat topic listings fill instantly on revisit while a fresh copy
    // loads in the background.
    let topics_cache =
        use_signal(std::collections::HashMap::<String, Vec<TdlibForumTopicInfo>>::new);
    let topics_with_new_media = use_signal(std::collections::HashSet::<String>::new);
    let twitter_gallery_badges = use_signal(std::collections::HashMap::<String, usize>::new);
    let twitter_sync_status = use_signal(|| None::<state::TwitterGallerySyncStatus>);
    let thumbnails = use_signal(std::collections::HashMap::<(i64, i64), String>::new);

    let search_results = use_signal(Vec::<TdlibMessageInfo>::new);
    let search_loading = use_signal(|| false);
    let search_error = use_signal(|| None::<String>);

    let telegram_tick = use_signal(|| 0u64);
    let message_tick = use_signal(|| 0u64);
    let downloads = use_signal(Vec::<DownloadRecord>::new);
    let mass_download_running = use_signal(|| false);
    let mass_download_stopping = use_signal(|| false);
    let mass_download_progress = use_signal(|| None::<MassDownloadProgress>);
    let mass_download_open = use_signal(|| false);
    // Topic pre-selected from the topic context menu; None means the panel
    // falls back to the currently selected topic.
    let mass_download_topic = use_signal(|| None::<TdlibForumTopicInfo>);
    let duplicate_search_progress = use_signal(|| None::<DuplicateSearchProgress>);
    let duplicate_search = use_signal(|| None::<Vec<DuplicateSearchMedia>>);
    let context_menu = use_signal(|| None::<context::ContextMenuState>);
    let select_portal = use_signal(|| None::<SelectPortal>);
    let reply_to = use_signal(|| None::<TdlibMessageInfo>);
    let forward_state = use_signal(|| None::<ForwardState>);
    let lightbox = use_signal(|| None::<LightboxState>);
    let user_media = use_signal(|| None::<UserMediaState>);
    let reaction_picker = use_signal(|| None::<ReactionPickerState>);
    let shared_url = use_signal(|| services.initial_shared_url.clone());

    let mut ui = use_context_provider(|| UiState {
        shell: ShellState {
            tab,
            style,
            color_mode,
            density,
            sidebar_open,
            drawer_open,
            drawer_tab,
            logged_in,
            show_spike,
            show_palette,
            toasts,
            select_portal,
            shared_url,
        },
        session: SessionState {
            auth_state,
            auth_error,
            auth_user,
            booting,
            preview_mode,
            avatar,
        },
        conversation: ConversationState {
            chats,
            chats_loading,
            chats_error,
            selected_chat,
            mobile_show_list,
            messages,
            messages_loading,
            messages_error,
            messages_has_more,
            messages_key,
            topics,
            selected_topic,
            hidden_topics,
            topic_browser_open,
            topics_cache,
            topics_with_new_media,
            twitter_gallery_badges,
            twitter_sync_status,
            thumbnails,
            search_results,
            search_loading,
            search_error,
            telegram_tick,
            message_tick,
        },
        transfers: TransferState {
            downloads,
            mass_download_running,
            mass_download_stopping,
            mass_download_progress,
            mass_download_open,
            mass_download_topic,
            duplicate_search_progress,
            duplicate_search,
        },
        panels: PanelState {
            context_menu,
            reply_to,
            forward_state,
            lightbox,
            user_media,
            reaction_picker,
        },
    });

    // Restore the persisted download history once. The React frontend uses the
    // same SQLite row, so downloads started by either shell remain visible.
    let history_services = services.clone();
    use_future(move || {
        let history_services = history_services.clone();
        async move {
            if let Ok(items) = history_services.downloads.load_history().await {
                let mut existing = ui.transfers.downloads();
                for item in items {
                    if !existing.iter().any(|entry| entry.id == item.id) {
                        existing.push(item);
                    }
                }
                existing.sort_by_key(|item| item.created_at.unwrap_or_default());
                ui.transfers.downloads.set(existing);
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
                    "Credenciais do Telegram ausentes. Defina PLASMA_TELEGRAM_API_ID e \
                 PLASMA_TELEGRAM_API_HASH (ou VITE_API_ID/VITE_API_HASH) no .env e recompile \
                 com cargo build -p plasma, ou use a prévia sem conta."
                        .to_string(),
                );
                ui.session.booting.set(false);
                return;
            };

            ui.session.auth_state.set("initializing".to_string());
            match init_services
                .telegram
                .init(credentials.api_id, credentials.api_hash)
                .await
            {
                Ok(status) => apply_auth_status(ui, status),
                Err(error) => apply_auth_error(ui, error),
            }
            ui.session.booting.set(false);

            if ui.session.auth_state() == "ready" {
                refresh_identity(init_services.clone(), ui).await;
            }
        }
    });

    // One-shot import of synthetic Twitter galleries left in the webview.
    let migrate_services = services.clone();
    use_future(move || {
        let migrate_services = migrate_services.clone();
        async move {
            match crate::twitter_fake::migrate_twitter_fake_chats_from_local_storage(
                &migrate_services.settings,
            )
            .await
            {
                Ok(0) => {}
                Ok(added) => {
                    let mut chats = ui.conversation.chats();
                    crate::twitter_fake::merge_fake_dialogs_into(
                        &mut chats,
                        &migrate_services.settings,
                    );
                    ui.conversation.chats.set(chats);
                    state::push_toast(
                        ui.shell.toasts,
                        format!("{added} item(ns) Twitter/X importado(s) do app antigo."),
                        ToastKind::Success,
                    );
                }
                Err(_) => {
                    // localStorage may be unavailable outside the WebView; ignore.
                }
            }
        }
    });

    // Periodic append-only refresh of followed Twitter media galleries.
    let poller_services = services.clone();
    use_future(move || {
        let poller_services = poller_services.clone();
        async move {
            const INTERVAL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
            loop {
                tokio::time::sleep(INTERVAL).await;
                let chats = crate::twitter_fake::read_twitter_fake_chats(&poller_services.settings);
                if chats.is_empty() {
                    continue;
                }
                let cookies = poller_services
                    .twitter
                    .load_cookies()
                    .await
                    .unwrap_or_default();
                let cookie_value = {
                    let trimmed = cookies.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_string())
                };
                for chat in chats {
                    let url = if chat.original_url.trim().is_empty() {
                        format!("https://x.com/{}", chat.username)
                    } else {
                        chat.original_url.clone()
                    };
                    let profile = match poller_services
                        .twitter
                        .analyze_profile(url, cookie_value.clone())
                        .await
                    {
                        Ok(profile) => profile,
                        Err(error) => {
                            if ui
                                .conversation
                                .selected_chat()
                                .is_some_and(|selected| selected.id == chat.id)
                            {
                                ui.conversation.twitter_sync_status.set(Some(
                                    state::TwitterGallerySyncStatus {
                                        chat_id: chat.id.clone(),
                                        last_at_ms: None,
                                        last_error: Some(error.to_string()),
                                        checking: false,
                                    },
                                ));
                            }
                            continue;
                        }
                    };
                    let Ok(result) = crate::twitter_fake::update_from_profile(
                        &poller_services.settings,
                        &profile,
                    ) else {
                        continue;
                    };
                    let open = ui
                        .conversation
                        .selected_chat()
                        .is_some_and(|selected| selected.id == result.chat_id);
                    if open {
                        ui.conversation.twitter_sync_status.set(Some(
                            state::TwitterGallerySyncStatus {
                                chat_id: result.chat_id.clone(),
                                last_at_ms: Some(state::now_millis()),
                                last_error: None,
                                checking: false,
                            },
                        ));
                    }
                    if result.added_count == 0 {
                        continue;
                    }
                    let mut chats = ui.conversation.chats();
                    crate::twitter_fake::merge_fake_dialogs_into(
                        &mut chats,
                        &poller_services.settings,
                    );
                    ui.conversation.chats.set(chats);
                    if open {
                        if let Some(fake) = crate::twitter_fake::get_twitter_fake_chat(
                            &poller_services.settings,
                            &result.chat_id,
                        ) {
                            let thumbs = fake.thumbnail_seed();
                            if !thumbs.is_empty() {
                                let mut cache = ui.conversation.thumbnails.write();
                                for (key, url) in thumbs {
                                    cache.insert(key, url);
                                }
                            }
                            ui.conversation.messages.set(fake.messages_as_tdlib());
                        }
                        ui.conversation
                            .twitter_gallery_badges
                            .write()
                            .remove(&result.chat_id);
                    } else {
                        let mut badges = ui.conversation.twitter_gallery_badges();
                        let entry = badges.entry(result.chat_id.clone()).or_insert(0);
                        *entry = entry.saturating_add(result.added_count);
                        ui.conversation.twitter_gallery_badges.set(badges);
                    }
                    state::push_toast(
                        ui.shell.toasts,
                        format!(
                            "{} nova(s) mídia(s) em @{}",
                            result.added_count, chat.username
                        ),
                        ToastKind::Info,
                    );
                }
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
                        ui.session.auth_state.set(state.clone());
                        if ready {
                            ui.shell.logged_in.set(true);
                            ui.session.preview_mode.set(false);
                            // Refresh the chat list; leave the open timeline alone.
                            // Opening a chat already loads its history.
                            bump_telegram_tick(ui);
                            refresh_identity(event_services.clone(), ui).await;
                        } else if matches!(state.as_str(), "closed" | "logging_out") {
                            ui.shell.logged_in.set(false);
                            clear_cached_chat_list(&event_services.settings);
                            ui.conversation.chats.set(Vec::new());
                            ui.conversation.selected_chat.set(None);
                        }
                    }
                    Ok(AppEvent::TelegramNewMessage {
                        chat_id, message, ..
                    }) => match serde_json::from_value::<TdlibMessageInfo>(message) {
                        Ok(message) => {
                            apply_live_message(&event_services, ui, chat_id, message);
                        }
                        Err(_) => bump_telegram_tick(ui),
                    },
                    Ok(AppEvent::TelegramMessageUpdated {
                        chat_id,
                        message_id,
                        text,
                        has_media,
                        is_photo,
                        is_video,
                    }) => {
                        apply_message_content(
                            ui, chat_id, message_id, text, has_media, is_photo, is_video,
                        );
                    }
                    Ok(AppEvent::TelegramChatPatch {
                        chat_id,
                        title,
                        unread_count,
                    }) => {
                        apply_chat_patch(ui, chat_id, title, unread_count);
                    }
                    Ok(AppEvent::TelegramMessagesDeleted {
                        chat_id,
                        message_ids,
                    }) => {
                        mark_deleted_messages(ui, &message_ids);
                        let services = event_services.clone();
                        spawn(async move {
                            let _ = services
                                .telegram
                                .mark_messages_deleted(chat_id.to_string(), message_ids)
                                .await;
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
                            .collect::<Vec<_>>();
                        let chat_title = ui
                            .conversation
                            .chats()
                            .into_iter()
                            .find(|chat| chat.id == chat_id.to_string())
                            .map(|chat| chat.title);
                        sync_telegram_downloads(
                            event_services.clone(),
                            ui,
                            chat_id,
                            chat_title,
                            total,
                            downloaded,
                            &items,
                        );
                        let finished = mass_download_finished(&current_file, is_scanning);
                        ui.transfers
                            .mass_download_progress
                            .set(Some(MassDownloadProgress {
                                chat_id,
                                total,
                                downloaded,
                                current_file,
                                is_scanning,
                                items,
                            }));
                        // Clear running here so a stuck JoinHandle await on the
                        // Dioxus executor cannot leave "Parar download" locked.
                        if finished {
                            ui.transfers.mass_download_running.set(false);
                            ui.transfers.mass_download_stopping.set(false);
                        }
                    }
                    Ok(AppEvent::DuplicateSearchProgress {
                        run_id,
                        scanned,
                        analyzed,
                        matches,
                        unreadable,
                    }) => {
                        // Backend now sends the cumulative match list; replace
                        // is safe and keeps the dialog in sync on large folders.
                        let matches = matches
                            .into_iter()
                            .filter_map(|item| serde_json::from_value::<SearchMatch>(item).ok())
                            .collect();
                        ui.transfers
                            .duplicate_search_progress
                            .set(Some(DuplicateSearchProgress {
                                run_id,
                                scanned,
                                analyzed,
                                matches,
                                unreadable,
                            }));
                    }
                    Ok(AppEvent::MediaProgress {
                        chat_id,
                        message_id,
                        downloaded_bytes,
                        total_bytes,
                        state,
                    }) => {
                        let id = format!("telegram-{chat_id}-{message_id}");
                        if ui
                            .transfers
                            .downloads()
                            .iter()
                            .any(|item| item.id == id && item.is_active())
                        {
                            match state.as_str() {
                                "ready" => apply_telegram_media_progress(
                                    ui.transfers.downloads,
                                    &id,
                                    total_bytes,
                                    total_bytes,
                                    Some(99.5),
                                ),
                                "downloading" => {
                                    let percent = (total_bytes > 0).then(|| {
                                        (downloaded_bytes as f64 / total_bytes as f64) * 100.0
                                    });
                                    apply_telegram_media_progress(
                                        ui.transfers.downloads,
                                        &id,
                                        downloaded_bytes,
                                        total_bytes,
                                        percent,
                                    );
                                }
                                _ => {}
                            }
                        }
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
                        let label = ui
                            .transfers
                            .downloads()
                            .iter()
                            .find(|item| item.id == id)
                            .map(|item| item.title())
                            .unwrap_or_else(|| id.clone());
                        apply_download_done(ui, &id);
                        persist_downloads(event_services.clone(), ui);
                        if ui.shell.tab() != Tab::Downloads {
                            state::push_toast(
                                ui.shell.toasts,
                                format!("Download concluído: {label}"),
                                ToastKind::Success,
                            );
                        }
                    }
                    Ok(AppEvent::DownloadError { id, error }) => {
                        apply_download_error(ui, &id, error);
                        persist_downloads(event_services.clone(), ui);
                    }
                    Ok(AppEvent::SharedUrl { url }) => {
                        ui.shell.shared_url.set(Some(url));
                        ui.shell.tab.set(Tab::Downloads);
                        // Downloads analysis does not need Telegram auth; open
                        // the shell immediately when a share arrives on login.
                        if !ui.shell.logged_in() {
                            ui.session.preview_mode.set(true);
                            ui.shell.logged_in.set(true);
                        }
                        state::push_toast(
                            ui.shell.toasts,
                            "Link recebido — analisando para download…",
                            ToastKind::Info,
                        );
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
                // A startup burst otherwise stays inside this task and the
                // window stops processing events until the queue drains.
                tokio::task::yield_now().await;
            }
        }
    });

    // Global shortcuts live in the webview, so they work regardless of which
    // element currently has focus.
    let shortcut_services = services.clone();
    use_effect(move || {
        let mut ui = ui;
        let services = shortcut_services.clone();
        spawn(async move {
            let mut eval = document::eval(
                r#"
                document.addEventListener(
                    "contextmenu",
                    (event) => event.preventDefault(),
                    { capture: true },
                );
                document.addEventListener("keydown", (event) => {
                    const key = event.key.toLowerCase();
                    const target = event.target;
                    const typing = target &&
                        (target.tagName === "INPUT" ||
                         target.tagName === "TEXTAREA" ||
                         target.isContentEditable);
                    if ((event.ctrlKey || event.metaKey) && key === "k") {
                        event.preventDefault();
                        dioxus.send("toggle-palette");
                    } else if ((event.ctrlKey || event.metaKey) && key === "b") {
                        event.preventDefault();
                        dioxus.send("toggle-sidebar");
                    } else if ((event.ctrlKey || event.metaKey) && key === "j") {
                        event.preventDefault();
                        dioxus.send("toggle-drawer");
                    } else if (event.key === "Escape") {
                        dioxus.send("close");
                    } else if (
                        !typing &&
                        (event.key === "ArrowLeft" || event.key === "ArrowRight") &&
                        document.querySelector(".lightbox-frame.has-album")
                    ) {
                        event.preventDefault();
                        dioxus.send(event.key === "ArrowLeft" ? "lightbox-prev" : "lightbox-next");
                    }
                });
                "#,
            );

            while let Ok(command) = eval.recv::<String>().await {
                match command.as_str() {
                    "toggle-palette" => {
                        let next = !ui.shell.show_palette();
                        ui.shell.show_palette.set(next);
                    }
                    "toggle-sidebar" => {
                        let next = !ui.shell.sidebar_open();
                        ui.shell.sidebar_open.set(next);
                    }
                    "toggle-drawer" => {
                        let next = !ui.shell.drawer_open();
                        set_drawer_open(&services, ui, next);
                    }
                    "close" => {
                        if ui.shell.show_palette() {
                            ui.shell.show_palette.set(false);
                        } else if ui.panels.context_menu().is_some() {
                            ui.panels.context_menu.set(None);
                        } else if ui.shell.select_portal().is_some() {
                            ui.shell.select_portal.set(None);
                        } else if ui.panels.reaction_picker().is_some() {
                            ui.panels.reaction_picker.set(None);
                        } else if ui.panels.lightbox().is_some() {
                            ui.panels.lightbox.set(None);
                        } else if ui.panels.forward_state().is_some() {
                            ui.panels.forward_state.set(None);
                        } else if ui.transfers.duplicate_search().is_some() {
                            ui.transfers.duplicate_search.set(None);
                        }
                    }
                    "lightbox-prev" => {
                        if let Some(state) = ui.panels.lightbox() {
                            if let Some(previous) = state.previous_album() {
                                ui.panels.lightbox.set(Some(previous));
                            }
                        }
                    }
                    "lightbox-next" => {
                        if let Some(state) = ui.panels.lightbox() {
                            if let Some(next) = state.next_album() {
                                ui.panels.lightbox.set(Some(next));
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
    });

    let style = ui.shell.style();
    let color_mode = ui.shell.color_mode();
    let density = ui.shell.density();
    let open_menu = ui.panels.context_menu();

    rsx! {
        style { dangerous_inner_html: SHELL_CSS }
        div {
            class: "plasma-root",
            "data-style": style.as_str(),
            "data-mode": color_mode.as_str(),
            "data-density": density.as_str(),
            if ui.shell.logged_in() {
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
                            let Some(menu) = ui.panels.context_menu() else {
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
                    on_close: move |_| ui.panels.context_menu.set(None),
                }
            }
            if let Some(portal) = ui.shell.select_portal() {
                SelectMenu {
                    portal,
                    on_close: move |_| ui.shell.select_portal.set(None),
                }
            }
            if let Some(target) = ui.panels.reaction_picker() {
                ReactionPicker {
                    target,
                    on_close: move |_| ui.panels.reaction_picker.set(None),
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
            ui.session.auth_user.set(display);
        }
        Ok(me) => {
            if let Some(error) = me.error {
                ui.session.auth_error.set(Some(error));
            }
        }
        Err(error) => ui.session.auth_error.set(Some(error.to_string())),
    }
}

/// Terminal mass-download progress labels from TDLib and Twitter/X galleries.
fn mass_download_finished(current_file: &str, is_scanning: bool) -> bool {
    if is_scanning {
        return false;
    }
    current_file.starts_with("Concluído")
        || current_file.starts_with("Interrompido")
        || current_file.starts_with("Concluido:")
        || current_file.starts_with("Parado:")
}

/// Unified 3-pane Messenger Workspace shell.
#[component]
fn Shell() -> Element {
    rsx! {
        TelegramView {}
    }
}
