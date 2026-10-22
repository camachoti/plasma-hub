//! Chat list rows with the real avatar of each conversation.

use crate::context::{ContextMenuState, ContextTarget};
use crate::state::{AppServices, UiState};
use dioxus::prelude::*;
use plasma_telegram::TdlibChatInfo;

/// Avatar of a chat.
///
/// The photo is downloaded on demand and served by the local media server, so
/// the list does not carry base64 payloads in the DOM. Until the file is
/// ready the row keeps the coloured letter placeholder.
#[component]
pub fn ChatAvatar(chat_id: i64, title: String) -> Element {
    let services = use_context::<AppServices>();
    let resource = use_resource(move || {
        let services = services.clone();
        async move {
            let _ = services.telegram.ensure_chat_avatar(chat_id).await;
            services.telegram.chat_avatar_url(chat_id)
        }
    });

    rsx! {
        div { class: "chat-avatar",
            match &*resource.read_unchecked() {
                Some(Some(url)) => rsx! { img { src: "{url}", alt: "" } },
                _ => rsx! { span { "{title.chars().next().unwrap_or('?')}" } },
            }
        }
    }
}

/// A single conversation in the chat list.
#[component]
pub fn ChatRow(
    chat: TdlibChatInfo,
    active: bool,
    on_open: EventHandler<TdlibChatInfo>,
    on_context: EventHandler<(TdlibChatInfo, f64, f64)>,
) -> Element {
    let chat_id = chat.id.parse::<i64>().ok();
    let title = chat.title.clone();

    rsx! {
        button {
            class: if active { "chat-row active" } else { "chat-row" },
            oncontextmenu: {
                let chat = chat.clone();
                move |event: Event<MouseData>| {
                    event.prevent_default();
                    let point = event.data().client_coordinates();
                    on_context.call((chat.clone(), point.x, point.y));
                }
            },
            onclick: {
                let chat = chat.clone();
                move |_| on_open.call(chat.clone())
            },
            if let Some(chat_id) = chat_id {
                ChatAvatar { chat_id, title }
            } else {
                div { class: "chat-avatar",
                    span { "{title.chars().next().unwrap_or('?')}" }
                }
            }
            div { class: "chat-copy",
                span { class: "chat-name", "{chat.title}" }
                span { class: "chat-preview",
                    if chat.last_message_has_media { "▣ " }
                    if chat.last_message_text.trim().is_empty() {
                        "{chat_kind(&chat)}"
                    } else {
                        "{chat.last_message_text}"
                    }
                }
            }
            div { class: "chat-meta",
                span { class: "chat-time", "{format_timestamp(chat.last_message_date)}" }
                if chat.unread_count > 0 {
                    span { class: "unread", "{chat.unread_count}" }
                }
            }
        }
    }
}

/// Opens the context menu of a chat row.
pub fn open_chat_menu(mut ui: UiState, chat: TdlibChatInfo, x: f64, y: f64) {
    ui.context_menu.set(Some(ContextMenuState::new(
        x,
        y,
        ContextTarget::Chat { id: chat.id },
    )));
}

fn chat_kind(chat: &TdlibChatInfo) -> &'static str {
    if chat.is_channel {
        "Canal"
    } else if chat.is_group {
        "Grupo"
    } else {
        "Contato"
    }
}

fn format_timestamp(timestamp: i32) -> String {
    crate::views::timeline::format_timestamp(timestamp)
}

/// Marks a conversation as read and selects it.
pub fn open_chat(services: AppServices, mut ui: UiState, chat: TdlibChatInfo) {
    let mut selected = chat;
    selected.unread_count = 0;
    ui.selected_chat.set(Some(selected.clone()));
    ui.selected_topic.set(None);
    ui.avatar.set(None);
    ui.messages.set(Vec::new());
    ui.messages_has_more.set(false);
    ui.messages_key.set(None);
    ui.search_results.set(Vec::new());
    ui.search_error.set(None);
    ui.search_loading.set(false);
    ui.reply_to.set(None);
    if let Ok(chat_id) = selected.id.parse::<i64>() {
        spawn(async move {
            let _ = services.telegram.read_chat(chat_id).await;
        });
    }
}
