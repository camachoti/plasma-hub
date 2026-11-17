use super::{push_toast, set_if_changed, AppServices, ToastKind, UiState};
use dioxus::prelude::*;
use plasma_telegram::{TdlibChatInfo, TdlibForumTopicInfo};

/// Stable key identifying a forum topic inside a chat.
pub fn topic_visibility_key(topic: &TdlibForumTopicInfo) -> String {
    let kind = if topic.kind.is_empty() {
        "forum"
    } else {
        topic.kind.as_str()
    };
    format!("{kind}:{}", topic.id)
}

/// Stable key for mass-download tracking (`topic_download_state` / new-media hints).
pub fn topic_download_key(topic_id: impl std::fmt::Display, topic_kind: &str) -> String {
    let kind = if topic_kind.is_empty() {
        "forum"
    } else {
        topic_kind
    };
    format!("{kind}:{topic_id}")
}

/// SQLite key holding the hidden topic list of a chat.
pub fn topic_hidden_setting_key(chat_id: i64) -> String {
    format!("plasma_hidden_topics_{chat_id}")
}

/// SQLite key holding the last topic list loaded for a chat.
pub fn topic_list_setting_key(chat_id: i64) -> String {
    format!("plasma_topics_{chat_id}")
}

/// SQLite key remembering whether a chat is a forum (`"1"` or `"0"`).
pub fn forum_flag_setting_key(chat_id: i64) -> String {
    format!("plasma_forum_{chat_id}")
}

/// What the UI can decide about a chat before calling TDLib again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForumDecision {
    /// The topic list is already cached. Show it and refresh in the background.
    ShowCached,
    /// The chat is a forum, but the list is not cached yet. Show the bar now.
    ShowLoading,
    /// Confirmed ordinary chat. Do not ask TDLib.
    Plain,
    /// Group or channel we have never classified. Probe `is_forum` first.
    Probe,
}

/// Picks how to open a chat from the flags already on hand.
///
/// A live `has_topics` or a stored forum flag wins over a missing list, so the
/// topic bar can appear before `getForumTopics` returns.
pub fn forum_decision(
    has_topics: bool,
    is_groupish: bool,
    has_cached_topics: bool,
    stored_flag: Option<bool>,
) -> ForumDecision {
    if has_topics || has_cached_topics || stored_flag == Some(true) {
        if has_cached_topics {
            ForumDecision::ShowCached
        } else {
            ForumDecision::ShowLoading
        }
    } else if !is_groupish || stored_flag == Some(false) {
        ForumDecision::Plain
    } else {
        ForumDecision::Probe
    }
}

/// Returns the topic list cached in memory or SQLite for `chat_id`.
///
/// A hit is copied into the in-memory map so the next click skips SQLite.
pub fn recall_cached_topics(
    services: &AppServices,
    mut ui: UiState,
    chat_id: &str,
) -> Option<Vec<TdlibForumTopicInfo>> {
    if let Some(cached) = ui.conversation.topics_cache.peek().get(chat_id).cloned() {
        return Some(cached).filter(|topics| !topics.is_empty());
    }
    let id = chat_id.parse::<i64>().ok()?;
    let topics = services
        .settings
        .get_raw(&topic_list_setting_key(id))
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<Vec<TdlibForumTopicInfo>>(&raw).ok())
        .filter(|topics: &Vec<_>| !topics.is_empty())?;
    ui.conversation
        .topics_cache
        .write()
        .insert(chat_id.to_string(), topics.clone());
    Some(topics)
}

/// Reads the persisted forum flag. `None` means the chat was never classified.
pub fn recall_forum_flag(services: &AppServices, chat_id: i64) -> Option<bool> {
    match services
        .settings
        .get_raw(&forum_flag_setting_key(chat_id))
        .ok()
        .flatten()
        .as_deref()
    {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    }
}

/// Loads mass-download baselines and marks topics that gained media since then.
pub fn refresh_topics_with_new_media(services: AppServices, mut ui: UiState, chat_id: i64) {
    spawn(async move {
        let Ok(states) = services
            .telegram
            .topic_download_states(chat_id.to_string())
            .await
        else {
            return;
        };
        if states.is_empty() {
            if viewing_numeric_chat(ui, chat_id) {
                ui.conversation
                    .topics_with_new_media
                    .set(std::collections::HashSet::new());
            }
            return;
        }
        let mut next = std::collections::HashSet::new();
        for state in states {
            let Ok(topic_id) = state.topic_id.parse::<i64>() else {
                continue;
            };
            let kind = if state.topic_kind.is_empty() {
                None
            } else {
                Some(state.topic_kind.clone())
            };
            if let Ok(true) = services
                .telegram
                .topic_has_new_media(chat_id, topic_id, kind, state.last_message_id)
                .await
            {
                next.insert(topic_download_key(&state.topic_id, &state.topic_kind));
            }
        }
        if viewing_numeric_chat(ui, chat_id) {
            ui.conversation.topics_with_new_media.set(next);
        }
    });
}

fn viewing_numeric_chat(ui: UiState, chat_id: i64) -> bool {
    ui.conversation
        .selected_chat
        .peek()
        .as_ref()
        .and_then(|chat| chat.id.parse::<i64>().ok())
        == Some(chat_id)
}

/// Applies a cached forum decision to the open chat before the next paint.
///
/// `chat.has_topics` is set when the chat is already known to be a forum, so
/// the topic bar does not wait for TDLib.
pub fn prepare_forum_view(
    services: &AppServices,
    mut ui: UiState,
    chat: &mut TdlibChatInfo,
) -> ForumDecision {
    let cached = recall_cached_topics(services, ui, &chat.id);
    let stored_flag = chat
        .id
        .parse::<i64>()
        .ok()
        .and_then(|chat_id| recall_forum_flag(services, chat_id));
    let decision = forum_decision(
        chat.has_topics,
        chat.is_group || chat.is_channel,
        cached.is_some(),
        stored_flag,
    );
    match decision {
        ForumDecision::ShowCached => {
            set_if_changed(ui.conversation.topics, cached.unwrap_or_default());
            chat.has_topics = true;
        }
        ForumDecision::ShowLoading => {
            ui.conversation.topics.set(Vec::new());
            chat.has_topics = true;
        }
        ForumDecision::Plain | ForumDecision::Probe => {
            ui.conversation.topics.set(Vec::new());
        }
    }
    decision
}

/// Writes the topic list into memory and SQLite, and marks the chat as a forum.
pub fn remember_forum_topics(
    services: &AppServices,
    mut ui: UiState,
    chat_id: &str,
    topics: Vec<TdlibForumTopicInfo>,
) {
    let unchanged = ui
        .conversation
        .topics_cache
        .peek()
        .get(chat_id)
        .is_some_and(|current| current == &topics);
    if !unchanged {
        ui.conversation
            .topics_cache
            .write()
            .insert(chat_id.to_string(), topics.clone());
    }
    let Ok(id) = chat_id.parse::<i64>() else {
        return;
    };
    if unchanged {
        return;
    }
    let Ok(raw) = serde_json::to_string(&topics) else {
        return;
    };
    let services = services.clone();
    spawn(async move {
        let _ = services.settings.set_raw(&topic_list_setting_key(id), &raw);
        let _ = services.settings.set_raw(&forum_flag_setting_key(id), "1");
    });
}

/// Persists whether `chat_id` is a forum so the next open skips the probe.
pub fn remember_forum_flag(services: &AppServices, chat_id: i64, is_forum: bool) {
    let services = services.clone();
    let value = if is_forum { "1" } else { "0" };
    spawn(async move {
        let _ = services
            .settings
            .set_raw(&forum_flag_setting_key(chat_id), value);
        if !is_forum {
            let _ = services
                .settings
                .remove_raw(&topic_list_setting_key(chat_id));
        }
    });
}

/// Updates `has_topics` on the selected chat and on its row in the list.
pub fn set_chat_has_topics(mut ui: UiState, chat_id: &str, has_topics: bool) {
    if ui
        .conversation
        .selected_chat
        .peek()
        .as_ref()
        .is_some_and(|chat| chat.id == chat_id && chat.has_topics != has_topics)
    {
        let mut selected = ui.conversation.selected_chat.peek().clone();
        if let Some(chat) = selected.as_mut() {
            chat.has_topics = has_topics;
        }
        ui.conversation.selected_chat.set(selected);
    }
    if ui
        .conversation
        .chats
        .peek()
        .iter()
        .any(|chat| chat.id == chat_id && chat.has_topics != has_topics)
    {
        let mut chats = ui.conversation.chats.peek().clone();
        if let Some(chat) = chats.iter_mut().find(|chat| chat.id == chat_id) {
            chat.has_topics = has_topics;
        }
        ui.conversation.chats.set(chats);
    }
}

/// Loads the hidden topics persisted for a chat.
pub fn load_hidden_topics(services: &AppServices, ui: UiState, chat_id: i64) {
    let keys = services
        .settings
        .get_raw(&topic_hidden_setting_key(chat_id))
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default();
    set_if_changed(ui.conversation.hidden_topics, keys);
}

/// Toggles a topic between hidden and visible, persisting the chat list.
///
/// Returns `true` when the topic is now hidden.
pub fn toggle_topic_hidden(
    services: &AppServices,
    mut ui: UiState,
    chat_id: i64,
    topic: &TdlibForumTopicInfo,
) -> bool {
    let key = topic_visibility_key(topic);
    let mut keys = ui.conversation.hidden_topics();
    let hidden = if let Some(position) = keys.iter().position(|entry| *entry == key) {
        keys.remove(position);
        false
    } else {
        keys.push(key);
        true
    };
    // Update the UI first and persist off the UI thread: the SQLite write used
    // to keep the chip visible until it finished.
    ui.conversation.hidden_topics.set(keys.clone());
    if let Ok(raw) = serde_json::to_string(&keys) {
        let services = services.clone();
        let setting = topic_hidden_setting_key(chat_id);
        spawn(async move {
            if let Err(error) = services.settings.set_raw(&setting, &raw) {
                push_toast(
                    ui.shell.toasts,
                    format!("Falha ao salvar os tópicos ocultos: {error}"),
                    ToastKind::Error,
                );
            }
        });
    }
    hidden
}
