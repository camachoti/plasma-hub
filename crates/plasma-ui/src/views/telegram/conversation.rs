//! Load, search and forum topics for the open chat.

use crate::state::{
    merge_messages, message_cache_key, push_toast, remember_forum_flag, remember_forum_topics,
    set_chat_has_topics, set_if_changed, AppServices, ToastKind, UiState,
};
use crate::views::chat_list;
use dioxus::prelude::*;
use plasma_telegram::{
    TdlibChatInfo, TdlibForumTopicInfo, TdlibGlobalSearchMessage, TdlibMessageInfo,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy)]
pub(super) struct SearchWatch {
    pub(super) query: Signal<String>,
    pub(super) epoch: Signal<u64>,
    pub(super) global: Signal<bool>,
}

pub(super) fn bump_epoch(mut epoch: Signal<u64>) -> u64 {
    let next = epoch().wrapping_add(1);
    epoch.set(next);
    next
}

pub(super) fn run_message_search(
    services: AppServices,
    mut ui: UiState,
    chat: TdlibChatInfo,
    topic: Option<TdlibForumTopicInfo>,
    query: String,
    watch: SearchWatch,
) {
    let SearchWatch {
        query: message_query,
        epoch: search_epoch,
        global: global_search,
    } = watch;
    let Some(chat_id) = parse_chat_id(&chat) else {
        ui.conversation
            .search_error
            .set(Some("Chat inválido.".to_string()));
        return;
    };
    let generation = bump_epoch(search_epoch);
    ui.conversation.search_loading.set(true);
    ui.conversation.search_error.set(None);
    ui.conversation.search_results.set(Vec::new());
    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());
    spawn(async move {
        let outcome = services
            .telegram
            .search_chat_messages(chat_id, query.clone(), 100, None, topic_id, topic_kind)
            .await;
        if search_epoch() != generation {
            return;
        }
        if global_search()
            || message_query().trim() != query
            || !conversation_matches(ui, &chat, topic.as_ref())
        {
            ui.conversation.search_loading.set(false);
            return;
        }
        match outcome {
            Ok(result) if result.success => ui.conversation.search_results.set(result.messages),
            Ok(result) => ui.conversation.search_error.set(
                result
                    .error
                    .or_else(|| Some("Busca não retornou resultados.".into())),
            ),
            Err(error) => ui.conversation.search_error.set(Some(error.to_string())),
        }
        ui.conversation.search_loading.set(false);
    });
}

pub(super) fn fill_missing_chat_titles(
    ui: UiState,
    messages: Vec<TdlibGlobalSearchMessage>,
) -> Vec<TdlibGlobalSearchMessage> {
    let titles: std::collections::HashMap<String, String> = ui
        .conversation
        .chats()
        .into_iter()
        .map(|chat| (chat.id, chat.title))
        .collect();
    messages
        .into_iter()
        .map(|mut item| {
            if item.chat_title.trim().is_empty() {
                item.chat_title = titles
                    .get(&item.chat_id)
                    .cloned()
                    .unwrap_or_else(|| "Conversa".to_string());
            }
            item
        })
        .collect()
}

pub(super) fn sort_global_results(results: &mut [TdlibGlobalSearchMessage]) {
    results.sort_by(|left, right| {
        right
            .message
            .date
            .cmp(&left.message.date)
            .then_with(|| right.message.id.cmp(&left.message.id))
    });
}

pub(super) fn run_global_search(
    services: AppServices,
    mut ui: UiState,
    mut results: Signal<Vec<TdlibGlobalSearchMessage>>,
    mut offset: Signal<String>,
    query: String,
    watch: SearchWatch,
) {
    let SearchWatch {
        query: message_query,
        epoch: search_epoch,
        global: global_search,
    } = watch;
    if !global_search() {
        return;
    }
    let generation = bump_epoch(search_epoch);
    ui.conversation.search_loading.set(true);
    ui.conversation.search_error.set(None);
    results.set(Vec::new());
    offset.set(String::new());
    let query = query.clone();
    spawn(async move {
        let outcome = services
            .telegram
            .search_messages(query.clone(), 100, None)
            .await;
        if search_epoch() != generation || !global_search() || message_query().trim() != query {
            ui.conversation.search_loading.set(false);
            return;
        }
        match outcome {
            Ok(result) if result.success => {
                results.set(fill_missing_chat_titles(ui, result.messages));
                offset.set(result.next_offset);
            }
            Ok(result) => ui.conversation.search_error.set(
                result
                    .error
                    .or_else(|| Some("Busca não retornou resultados.".into())),
            ),
            Err(error) => ui.conversation.search_error.set(Some(error.to_string())),
        }
        ui.conversation.search_loading.set(false);
    });
}

pub(super) fn run_global_search_more(
    services: AppServices,
    mut ui: UiState,
    mut results: Signal<Vec<TdlibGlobalSearchMessage>>,
    mut offset: Signal<String>,
    mut loading_more: Signal<bool>,
    query: String,
    watch: SearchWatch,
) {
    let SearchWatch {
        query: message_query,
        epoch: search_epoch,
        global: global_search,
    } = watch;
    let next_offset = offset();
    if !global_search()
        || query.trim() != message_query().trim()
        || next_offset.is_empty()
        || loading_more()
    {
        return;
    }
    let generation = search_epoch();
    loading_more.set(true);
    let query = query.trim().to_string();
    spawn(async move {
        let outcome = services
            .telegram
            .search_messages(query.clone(), 100, Some(next_offset))
            .await;
        if search_epoch() != generation || !global_search() || message_query().trim() != query {
            loading_more.set(false);
            return;
        }
        match outcome {
            Ok(result) if result.success => {
                let mut merged = results();
                for item in fill_missing_chat_titles(ui, result.messages) {
                    let duplicate = merged.iter().any(|existing| {
                        existing.chat_id == item.chat_id && existing.message.id == item.message.id
                    });
                    if !duplicate {
                        merged.push(item);
                    }
                }
                sort_global_results(&mut merged);
                results.set(merged);
                offset.set(result.next_offset);
            }
            Ok(result) => ui.conversation.search_error.set(
                result
                    .error
                    .or_else(|| Some("Busca não retornou resultados.".into())),
            ),
            Err(error) => ui.conversation.search_error.set(Some(error.to_string())),
        }
        loading_more.set(false);
    });
}

fn load_twitter_fake_conversation(
    services: AppServices,
    mut ui: UiState,
    chat: TdlibChatInfo,
    epoch: Arc<AtomicU64>,
    generation: u64,
) {
    if epoch.load(Ordering::Acquire) != generation {
        return;
    }
    let cache_key = message_cache_key(&chat.id, None);
    set_if_changed(ui.conversation.messages_key, Some(cache_key));
    set_if_changed(ui.conversation.messages_error, None::<String>);
    set_if_changed(ui.conversation.messages_has_more, false);
    set_if_changed(ui.conversation.messages_loading, true);

    let Some(fake) = crate::twitter_fake::get_twitter_fake_chat(&services.settings, &chat.id)
    else {
        set_if_changed(
            ui.conversation.messages_error,
            Some("Galeria Twitter/X não encontrada.".to_string()),
        );
        set_if_changed(ui.conversation.messages_loading, false);
        return;
    };
    if epoch.load(Ordering::Acquire) != generation || !conversation_matches(ui, &chat, None) {
        return;
    }
    let messages = fake.messages_as_tdlib();
    let thumbs = fake.thumbnail_seed();
    if !thumbs.is_empty() {
        let mut cache = ui.conversation.thumbnails.write();
        for (key, url) in thumbs {
            cache.insert(key, url);
        }
    }
    if let Some(avatar) = fake.avatar_url.clone() {
        ui.session.avatar.set(Some(avatar));
    }
    set_if_changed(ui.conversation.messages, messages);
    set_if_changed(ui.conversation.messages_loading, false);
}

pub(super) async fn load_conversation(
    services: AppServices,
    mut ui: UiState,
    chat: TdlibChatInfo,
    topic: Option<TdlibForumTopicInfo>,
    epoch: Arc<AtomicU64>,
    generation: u64,
) {
    if crate::twitter_fake::is_twitter_fake_chat_id(&chat.id) {
        load_twitter_fake_conversation(services, ui, chat, epoch, generation);
        return;
    }
    let Some(chat_id) = parse_chat_id(&chat) else {
        ui.conversation
            .messages_error
            .set(Some("Chat inválido.".to_string()));
        return;
    };
    if epoch.load(Ordering::Acquire) != generation {
        return;
    }
    let cache_key = message_cache_key(&chat.id, topic.as_ref());
    // Background refreshes (new messages, edits, deletions) must not tear down
    // the timeline: that used to blink the loading state and jump to the
    // newest message. Only a real conversation change clears the page.
    let keep_visible = ui.conversation.messages_key().as_deref() == Some(cache_key.as_str())
        && !ui.conversation.messages().is_empty();
    if !keep_visible {
        ui.conversation.messages_loading.set(true);
        ui.conversation.messages.set(Vec::new());
        set_if_changed(ui.conversation.messages_has_more, false);
    }
    set_if_changed(ui.conversation.messages_key, Some(cache_key.clone()));
    set_if_changed(ui.conversation.messages_error, None::<String>);

    let mut cached = Vec::new();
    if !keep_visible {
        if let Ok(cache) = services.telegram.cached_messages(cache_key.clone()).await {
            cached = cache
                .messages
                .iter()
                .filter_map(|value| serde_json::from_value::<TdlibMessageInfo>(value.clone()).ok())
                .collect();
            if !cached.is_empty()
                && epoch.load(Ordering::Acquire) == generation
                && conversation_matches(ui, &chat, topic.as_ref())
            {
                set_if_changed(ui.conversation.messages, cached.clone());
                set_if_changed(ui.conversation.messages_loading, false);
            }
        }
    }

    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());

    // Prefer tip/unread from the list row (selected copy may already be zeroed).
    let list_row = ui
        .conversation
        .chats
        .peek()
        .iter()
        .find(|item| item.id == chat.id)
        .cloned();
    let unread_hint = list_row
        .as_ref()
        .map(|item| item.unread_count)
        .unwrap_or(chat.unread_count);
    let last_message_id = list_row
        .and_then(|item| item.last_message_id)
        .or(chat.last_message_id);

    // Hold Conversation priority through the network follow-up so soft list
    // scans, avatars and reply lookups cannot stampede the single TDLib client
    // right after a thin local-cache paint (often count=1).
    let owns_loading = !keep_visible;
    let mut painted_quick = false;
    let mut need_follow_up = false;
    let mut solid_page = false;
    if epoch.load(Ordering::Acquire) != generation {
        return;
    }
    // Let a reactive spawn storm settle: only the latest generation takes a
    // Conversation lease. Taking it before the first await pinned tens of
    // thousands of obsolete loads on the TDLib client.
    tokio::task::yield_now().await;
    if epoch.load(Ordering::Acquire) != generation {
        return;
    }
    let _priority = services.telegram.begin_conversation_priority();
    let quick = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        services
            .telegram
            .get_messages(chat_id, 80, 0, 0, topic_id, topic_kind.clone(), Some(2)),
    )
    .await;
    match quick {
        Ok(Ok(result)) if result.success => {
            if epoch.load(Ordering::Acquire) != generation
                || !conversation_matches(ui, &chat, topic.as_ref())
            {
                if owns_loading && epoch.load(Ordering::Acquire) == generation {
                    set_if_changed(ui.conversation.messages_loading, false);
                }
                return;
            }
            let mut merged = if keep_visible {
                ui.conversation.messages()
            } else {
                cached.clone()
            };
            merge_messages(&mut merged, result.messages);
            set_if_changed(ui.conversation.messages, merged.clone());
            painted_quick = !merged.is_empty();
            // Local-first quick pages always set has_more so we refresh from
            // the network after the spinner is gone.
            need_follow_up = result.has_more || merged.len() < 40;
            solid_page = merged.len() >= 40 && !need_follow_up;
            if !keep_visible {
                set_if_changed(ui.conversation.messages_has_more, result.has_more);
            }
            if painted_quick {
                set_if_changed(ui.conversation.messages_loading, false);
            }
        }
        Ok(Ok(result)) => {
            if epoch.load(Ordering::Acquire) == generation
                && conversation_matches(ui, &chat, topic.as_ref())
            {
                set_if_changed(
                    ui.conversation.messages_error,
                    result
                        .error
                        .or_else(|| Some("Mensagens indisponíveis.".into())),
                );
            }
        }
        Ok(Err(error)) => {
            if epoch.load(Ordering::Acquire) == generation
                && conversation_matches(ui, &chat, topic.as_ref())
            {
                set_if_changed(ui.conversation.messages_error, Some(error.to_string()));
            }
        }
        Err(_) => {
            if epoch.load(Ordering::Acquire) == generation
                && conversation_matches(ui, &chat, topic.as_ref())
            {
                set_if_changed(
                    ui.conversation.messages_error,
                    Some("Tempo esgotado ao carregar mensagens.".into()),
                );
            }
            need_follow_up = true;
        }
    }

    if need_follow_up {
        match tokio::time::timeout(
            std::time::Duration::from_secs(12),
            services
                .telegram
                .get_messages(chat_id, 80, 0, 0, topic_id, topic_kind, Some(4)),
        )
        .await
        {
            Ok(Ok(result)) if result.success => {
                if epoch.load(Ordering::Acquire) != generation
                    || !conversation_matches(ui, &chat, topic.as_ref())
                {
                    if owns_loading && epoch.load(Ordering::Acquire) == generation {
                        set_if_changed(ui.conversation.messages_loading, false);
                    }
                    return;
                }
                let mut merged = if keep_visible || painted_quick {
                    ui.conversation.messages()
                } else {
                    cached
                };
                merge_messages(&mut merged, result.messages);
                set_if_changed(ui.conversation.messages, merged.clone());
                solid_page = !merged.is_empty();
                if !keep_visible {
                    set_if_changed(ui.conversation.messages_has_more, result.has_more);
                    let _ = services.telegram.cache_messages(cache_key, merged).await;
                }
            }
            Ok(Ok(result)) => {
                if epoch.load(Ordering::Acquire) == generation
                    && conversation_matches(ui, &chat, topic.as_ref())
                {
                    set_if_changed(
                        ui.conversation.messages_error,
                        result
                            .error
                            .or_else(|| Some("Mensagens indisponíveis.".into())),
                    );
                }
            }
            Ok(Err(error)) => {
                if epoch.load(Ordering::Acquire) == generation
                    && conversation_matches(ui, &chat, topic.as_ref())
                {
                    set_if_changed(ui.conversation.messages_error, Some(error.to_string()));
                }
            }
            Err(_) => {}
        }
    }

    // Drop Conversation lease before Interactive/Background work (read, names).
    drop(_priority);

    if epoch.load(Ordering::Acquire) == generation
        && conversation_matches(ui, &chat, topic.as_ref())
    {
        set_if_changed(ui.conversation.messages_loading, false);
    }

    // Badge clear is local; TDLib read waits on the Interactive queue.
    if unread_hint > 0 {
        let mut chats = ui.conversation.chats.peek().clone();
        if let Some(row) = chats.iter_mut().find(|item| item.id == chat.id) {
            row.unread_count = 0;
            ui.conversation.chats.set(chats);
        }
    }
    let read_services = services.clone();
    spawn(async move {
        let _ = read_services
            .telegram
            .read_chat(chat_id, last_message_id, Some(unread_hint))
            .await;
    });

    if (solid_page || painted_quick)
        && epoch.load(Ordering::Acquire) == generation
        && conversation_matches(ui, &chat, topic.as_ref())
    {
        refresh_sender_names(
            services.clone(),
            ui,
            chat.clone(),
            topic.clone(),
            epoch,
            generation,
        );
    }
}

/// Looks up sender names after the page is already visible.
fn refresh_sender_names(
    services: AppServices,
    ui: UiState,
    chat: TdlibChatInfo,
    topic: Option<TdlibForumTopicInfo>,
    epoch: Arc<AtomicU64>,
    generation: u64,
) {
    let pending = ui.conversation.messages.peek().clone();
    if pending
        .iter()
        .all(|message| message.out || !message.sender_name.trim().is_empty())
    {
        return;
    }
    spawn(async move {
        let Ok(named) = services.telegram.fill_sender_names(pending).await else {
            return;
        };
        if epoch.load(Ordering::Acquire) != generation
            || !conversation_matches(ui, &chat, topic.as_ref())
        {
            return;
        }
        let names = named
            .iter()
            .map(|message| (message.id, message.sender_name.clone()))
            .collect::<HashMap<_, _>>();
        let mut messages = ui.conversation.messages.peek().clone();
        let mut touched = false;
        for message in &mut messages {
            if message.out || !message.sender_name.trim().is_empty() {
                continue;
            }
            if let Some(name) = names.get(&message.id) {
                if !name.trim().is_empty() {
                    message.sender_name.clone_from(name);
                    touched = true;
                }
            }
        }
        if touched {
            set_if_changed(ui.conversation.messages, messages);
        }
    });
}

/// Scrolls to a reply target, loading the messages between it and the oldest
/// message currently in the timeline when the original is outside the page.
pub(super) fn jump_to_message(
    services: AppServices,
    ui: UiState,
    message_id: i64,
    mut jump_to: Signal<Option<i64>>,
    search_active: bool,
    mut loading: Signal<bool>,
    conversation_epoch: Arc<AtomicU64>,
) {
    if search_active
        || ui
            .conversation
            .messages()
            .iter()
            .any(|message| message.id == message_id)
    {
        jump_to.set(Some(message_id));
        return;
    }
    if loading() {
        push_toast(
            ui.shell.toasts,
            "Aguarde o carregamento atual.",
            ToastKind::Info,
        );
        return;
    }
    let Some(chat) = ui.conversation.selected_chat() else {
        return;
    };
    let Some(chat_id) = parse_chat_id(&chat) else {
        return;
    };
    let topic = ui.conversation.selected_topic();
    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());
    let cache_key = message_cache_key(&chat.id, topic.as_ref());
    let oldest_loaded = ui.conversation.messages().first().map(|message| message.id);
    let generation = conversation_epoch.load(Ordering::Acquire);
    loading.set(true);
    spawn(async move {
        // TDLib returns the target, the messages newer than it and one older
        // message per request. Keep advancing the newest id until the block
        // connects to the page already on screen.
        const MAX_CONTEXT_PAGES: usize = 10;
        let mut collected = Vec::new();
        let mut anchor = message_id;
        let mut connected = oldest_loaded.is_none();
        let mut error = None;
        let _priority = services.telegram.begin_conversation_priority();
        for _ in 0..MAX_CONTEXT_PAGES {
            let result = services
                .telegram
                .get_messages(
                    chat_id,
                    100,
                    anchor,
                    -99,
                    topic_id,
                    topic_kind.clone(),
                    None,
                )
                .await;
            match result {
                Ok(result) if result.success => {
                    if result.messages.is_empty() {
                        error = Some("Mensagem original indisponível.".to_string());
                        break;
                    }
                    let newest = result
                        .messages
                        .iter()
                        .map(|message| message.id)
                        .max()
                        .unwrap_or(anchor);
                    collected.extend(result.messages);
                    if let Some(oldest) = oldest_loaded {
                        if newest >= oldest {
                            connected = true;
                            break;
                        }
                    } else {
                        connected = true;
                        break;
                    }
                    if newest <= anchor {
                        break;
                    }
                    anchor = newest;
                }
                Ok(result) => {
                    error = Some(
                        result
                            .error
                            .unwrap_or_else(|| "Mensagem original indisponível.".to_string()),
                    );
                    break;
                }
                Err(err) => {
                    error = Some(err.to_string());
                    break;
                }
            }
        }
        drop(_priority);
        if conversation_epoch.load(Ordering::Acquire) != generation
            || !conversation_matches(ui, &chat, topic.as_ref())
        {
            loading.set(false);
            return;
        }
        let has_target = collected.iter().any(|message| message.id == message_id);
        if !has_target || !connected {
            loading.set(false);
            let (message, kind) = match error {
                Some(error) => (error, ToastKind::Error),
                None if !has_target => (
                    "Mensagem original indisponível.".to_string(),
                    ToastKind::Error,
                ),
                None => (
                    "A mensagem original está muito distante para abrir agora.".to_string(),
                    ToastKind::Info,
                ),
            };
            push_toast(ui.shell.toasts, message, kind);
            return;
        }
        let mut merged = ui.conversation.messages();
        merge_messages(&mut merged, collected);
        set_if_changed(ui.conversation.messages, merged.clone());
        let _ = services.telegram.cache_messages(cache_key, merged).await;
        if conversation_epoch.load(Ordering::Acquire) != generation
            || !conversation_matches(ui, &chat, topic.as_ref())
        {
            loading.set(false);
            return;
        }
        jump_to.set(Some(message_id));
        loading.set(false);
    });
}

/// Opens a Telegram link in the shell.
///
/// A previewable chat opens without joining. A join request, or an invite that
/// cannot be previewed, fills `join_offer` so the modal can ask first.
pub(super) fn open_telegram_link(
    services: AppServices,
    mut ui: UiState,
    url: String,
    mut pending_search_jump: Signal<Option<super::PendingSearchJump>>,
    mut join_offer: Signal<Option<super::join::JoinOffer>>,
) {
    let url = normalize_clicked_link(&url);
    if !plasma_telegram::looks_like_telegram_link(&url) {
        let external = if url.to_ascii_lowercase().starts_with("www.") {
            format!("https://{url}")
        } else {
            url
        };
        if let Err(error) = plasma_platform::open_url(&external) {
            push_toast(ui.shell.toasts, error, ToastKind::Error);
        }
        return;
    }
    spawn(async move {
        let target = match services.telegram.resolve_link(url).await {
            Ok(target) => target,
            Err(error) => {
                push_toast(ui.shell.toasts, error.to_string(), ToastKind::Error);
                return;
            }
        };
        if !target.success {
            push_toast(
                ui.shell.toasts,
                target
                    .error
                    .unwrap_or_else(|| "Não foi possível abrir o link.".to_string()),
                ToastKind::Error,
            );
            return;
        }
        let offer = target.needs_join.then(|| super::join::JoinOffer {
            chat_id: target.chat.as_ref().and_then(|chat| chat.id.parse().ok()),
            title: target
                .chat
                .as_ref()
                .map(|chat| chat.title.clone())
                .or(target.topic_title.clone())
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| "Chat".to_string()),
            description: target.description.clone().unwrap_or_default(),
            member_count: target.member_count,
            is_channel: target.is_channel,
            is_group: target.is_group,
            join_by_request: target.join_by_request,
            invite_link: target.invite_link.clone(),
            photo_data_url: target.photo_data_url.clone(),
            show_modal: target.chat.is_none() || target.join_by_request,
            pending: false,
        });
        let Some(mut chat) = target.chat else {
            join_offer.set(offer);
            return;
        };
        if target.topic_id.is_some() {
            chat.has_topics = true;
        }
        let mut chats = ui.conversation.chats();
        if let Some(existing) = chats.iter_mut().find(|item| item.id == chat.id) {
            *existing = chat.clone();
        } else {
            chats.insert(0, chat.clone());
        }
        set_if_changed(ui.conversation.chats, chats);
        let topic_id = target.topic_id;
        let topic_kind = target.topic_kind.clone();
        chat_list::open_chat(services, ui, chat.clone());
        if let Some(topic_id) = topic_id {
            let kind = topic_kind.clone().unwrap_or_else(|| "forum".to_string());
            ui.conversation
                .selected_topic
                .set(Some(TdlibForumTopicInfo {
                    id: topic_id,
                    kind,
                    title: target
                        .topic_title
                        .clone()
                        .filter(|title| !title.trim().is_empty())
                        .unwrap_or_else(|| {
                            search_topic_title(topic_kind.as_deref().unwrap_or("forum")).to_string()
                        }),
                    is_closed: false,
                    is_pinned: false,
                    unread_count: 0,
                }));
        }
        if let Some(message_id) = target.message_id {
            pending_search_jump.set(Some(super::PendingSearchJump {
                chat_id: chat.id,
                topic_id,
                topic_kind,
                message_id,
            }));
        }
        join_offer.set(offer);
    });
}

fn normalize_clicked_link(url: &str) -> String {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("t.me/")
        || lower.starts_with("telegram.me/")
        || lower.starts_with("telegram.dog/")
    {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    }
}

pub(super) fn load_older(
    services: AppServices,
    mut ui: UiState,
    chat: TdlibChatInfo,
    topic: Option<TdlibForumTopicInfo>,
    mut loading: Signal<bool>,
) {
    if loading() {
        return;
    }
    let Some(chat_id) = parse_chat_id(&chat) else {
        return;
    };
    let Some(oldest_id) = ui.conversation.messages().first().map(|message| message.id) else {
        return;
    };
    loading.set(true);
    let cache_key = message_cache_key(&chat.id, topic.as_ref());
    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());
    spawn(async move {
        let _priority = services.telegram.begin_conversation_priority();
        match services
            .telegram
            .get_messages(chat_id, 50, oldest_id, 0, topic_id, topic_kind, None)
            .await
        {
            Ok(result) if result.success => {
                drop(_priority);
                if !conversation_matches(ui, &chat, topic.as_ref()) {
                    loading.set(false);
                    return;
                }
                let mut merged = ui.conversation.messages();
                merge_messages(&mut merged, result.messages);
                set_if_changed(ui.conversation.messages, merged.clone());
                set_if_changed(ui.conversation.messages_has_more, result.has_more);
                let _ = services.telegram.cache_messages(cache_key, merged).await;
            }
            Ok(result) => ui.conversation.messages_error.set(
                result
                    .error
                    .or_else(|| Some("Mensagens antigas indisponíveis.".into())),
            ),
            Err(error) => ui.conversation.messages_error.set(Some(error.to_string())),
        }
        loading.set(false);
    });
}

pub(super) fn parse_chat_id(chat: &TdlibChatInfo) -> Option<i64> {
    if crate::twitter_fake::is_twitter_fake_chat_id(&chat.id) {
        Some(crate::twitter_fake::fake_chat_numeric_id(&chat.id))
    } else {
        chat.id.parse::<i64>().ok()
    }
}

pub(super) fn chat_kind(chat: &TdlibChatInfo) -> &'static str {
    if crate::twitter_fake::is_twitter_fake_chat_id(&chat.id) {
        "Twitter"
    } else if chat.is_channel {
        "Canal"
    } else if chat.is_group {
        "Grupo"
    } else {
        "Conversa privada"
    }
}

/// Fallback for a search hit whose chat is outside the loaded chat list.
pub(super) fn chat_from_global_result(item: &TdlibGlobalSearchMessage) -> TdlibChatInfo {
    TdlibChatInfo {
        id: item.chat_id.clone(),
        title: if item.chat_title.trim().is_empty() {
            "Conversa".to_string()
        } else {
            item.chat_title.clone()
        },
        date: item.message.date,
        unread_count: 0,
        is_group: item.chat_id.parse::<i64>().is_ok_and(|id| id < 0),
        is_channel: false,
        is_member: true,
        can_send_messages: false,
        can_send_media: false,
        has_topics: item.message.topic_id.is_some(),
        last_message_id: Some(item.message.id),
        last_message_text: item.message.text.clone(),
        last_message_date: item.message.date,
        last_message_has_media: item.message.has_media,
        last_message_is_video: item.message.is_video,
        last_message_is_photo: item.message.is_photo,
    }
}

pub(super) fn search_topic_title(kind: &str) -> &'static str {
    match kind {
        "direct_messages" => "Mensagens diretas",
        "saved_messages" => "Mensagens salvas",
        _ => "Tópico",
    }
}

/// One-line subtitle of the conversation header.
pub(super) fn chat_summary(chat: &TdlibChatInfo) -> String {
    let kind = if chat.has_topics {
        format!("{} com tópicos", chat_kind(chat))
    } else {
        chat_kind(chat).to_string()
    };
    format!(
        "{kind} · {}",
        chat_list::summarize_last_message(&chat.last_message_text, 80)
    )
}

fn viewing_chat(ui: UiState, chat_id_key: &str) -> bool {
    ui.conversation
        .selected_chat
        .peek()
        .as_ref()
        .is_some_and(|chat| chat.id == chat_id_key)
}

fn release_topics_fetch(mut topics_fetch: CopyValue<Option<String>>, chat_id_key: &str) {
    if topics_fetch.peek().as_deref() == Some(chat_id_key) {
        topics_fetch.set(None);
    }
}

fn store_forum_flag(
    services: &AppServices,
    mut forum_flags: CopyValue<HashMap<String, bool>>,
    chat_id_key: &str,
    chat_id: i64,
    is_forum: bool,
) {
    let mut flags = forum_flags.peek().clone();
    if flags.get(chat_id_key).copied() != Some(is_forum) {
        flags.insert(chat_id_key.to_string(), is_forum);
        forum_flags.set(flags);
    }
    remember_forum_flag(services, chat_id, is_forum);
}

/// Loads topics for the open chat.
pub(super) fn load_forum_topics(
    services: AppServices,
    mut ui: UiState,
    chat_id_key: String,
    chat_id: i64,
    known_forum: bool,
    topics_fetch: CopyValue<Option<String>>,
    forum_flags: CopyValue<HashMap<String, bool>>,
) {
    spawn(async move {
        services.telegram.wait_conversation_priority_clear().await;

        if !known_forum {
            match services.telegram.chat_is_forum(chat_id).await {
                Ok(true) => {
                    if !viewing_chat(ui, &chat_id_key) {
                        release_topics_fetch(topics_fetch, &chat_id_key);
                        return;
                    }
                    set_chat_has_topics(ui, &chat_id_key, true);
                    store_forum_flag(&services, forum_flags, &chat_id_key, chat_id, true);
                }
                Ok(false) => {
                    if viewing_chat(ui, &chat_id_key) {
                        set_chat_has_topics(ui, &chat_id_key, false);
                        ui.conversation.topics.set(Vec::new());
                        ui.conversation.topics_cache.write().remove(&chat_id_key);
                    }
                    store_forum_flag(&services, forum_flags, &chat_id_key, chat_id, false);
                    release_topics_fetch(topics_fetch, &chat_id_key);
                    return;
                }
                Err(_) => {
                    release_topics_fetch(topics_fetch, &chat_id_key);
                    return;
                }
            }
        }

        if !viewing_chat(ui, &chat_id_key) {
            release_topics_fetch(topics_fetch, &chat_id_key);
            return;
        }

        match services.telegram.get_forum_topics(chat_id, 200, true).await {
            Ok(result) if result.success && result.is_forum && viewing_chat(ui, &chat_id_key) => {
                set_if_changed(ui.conversation.topics, result.topics.clone());
                remember_forum_topics(&services, ui, &chat_id_key, result.topics);
                set_chat_has_topics(ui, &chat_id_key, true);
                store_forum_flag(&services, forum_flags, &chat_id_key, chat_id, true);
            }
            Ok(result) if result.success && viewing_chat(ui, &chat_id_key) => {
                set_chat_has_topics(ui, &chat_id_key, false);
                ui.conversation.topics.set(Vec::new());
                ui.conversation.topics_cache.write().remove(&chat_id_key);
                store_forum_flag(&services, forum_flags, &chat_id_key, chat_id, false);
            }
            _ => {}
        }
        release_topics_fetch(topics_fetch, &chat_id_key);
    });
}

fn conversation_matches(
    ui: UiState,
    chat: &TdlibChatInfo,
    topic: Option<&TdlibForumTopicInfo>,
) -> bool {
    ui.conversation.selected_chat().map(|item| item.id) == Some(chat.id.clone())
        && ui
            .conversation
            .selected_topic()
            .map(|item| (item.id, item.kind))
            == topic.map(|item| (item.id, item.kind.clone()))
}
