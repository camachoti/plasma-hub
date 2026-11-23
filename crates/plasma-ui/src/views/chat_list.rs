//! Chat list rows with circular gradient monograms or real TDLib/X avatars.

use crate::context::{ContextMenuState, ContextTarget};
use crate::state::{
    prepare_forum_view, set_chat_has_topics, AppServices, ForumDecision, Tab, UiState,
};
use crate::theme::{avatar_gradient, avatar_initials};
use dioxus::prelude::*;
use plasma_telegram::TdlibChatInfo;
use std::collections::HashSet;

/// Collapses whitespace and truncates a message preview.
pub fn summarize_last_message(text: &str, limit: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= limit {
        return collapsed;
    }
    let mut summary: String = collapsed.chars().take(limit.saturating_sub(1)).collect();
    summary.push('…');
    summary
}

/// A single conversation row in the unified sidebar.
#[component]
pub fn ChatRow(
    chat: TdlibChatInfo,
    active: bool,
    visible_chats: Signal<HashSet<String>>,
    on_open: EventHandler<TdlibChatInfo>,
    on_context: EventHandler<(TdlibChatInfo, f64, f64)>,
) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let is_twitter_fake = crate::twitter_fake::is_twitter_fake_chat_id(&chat.id);
    let gallery_badge = if is_twitter_fake {
        ui.conversation
            .twitter_gallery_badges()
            .get(&chat.id)
            .copied()
            .filter(|count| *count > 0)
    } else {
        None
    };
    let chat_id = if is_twitter_fake {
        None
    } else {
        chat.id.parse::<i64>().ok()
    };
    let title = chat.title.clone();
    let initials = avatar_initials(&title);
    let gradient = avatar_gradient(&chat.id);
    let preview = summarize_last_message(&chat.last_message_text, 90);
    let chat_id_key = chat.id.clone();
    let mut avatar_url = use_signal({
        let services = services.clone();
        let parsed = chat_id;
        let fake_id = chat.id.clone();
        move || {
            if crate::twitter_fake::is_twitter_fake_chat_id(&fake_id) {
                crate::twitter_fake::get_twitter_fake_chat(&services.settings, &fake_id)
                    .and_then(|chat| chat.avatar_url)
            } else {
                parsed.and_then(|chat_id| services.telegram.chat_avatar_url(chat_id))
            }
        }
    });
    let mut avatar_requested = use_hook(|| CopyValue::new(false));
    use_effect(move || {
        let vis = visible_chats.read();
        let wanted = vis.contains(&chat_id_key);
        if avatar_url.peek().is_some() || !wanted || avatar_requested() {
            return;
        }
        if is_twitter_fake {
            avatar_requested.set(true);
            return;
        }
        let Some(chat_id) = chat_id else {
            return;
        };
        avatar_requested.set(true);
        let services = services.clone();
        spawn(async move {
            let _ = services.telegram.ensure_chat_avatar(chat_id).await;
            if let Some(url) = services.telegram.chat_avatar_url(chat_id) {
                avatar_url.set(Some(url));
            }
        });
    });

    let media_prefix = if chat.last_message_is_video {
        "🎬 "
    } else if chat.last_message_is_photo {
        "🖼 "
    } else if chat.last_message_has_media {
        "📎 "
    } else {
        ""
    };

    rsx! {
        button {
            class: if active { "chat-row active" } else { "chat-row" },
            "data-chat-id": "{chat.id}",
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
            div {
                class: "chat-avatar",
                style: "background: {gradient};",
                if let Some(url) = avatar_url() {
                    img { src: "{url}", alt: "" }
                } else {
                    span { "{initials}" }
                }
            }
            div { class: "chat-copy",
                div { class: "chat-name-row",
                    span { class: "chat-name", "{chat.title}" }
                    span { class: "chat-time", "{format_timestamp(chat.last_message_date)}" }
                }
                div { class: "chat-preview-row",
                    span { class: "chat-preview",
                        "{media_prefix}"
                        if preview.is_empty() {
                            "{chat_kind(&chat)}"
                        } else {
                            "{preview}"
                        }
                    }
                    if let Some(count) = gallery_badge {
                        span {
                            class: "unread gallery-new",
                            title: "Novas mídias do X",
                            "{count}"
                        }
                    } else if chat.unread_count > 0 {
                        span { class: "unread", "{chat.unread_count}" }
                    }
                }
            }
        }
    }
}

/// Opens the context menu of a chat row.
pub fn open_chat_menu(mut ui: UiState, chat: TdlibChatInfo, x: f64, y: f64) {
    ui.panels.context_menu.set(Some(ContextMenuState::new(
        x,
        y,
        ContextTarget::Chat { id: chat.id },
    )));
}

fn chat_kind(chat: &TdlibChatInfo) -> &'static str {
    if crate::twitter_fake::is_twitter_fake_chat_id(&chat.id) {
        "Galeria X"
    } else if chat.has_topics {
        "Grupo Fórum"
    } else if chat.is_channel {
        "Canal"
    } else if chat.is_group {
        "Grupo"
    } else {
        "Conversa"
    }
}

fn format_timestamp(timestamp: i32) -> String {
    crate::views::timeline::format_timestamp(timestamp)
}

/// Marks a conversation as read, focuses the Telegram stage and selects it.
pub fn open_chat(services: AppServices, mut ui: UiState, chat: TdlibChatInfo) {
    let mut selected = chat;
    selected.unread_count = 0;
    let chat_id = selected.id.clone();
    let is_twitter_fake = crate::twitter_fake::is_twitter_fake_chat_id(&chat_id);
    if is_twitter_fake {
        ui.conversation
            .twitter_gallery_badges
            .write()
            .remove(&chat_id);
    }
    let decision = if is_twitter_fake {
        ForumDecision::Plain
    } else {
        prepare_forum_view(&services, ui, &mut selected)
    };
    ui.shell.tab.set(Tab::Telegram);
    ui.conversation.selected_chat.set(Some(selected.clone()));
    ui.conversation.mobile_show_list.set(false);
    if matches!(
        decision,
        ForumDecision::ShowCached | ForumDecision::ShowLoading
    ) {
        set_chat_has_topics(ui, &chat_id, true);
    }
    ui.conversation.selected_topic.set(None);
    ui.conversation.hidden_topics.set(Vec::new());
    ui.conversation.topic_browser_open.set(false);
    if is_twitter_fake {
        ui.session.avatar.set(
            crate::twitter_fake::get_twitter_fake_chat(&services.settings, &chat_id)
                .and_then(|chat| chat.avatar_url),
        );
    } else {
        ui.session.avatar.set(None);
    }
    ui.conversation.messages.set(Vec::new());
    ui.conversation.messages_has_more.set(false);
    ui.conversation.messages_key.set(None);
    ui.conversation.search_results.set(Vec::new());
    ui.conversation.search_error.set(None);
    ui.conversation.search_loading.set(false);
    ui.panels.reply_to.set(None);
    ui.panels.user_media.set(None);
}

#[cfg(test)]
mod tests {
    use super::summarize_last_message;

    #[test]
    fn summarize_collapses_whitespace_and_truncates() {
        assert_eq!(
            summarize_last_message("  oi\n  tudo bem  ", 40),
            "oi tudo bem"
        );
        let long = "a".repeat(120);
        let summary = summarize_last_message(&long, 80);
        assert_eq!(summary.chars().count(), 80);
        assert!(summary.ends_with('…'));
    }
}
