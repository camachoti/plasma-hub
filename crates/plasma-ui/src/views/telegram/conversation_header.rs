//! Open conversation header, inline/global search capsule, drawer toggles and forum topic chips.

use super::conversation::{
    bump_epoch, chat_summary, run_global_search, run_message_search, SearchWatch,
};
use crate::context::{ContextMenuState, ContextTarget};
use crate::icons::{Icon, IconName};
use crate::state::{
    open_drawer_tab, push_toast, set_drawer_open, topic_download_key, topic_visibility_key,
    AppServices, DrawerTab, ToastKind, UiState,
};
use crate::theme::{avatar_gradient, avatar_initials};
use dioxus::prelude::*;
use plasma_telegram::{TdlibChatInfo, TdlibForumTopicInfo, TdlibGlobalSearchMessage};

async fn refresh_twitter_fake_chat(services: AppServices, mut ui: UiState, chat_id: String) {
    use crate::state::{now_millis, TwitterGallerySyncStatus};

    let Some(existing) = crate::twitter_fake::get_twitter_fake_chat(&services.settings, &chat_id)
    else {
        push_toast(
            ui.shell.toasts,
            "Galeria Twitter/X não encontrada.",
            ToastKind::Error,
        );
        return;
    };
    let previous_at = ui
        .conversation
        .twitter_sync_status
        .peek()
        .as_ref()
        .and_then(|status| status.last_at_ms);
    ui.conversation
        .twitter_sync_status
        .set(Some(TwitterGallerySyncStatus {
            chat_id: chat_id.clone(),
            last_at_ms: previous_at,
            last_error: None,
            checking: true,
        }));
    let cookies = services.twitter.load_cookies().await.unwrap_or_default();
    let cookie_value = {
        let trimmed = cookies.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_string())
    };
    let url = if existing.original_url.trim().is_empty() {
        format!("https://x.com/{}", existing.username)
    } else {
        existing.original_url.clone()
    };
    match services.twitter.analyze_profile(url, cookie_value).await {
        Ok(profile) => match crate::twitter_fake::update_from_profile(&services.settings, &profile)
        {
            Ok(result) => {
                let mut chats = ui.conversation.chats();
                crate::twitter_fake::merge_fake_dialogs_into(&mut chats, &services.settings);
                ui.conversation.chats.set(chats);
                if let Some(fake) =
                    crate::twitter_fake::get_twitter_fake_chat(&services.settings, &chat_id)
                {
                    let thumbs = fake.thumbnail_seed();
                    if !thumbs.is_empty() {
                        let mut cache = ui.conversation.thumbnails.write();
                        for (key, url) in thumbs {
                            cache.insert(key, url);
                        }
                    }
                    ui.conversation.messages.set(fake.messages_as_tdlib());
                    if let Some(avatar) = fake.avatar_url {
                        ui.session.avatar.set(Some(avatar));
                    }
                }
                ui.conversation
                    .twitter_gallery_badges
                    .write()
                    .remove(&chat_id);
                ui.conversation
                    .twitter_sync_status
                    .set(Some(TwitterGallerySyncStatus {
                        chat_id: chat_id.clone(),
                        last_at_ms: Some(now_millis()),
                        last_error: None,
                        checking: false,
                    }));
                let message = if result.added_count == 0 {
                    "Nenhuma mídia nova neste perfil.".to_string()
                } else {
                    format!("{} nova(s) mídia(s) adicionada(s).", result.added_count)
                };
                push_toast(ui.shell.toasts, message, ToastKind::Success);
            }
            Err(error) => {
                ui.conversation
                    .twitter_sync_status
                    .set(Some(TwitterGallerySyncStatus {
                        chat_id,
                        last_at_ms: None,
                        last_error: Some(error.message.clone()),
                        checking: false,
                    }));
                push_toast(ui.shell.toasts, error.message, ToastKind::Error);
            }
        },
        Err(error) => {
            let message = error.to_string();
            ui.conversation
                .twitter_sync_status
                .set(Some(TwitterGallerySyncStatus {
                    chat_id,
                    last_at_ms: None,
                    last_error: Some(message.clone()),
                    checking: false,
                }));
            push_toast(ui.shell.toasts, message, ToastKind::Error);
        }
    }
}

#[component]
pub(super) fn ConversationHeader(
    chat: TdlibChatInfo,
    topic: Option<TdlibForumTopicInfo>,
    timeline_chat_id: i64,
    in_search: bool,
    mut global_search: Signal<bool>,
    mut search_active: Signal<bool>,
    mut message_query: Signal<String>,
    mut global_results: Signal<Vec<TdlibGlobalSearchMessage>>,
    mut global_offset: Signal<String>,
    mut global_loading_more: Signal<bool>,
    mut search_epoch: Signal<u64>,
) -> Element {
    let mut ui = use_context::<UiState>();
    let services = use_context::<AppServices>();
    let search_placeholder = if global_search() {
        "Buscar em todos os chats…"
    } else {
        "Buscar na conversa…"
    };
    let topics = ui.conversation.topics();
    let hidden_keys = ui.conversation.hidden_topics();
    let topics_with_new_media = ui.conversation.topics_with_new_media();
    let twitter_sync = ui.conversation.twitter_sync_status();
    let listed_topics: Vec<TdlibForumTopicInfo> = topics
        .iter()
        .filter(|item| {
            let hidden = hidden_keys
                .iter()
                .any(|key| *key == topic_visibility_key(item));
            !hidden || topic.as_ref().map(|current| current.id) == Some(item.id)
        })
        .cloned()
        .collect();
    let is_twitter_fake = crate::twitter_fake::is_twitter_fake_chat_id(&chat.id);
    let subtitle = if let Some(topic) = topic.as_ref() {
        format!("Tópico: {}", topic.title)
    } else if is_twitter_fake {
        let sync = twitter_sync
            .as_ref()
            .filter(|status| status.chat_id == chat.id);
        if sync.is_some_and(|status| status.checking) {
            "Atualizando do X…".to_string()
        } else if let Some(error) = sync.and_then(|status| status.last_error.as_ref()) {
            format!("Sync: {error}")
        } else if let Some(at) = sync.and_then(|status| status.last_at_ms) {
            format!("Última sync: {}", format_sync_time(at))
        } else {
            "Galeria X · Sincronização ativa".to_string()
        }
    } else {
        chat_summary(&chat)
    };

    let trigger_search = {
        let services = services.clone();
        move || {
            let query = message_query().trim().to_string();
            if query.is_empty() {
                bump_epoch(search_epoch);
                search_active.set(false);
                global_results.set(Vec::new());
                global_offset.set(String::new());
                global_loading_more.set(false);
                ui.conversation.search_results.set(Vec::new());
                ui.conversation.search_error.set(None);
                ui.conversation.search_loading.set(false);
                return;
            }
            search_active.set(true);
            if global_search() {
                ui.conversation.search_results.set(Vec::new());
                global_loading_more.set(false);
                run_global_search(
                    services.clone(),
                    ui,
                    global_results,
                    global_offset,
                    query,
                    SearchWatch {
                        query: message_query,
                        epoch: search_epoch,
                        global: global_search,
                    },
                );
            } else {
                let Some(chat) = ui.conversation.selected_chat() else {
                    return;
                };
                global_results.set(Vec::new());
                global_offset.set(String::new());
                global_loading_more.set(false);
                run_message_search(
                    services.clone(),
                    ui,
                    chat,
                    ui.conversation.selected_topic(),
                    query,
                    SearchWatch {
                        query: message_query,
                        epoch: search_epoch,
                        global: global_search,
                    },
                );
            }
        }
    };

    let mut trigger_search_enter = trigger_search.clone();
    let mut trigger_search_click = trigger_search;
    let gradient = avatar_gradient(&chat.id);
    let initials = avatar_initials(&chat.title);
    let drawer_open = ui.shell.drawer_open();
    let drawer_tab = ui.shell.drawer_tab();

    rsx! {
        div { class: "chat-header-wrap",
            header { class: "chat-header",
                div { class: "chat-header-title",
                    button {
                        class: "icon-button mobile-nav-back",
                        title: "Voltar para a lista de chats",
                        aria_label: "Voltar para a lista de chats",
                        onclick: move |_| ui.conversation.mobile_show_list.set(true),
                        Icon { name: IconName::ChevronLeft, size: 18, class: "icon" }
                    }
                    if !ui.shell.sidebar_open() {
                        button {
                            class: "icon-button",
                            title: "Mostrar lista de conversas (Ctrl+B)",
                            aria_label: "Mostrar lista de conversas",
                            onclick: move |_| ui.shell.sidebar_open.set(true),
                            Icon { name: IconName::LayoutSidebar, size: 18, class: "icon" }
                        }
                    }
                    div {
                        class: "chat-header-avatar",
                        style: "background: {gradient};",
                        if let Some(avatar) = ui.session.avatar() {
                            img { src: "{avatar}", alt: "" }
                        } else {
                            span { "{initials}" }
                        }
                    }
                    div { class: "chat-header-copy",
                        strong { "{chat.title}" }
                        span { "{subtitle}" }
                    }
                }

                div { class: "chat-header-actions",
                    div { class: "header-search-pill",
                        button {
                            class: if global_search() { "icon-button small active" } else { "icon-button small" },
                            title: if global_search() {
                                "Busca global ativa (clique para buscar só nesta conversa)"
                            } else {
                                "Buscar em todas as conversas"
                            },
                            aria_label: "Alternar busca global",
                            onclick: move |_| {
                                let next = !global_search();
                                global_search.set(next);
                                bump_epoch(search_epoch);
                                search_active.set(false);
                                global_results.set(Vec::new());
                                global_offset.set(String::new());
                                global_loading_more.set(false);
                                ui.conversation.search_results.set(Vec::new());
                                ui.conversation.search_error.set(None);
                                ui.conversation.search_loading.set(false);
                            },
                            Icon {
                                name: if global_search() { IconName::WorldCheck } else { IconName::World },
                                size: 15,
                                class: "icon",
                            }
                        }
                        input {
                            r#type: "search",
                            placeholder: "{search_placeholder}",
                            aria_label: "{search_placeholder}",
                            value: "{message_query}",
                            oninput: move |event| message_query.set(event.value()),
                            onkeydown: move |event: KeyboardEvent| {
                                if event.key() == Key::Enter {
                                    trigger_search_enter();
                                }
                            },
                        }
                        if in_search {
                            button {
                                class: "icon-button small",
                                title: "Limpar busca",
                                aria_label: "Limpar busca",
                                onclick: move |_| {
                                    bump_epoch(search_epoch);
                                    search_active.set(false);
                                    message_query.set(String::new());
                                    global_results.set(Vec::new());
                                    global_offset.set(String::new());
                                    global_loading_more.set(false);
                                    ui.conversation.search_results.set(Vec::new());
                                    ui.conversation.search_error.set(None);
                                    ui.conversation.search_loading.set(false);
                                },
                                Icon { name: IconName::Close, size: 14, class: "icon" }
                            }
                        } else {
                            button {
                                class: "icon-button small",
                                disabled: ui.conversation.search_loading(),
                                title: "Buscar mensagens",
                                aria_label: "Buscar mensagens",
                                onclick: move |_| trigger_search_click(),
                                Icon { name: IconName::Search, size: 15, class: "icon" }
                            }
                        }
                    }

                    if is_twitter_fake {
                        button {
                            class: "icon-button",
                            title: "Sincronizar novas mídias do X",
                            aria_label: "Sincronizar novas mídias do X",
                            onclick: {
                                let services = services.clone();
                                let chat_id = chat.id.clone();
                                move |_| {
                                    let services = services.clone();
                                    let chat_id = chat_id.clone();
                                    spawn(async move {
                                        refresh_twitter_fake_chat(services, ui, chat_id).await;
                                    });
                                }
                            },
                            Icon { name: IconName::Refresh, size: 17, class: "icon" }
                        }
                    }

                    button {
                        class: if drawer_open && drawer_tab == DrawerTab::MassDownload {
                            "icon-button active"
                        } else {
                            "icon-button"
                        },
                        title: "Download em massa desta conversa",
                        aria_label: "Download em massa desta conversa",
                        onclick: {
                            let services = services.clone();
                            move |_| {
                                ui.transfers.mass_download_topic.set(None);
                                open_drawer_tab(&services, ui, DrawerTab::MassDownload);
                            }
                        },
                        Icon { name: IconName::CloudDownload, size: 18, class: "icon" }
                    }

                    if !is_twitter_fake {
                        button {
                            class: if drawer_open && drawer_tab == DrawerTab::Media {
                                "icon-button active"
                            } else {
                                "icon-button"
                            },
                            title: "Mídias compartilhadas",
                            aria_label: "Mídias compartilhadas",
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    ui.panels.user_media.set(None);
                                    open_drawer_tab(&services, ui, DrawerTab::Media);
                                }
                            },
                            Icon { name: IconName::Media, size: 18, class: "icon" }
                        }
                    }

                    button {
                        class: if drawer_open { "icon-button active" } else { "icon-button" },
                        title: "Alternar gaveta lateral (Ctrl+J)",
                        aria_label: "Alternar gaveta lateral",
                        onclick: {
                            let services = services.clone();
                            move |_| set_drawer_open(&services, ui, !drawer_open)
                        },
                        Icon { name: IconName::LayoutSidebarRight, size: 18, class: "icon" }
                    }
                }
            }

            if chat.has_topics {
                div { class: "topic-strip",
                    if topics.is_empty() {
                        span { class: "topic-chip", "Carregando tópicos…" }
                    } else {
                        button {
                            class: if topic.is_none() { "topic-chip active" } else { "topic-chip" },
                            onclick: move |_| {
                                ui.conversation.selected_topic.set(None);
                                ui.panels.reply_to.set(None);
                            },
                            oncontextmenu: move |event: Event<MouseData>| {
                                event.prevent_default();
                                event.stop_propagation();
                                let point = event.data().client_coordinates();
                                ui.panels.context_menu.set(Some(ContextMenuState::new(
                                    point.x,
                                    point.y,
                                    ContextTarget::Topics {
                                        browser_open: drawer_open && drawer_tab == DrawerTab::Topics,
                                    },
                                )));
                            },
                            "Todos"
                        }
                        for item in listed_topics {
                            {
                                let key = topic_download_key(item.id, &item.kind);
                                let has_new = topics_with_new_media.contains(&key);
                                let active = topic.as_ref().map(|current| current.id) == Some(item.id);
                                let hidden = hidden_keys
                                    .iter()
                                    .any(|vis| *vis == topic_visibility_key(&item));
                                let mut class = if active {
                                    "topic-chip active".to_string()
                                } else {
                                    "topic-chip".to_string()
                                };
                                if hidden {
                                    class.push_str(" hidden-topic");
                                }
                                rsx! {
                                    button {
                                        key: "{item.kind}:{item.id}",
                                        class: "{class}",
                                        title: if has_new {
                                            "Há mídia nova desde o último download em massa"
                                        } else {
                                            ""
                                        },
                                        onclick: {
                                            let item = item.clone();
                                            move |_| {
                                                ui.conversation.selected_topic.set(Some(item.clone()));
                                                ui.panels.reply_to.set(None);
                                            }
                                        },
                                        oncontextmenu: {
                                            let item = item.clone();
                                            move |event: Event<MouseData>| {
                                                event.prevent_default();
                                                event.stop_propagation();
                                                let point = event.data().client_coordinates();
                                                ui.panels.context_menu.set(Some(ContextMenuState::new(
                                                    point.x,
                                                    point.y,
                                                    ContextTarget::Topic {
                                                        chat_id: timeline_chat_id,
                                                        topic: Box::new(item.clone()),
                                                        hidden,
                                                        browser_open: drawer_open
                                                            && drawer_tab == DrawerTab::Topics,
                                                    },
                                                )));
                                            }
                                        },
                                        if item.is_closed { "🔒 " }
                                        "{item.title}"
                                        if has_new {
                                            span { class: "topic-new-dot" }
                                        }
                                        if item.unread_count > 0 {
                                            span { class: "topic-chip-badge", "{item.unread_count}" }
                                        }
                                    }
                                }
                            }
                        }
                        button {
                            class: "topic-chip",
                            title: "Ver todos os tópicos na gaveta lateral",
                            onclick: {
                                let services = services.clone();
                                move |_| open_drawer_tab(&services, ui, DrawerTab::Topics)
                            },
                            Icon { name: IconName::List, size: 14, class: "icon" }
                            "Lista"
                        }
                    }
                }
            }
        }
    }
}

fn format_sync_time(at_ms: i64) -> String {
    let now = crate::state::now_millis();
    let elapsed_secs = ((now - at_ms).max(0)) / 1000;
    let minutes = elapsed_secs / 60;
    if minutes < 1 {
        "agora".into()
    } else if minutes < 60 {
        format!("há {minutes} min")
    } else if minutes < 24 * 60 {
        format!("há {} h", minutes / 60)
    } else {
        format!("há {} d", minutes / (24 * 60))
    }
}
