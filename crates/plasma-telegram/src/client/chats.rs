use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::time::Duration;

#[cfg(debug_assertions)]
use std::time::Instant;
use tokio::task::JoinSet;

use tdlib_rs::{enums::ChatList, functions};

use super::*;

pub async fn tdlib_get_chats(
    state: &TdlibManager,
    limit: Option<i32>,
    soft: bool,
) -> plasma_core::Result<TdlibChatsResult> {
    with_priority(
        &state.queue,
        TdlibPriority::Background,
        "get_chats",
        || async move {
            #[cfg(debug_assertions)]
            let started_at = Instant::now();
            let client_id = match ready_client_id(state).await {
                Ok(client_id) => client_id,
                Err(status) => {
                    return Ok(TdlibChatsResult {
                        success: false,
                        dialogs: Vec::new(),
                        has_more: false,
                        error: status.error.or(Some(status.state)),
                    });
                }
            };

            // Soft reconciles stay cheap so they never bury an open conversation.
            // Full loads keep the Main+Archive window and contact fill.
            let limit = if soft {
                limit.unwrap_or(20).clamp(10, 40)
            } else {
                limit.unwrap_or(80).clamp(20, 5000)
            };
            let (main_limit, archive_limit, load_limit) = if soft {
                (limit, 0, limit.min(40))
            } else {
                let main_limit = ((limit * 4) / 5).max(1);
                let archive_limit = (limit - main_limit).max(1);
                (main_limit, archive_limit, limit.min(100))
            };
            let _ = functions::load_chats(Some(ChatList::Main), load_limit, client_id).await;
            if archive_limit > 0 {
                let _ = functions::load_chats(Some(ChatList::Archive), load_limit, client_id).await;
            }
            #[cfg(debug_assertions)]
            let loaded_at = Instant::now();
            let main_chat_ids =
                match functions::get_chats(Some(ChatList::Main), main_limit, client_id).await {
                    Ok(tdlib_rs::enums::Chats::Chats(chats)) => chats.chat_ids,
                    Err(error) => {
                        return Ok(TdlibChatsResult {
                            success: false,
                            dialogs: Vec::new(),
                            has_more: false,
                            error: Some(error.message),
                        });
                    }
                };

            let archive_chat_ids = if archive_limit > 0 {
                if let Ok(tdlib_rs::enums::Chats::Chats(chats)) =
                    functions::get_chats(Some(ChatList::Archive), archive_limit, client_id).await
                {
                    chats.chat_ids
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            let has_more = main_chat_ids.len() >= main_limit as usize
                || archive_chat_ids.len() >= archive_limit as usize;
            let mut chat_ids = main_chat_ids;
            chat_ids.extend(archive_chat_ids);
            #[cfg(debug_assertions)]
            let listed_at = Instant::now();

            let mut dialogs = Vec::new();
            let mut seen_chat_ids = HashMap::<String, bool>::new();
            // One getChat at a time turned a cold open of ~120 dialogs into tens of
            // seconds on the only TDLib client, long enough for the window to be
            // marked unresponsive. TDLib runs the requests concurrently; the batch
            // keeps their original order.
            let mut pending_ids = Vec::new();
            let mut queued_ids = HashSet::new();
            for chat_id in chat_ids {
                if queued_ids.insert(chat_id) {
                    pending_ids.push(chat_id);
                }
            }
            let resolve_batch = if soft { 4 } else { 8 };
            for batch in pending_ids.chunks(resolve_batch) {
                let mut tasks = JoinSet::new();
                for (index, chat_id) in batch.iter().copied().enumerate() {
                    tasks.spawn(async move {
                        let chat = match functions::get_chat(chat_id, client_id).await {
                            Ok(tdlib_rs::enums::Chat::Chat(chat)) => Some(tdlib_chat_to_app(chat)),
                            Err(_) => None,
                        };
                        (index, chat)
                    });
                }
                let mut resolved = Vec::new();
                while let Some(joined) = tasks.join_next().await {
                    if let Ok((index, Some(chat))) = joined {
                        resolved.push((index, chat));
                    }
                }
                resolved.sort_by_key(|(index, _)| *index);
                for (_, chat) in resolved {
                    seen_chat_ids.insert(chat.id.clone(), true);
                    dialogs.push(chat);
                }
            }
            #[cfg(debug_assertions)]
            let chats_resolved_at = Instant::now();
            #[cfg(debug_assertions)]
            let resolved_chats = dialogs.len();

            if !soft {
                let remaining_contact_slots = (limit as usize).saturating_sub(dialogs.len());
                if remaining_contact_slots > 0 {
                    if let Ok(tdlib_rs::enums::Users::Users(users)) =
                        functions::get_contacts(client_id).await
                    {
                        let mut contact_dialogs = Vec::new();
                        // Contacts are a fallback for sparse dialog lists. Resolving every
                        // contact one by one was the dominant cold-start cost for larger
                        // address books.
                        let candidates: Vec<i64> = users
                            .user_ids
                            .into_iter()
                            .filter(|user_id| !seen_chat_ids.contains_key(&user_id.to_string()))
                            .take(remaining_contact_slots)
                            .collect();
                        for batch in candidates.chunks(8) {
                            let mut tasks = JoinSet::new();
                            for user_id in batch.iter().copied() {
                                tasks.spawn(async move {
                                    let Ok(tdlib_rs::enums::User::User(user)) =
                                        functions::get_user(user_id, client_id).await
                                    else {
                                        return None;
                                    };
                                    user.have_access.then(|| tdlib_user_to_contact_chat(user))
                                });
                            }
                            while let Some(joined) = tasks.join_next().await {
                                if let Ok(Some(chat)) = joined {
                                    seen_chat_ids.insert(chat.id.clone(), true);
                                    contact_dialogs.push(chat);
                                }
                            }
                        }
                        contact_dialogs.sort_by_key(|left| left.title.to_lowercase());
                        dialogs.extend(contact_dialogs);
                    }
                }
            }
            #[cfg(debug_assertions)]
            {
                let load_ms = loaded_at.duration_since(started_at).as_millis() as u64;
                let list_ms = listed_at.duration_since(loaded_at).as_millis() as u64;
                let resolve_ms = chats_resolved_at.duration_since(listed_at).as_millis() as u64;
                let contacts_ms = chats_resolved_at.elapsed().as_millis() as u64;
                let total_ms = started_at.elapsed().as_millis() as u64;
                let contacts = dialogs.len().saturating_sub(resolved_chats);
                if STARTUP_TIMING_LOGGED.swap(true, Ordering::Relaxed) {
                    tracing::debug!(
                        soft,
                        load_ms,
                        list_ms,
                        resolve_ms,
                        contacts_ms,
                        total_ms,
                        chats = resolved_chats,
                        contacts,
                        "Telegram chats refreshed"
                    );
                } else {
                    tracing::info!(
                        soft,
                        load_ms,
                        list_ms,
                        resolve_ms,
                        contacts_ms,
                        total_ms,
                        chats = resolved_chats,
                        contacts,
                        "Telegram chats loaded"
                    );
                }
            }

            Ok(TdlibChatsResult {
                success: true,
                dialogs,
                has_more,
                error: None,
            })
        },
    )
    .await
}

/// Fetches one chat as the UI DTO, including capabilities and topic support.
///
/// Global search can return messages from conversations outside the loaded
/// chat list. Opening one of those results needs the real chat metadata,
/// otherwise the composer would be disabled by the fallback DTO.
pub async fn tdlib_get_chat(
    state: &TdlibManager,
    chat_id: i64,
) -> plasma_core::Result<TdlibChatInfo> {
    let client_id = ready_client_id(state)
        .await
        .map_err(|status| status.error.unwrap_or(status.state))?;
    match functions::get_chat(chat_id, client_id).await {
        Ok(tdlib_rs::enums::Chat::Chat(chat)) => Ok(tdlib_chat_to_app(chat)),
        Err(error) => Err(error.message.into()),
    }
}

pub async fn tdlib_read_chat(
    state: &TdlibManager,
    chat_id: i64,
    last_message_id: Option<i64>,
    unread_hint: Option<i32>,
) -> plasma_core::Result<()> {
    // Prefer the tip id from the chat list so opening a conversation does not
    // spend a getChat hop on the only TDLib client during history load.
    if unread_hint == Some(0) {
        return Ok(());
    }
    with_priority(
        &state.queue,
        TdlibPriority::Interactive,
        "read_chat",
        || async move {
            let client_id = ready_client_id(state)
                .await
                .map_err(|status| status.error.unwrap_or(status.state))?;
            if let Some(message_id) = last_message_id.filter(|_| unread_hint.unwrap_or(1) > 0) {
                let _ = functions::view_messages(chat_id, vec![message_id], None, true, client_id)
                    .await;
                return Ok(());
            }

            let tdlib_rs::enums::Chat::Chat(chat) = functions::get_chat(chat_id, client_id)
                .await
                .map_err(|error| error.message)?;

            if chat.unread_count > 0 {
                if let Some(message) = chat.last_message {
                    functions::view_messages(chat_id, vec![message.id], None, true, client_id)
                        .await
                        .map_err(|error| error.message)?;
                }
            }
            if chat.is_marked_as_unread {
                functions::toggle_chat_is_marked_as_unread(chat_id, false, client_id)
                    .await
                    .map_err(|error| error.message)?;
            }
            Ok(())
        },
    )
    .await
}

pub async fn tdlib_read_all_chats(state: &TdlibManager) -> plasma_core::Result<()> {
    let client_id = ready_client_id(state)
        .await
        .map_err(|status| status.error.unwrap_or(status.state))?;
    functions::read_chat_list(ChatList::Main, client_id)
        .await
        .map_err(|error| error.message)?;
    functions::read_chat_list(ChatList::Archive, client_id)
        .await
        .map_err(|error| error.message)?;
    Ok(())
}

/// Fetches a single message by its app-level id.
///
/// The timeline uses this to show the quoted message behind a reply when the
/// original is outside the currently loaded page.
pub async fn tdlib_get_message(
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<TdlibMessageInfo> {
    with_priority(
        &state.queue,
        TdlibPriority::Background,
        "get_message",
        || async move {
            let client_id = match ready_client_id(state).await {
                Ok(client_id) => client_id,
                Err(status) => return Err(status.error.unwrap_or(status.state).into()),
            };
            let tdlib_id = tdlib_message_id_from_app_id(message_id);
            let raw = match functions::get_message(chat_id, tdlib_id, client_id).await {
                Ok(tdlib_rs::enums::Message::Message(message)) => message,
                Err(_) if tdlib_id != message_id => {
                    match functions::get_message(chat_id, message_id, client_id).await {
                        Ok(tdlib_rs::enums::Message::Message(message)) => message,
                        Err(error) => return Err(error.message.into()),
                    }
                }
                Err(error) => return Err(error.message.into()),
            };
            let mut message = tdlib_message_to_app(raw);
            let sender_name = resolve_sender_name(client_id, &message.sender_id).await;
            if !sender_name.trim().is_empty() {
                message.sender_name = sender_name;
            }
            Ok(message)
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn tdlib_get_messages(
    state: &TdlibManager,
    chat_id: i64,
    limit: Option<i32>,
    offset_id: Option<i64>,
    offset: Option<i32>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
    max_rounds: Option<usize>,
) -> plasma_core::Result<TdlibMessagesResult> {
    // History itself does not take a Conversation lease: the UI holds one for
    // the open-chat window. Wrapping every get_messages call nested another
    // lease per concurrent soft-cancelled load and blew the counter into the
    // tens of thousands while Background work stayed starved.
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibMessagesResult {
                success: false,
                messages: Vec::new(),
                has_more: false,
                oldest_message_id: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let history_offset = offset.unwrap_or(0).clamp(-99, 0);
    let requested_limit = limit.unwrap_or(50).clamp((-history_offset).max(1), 100);
    let mut from_message_id = offset_id.map(tdlib_message_id_from_app_id).unwrap_or(0);
    let forum_topic_id = match topic_id {
        Some(topic_id) if topic_kind.as_deref() != Some("thread") => {
            Some(i32::try_from(topic_id).map_err(|_| "ID de tópico inválido")?)
        }
        _ => None,
    };
    let mut seen_message_ids = HashSet::new();
    // A página anterior já contém a âncora usada para buscar mensagens antigas.
    if from_message_id != 0 && history_offset == 0 {
        seen_message_ids.insert(from_message_id);
    }
    let mut raw_messages = Vec::new();
    let mut has_more_hint = false;
    let mut stalled_requests = 0;

    // Tell TDLib this chat is active before history. Without it, remote pages
    // can sit behind the account sync and the UI stays on "Carregando…".
    let _ = functions::open_chat(chat_id, client_id).await;

    // Prefer disk-local history for the tip of a plain chat so the first paint
    // never waits on the network while get_chats / avatars still run.
    let quick_open = max_rounds.is_some_and(|rounds| rounds <= 2)
        && topic_id.is_none()
        && from_message_id == 0
        && history_offset == 0;
    if quick_open {
        if let Ok(tdlib_rs::enums::Messages::Messages(history)) =
            functions::get_chat_history(chat_id, 0, 0, requested_limit, true, client_id).await
        {
            for message in history.messages.into_iter().flatten() {
                if !seen_message_ids.insert(message.id)
                    || is_hidden_membership_service_message(&message.content)
                {
                    continue;
                }
                from_message_id = message.id;
                raw_messages.push(message);
            }
            if !raw_messages.is_empty() {
                let mut messages: Vec<TdlibMessageInfo> =
                    raw_messages.into_iter().map(tdlib_message_to_app).collect();
                messages.sort_by_key(|message| {
                    let id_order = if message.id < 0 {
                        (1_u8, message.id.unsigned_abs())
                    } else {
                        (0_u8, message.id as u64)
                    };
                    (message.date, id_order)
                });
                let oldest_message_id = messages.first().map(|message| message.id);
                #[cfg(debug_assertions)]
                tracing::info!(
                    chat_id,
                    count = messages.len(),
                    leases = state.queue.conversation_leases(),
                    low_running = state.queue.low_running(),
                    "Telegram messages loaded from local cache"
                );
                return Ok(TdlibMessagesResult {
                    success: true,
                    messages,
                    // Force a network follow-up after the spinner is gone.
                    has_more: true,
                    oldest_message_id,
                    error: None,
                });
            }
        }
    }

    // TDLib escolhe quantas mensagens devolver e pode retornar só uma mesmo
    // quando o limite é 50. Continue pela âncora até preencher a página ou
    // chegar ao fim, preservando a busca contextual com offset negativo.
    let round_cap = max_rounds.unwrap_or(12).clamp(1, 12);
    for request_index in 0..round_cap {
        let remaining = requested_limit as usize - raw_messages.len();
        let page_limit = if history_offset < 0 {
            requested_limit
        } else {
            (remaining + usize::from(from_message_id != 0)).min(100) as i32
        };
        let history_result = if let Some(topic_id) = topic_id {
            if topic_kind.as_deref() == Some("thread") {
                functions::get_message_thread_history(
                    chat_id,
                    tdlib_message_id_from_app_id(topic_id),
                    from_message_id,
                    history_offset,
                    page_limit,
                    client_id,
                )
                .await
            } else {
                functions::get_forum_topic_history(
                    chat_id,
                    forum_topic_id.expect("forum topic ID validated"),
                    from_message_id,
                    history_offset,
                    page_limit,
                    client_id,
                )
                .await
            }
        } else {
            functions::get_chat_history(
                chat_id,
                from_message_id,
                history_offset,
                page_limit,
                false,
                client_id,
            )
            .await
        };

        let history = match history_result {
            Ok(tdlib_rs::enums::Messages::Messages(messages)) => messages,
            Err(error) if raw_messages.is_empty() => {
                return Ok(TdlibMessagesResult {
                    success: false,
                    messages: Vec::new(),
                    has_more: false,
                    oldest_message_id: None,
                    error: Some(error.message),
                });
            }
            Err(_) => {
                has_more_hint = true;
                break;
            }
        };

        has_more_hint = history.total_count > history.messages.len() as i32;
        let mut oldest_in_batch = None;
        let mut added = 0;
        for message in history.messages.into_iter().flatten() {
            oldest_in_batch = Some(message.id);
            if !seen_message_ids.insert(message.id)
                || is_hidden_membership_service_message(&message.content)
            {
                continue;
            }
            raw_messages.push(message);
            added += 1;
        }

        if history_offset < 0 || raw_messages.len() >= requested_limit as usize {
            break;
        }
        if request_index + 1 == round_cap && added > 0 {
            has_more_hint = true;
        }
        let Some(oldest_in_batch) = oldest_in_batch else {
            break;
        };
        if oldest_in_batch == from_message_id && added == 0 {
            stalled_requests += 1;
            if stalled_requests >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        } else {
            stalled_requests = 0;
            from_message_id = oldest_in_batch;
        }
    }

    let collected_count = raw_messages.len();
    let mut messages: Vec<TdlibMessageInfo> =
        raw_messages.into_iter().map(tdlib_message_to_app).collect();

    // Names are resolved after the page is on screen. Doing it here held the
    // only TDLib client until every sender was known, so opening a busy chat
    // froze the window behind the history request.
    messages.sort_by_key(|message| {
        let id_order = if message.id < 0 {
            (1_u8, message.id.unsigned_abs())
        } else {
            (0_u8, message.id as u64)
        };
        (message.date, id_order)
    });
    let oldest_message_id = messages.first().map(|message| message.id);
    let has_more = collected_count as i32 >= requested_limit || has_more_hint;

    #[cfg(debug_assertions)]
    tracing::info!(
        chat_id,
        count = messages.len(),
        has_more,
        rounds = round_cap,
        "Telegram messages loaded"
    );

    Ok(TdlibMessagesResult {
        success: true,
        messages,
        has_more,
        oldest_message_id,
        error: None,
    })
}

/// Fills `sender_name` for messages that were shown before the lookup finished.
pub async fn tdlib_fill_sender_names(
    state: &TdlibManager,
    mut messages: Vec<TdlibMessageInfo>,
) -> plasma_core::Result<Vec<TdlibMessageInfo>> {
    with_priority(
        &state.queue,
        TdlibPriority::Background,
        "fill_sender_names",
        || async move {
            let client_id = ready_client_id(state)
                .await
                .map_err(|status| status.error.unwrap_or(status.state))?;
            resolve_message_sender_names(client_id, &mut messages).await;
            Ok(messages)
        },
    )
    .await
}

pub async fn tdlib_search_chat_messages(
    state: &TdlibManager,
    chat_id: i64,
    query: String,
    limit: Option<i32>,
    from_message_id: Option<i64>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> plasma_core::Result<TdlibSearchMessagesResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibSearchMessagesResult {
                success: false,
                messages: Vec::new(),
                total_count: 0,
                next_from_message_id: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let trimmed_query = query.trim().to_string();
    if trimmed_query.is_empty() {
        return Ok(TdlibSearchMessagesResult {
            success: true,
            messages: Vec::new(),
            total_count: 0,
            next_from_message_id: None,
            error: None,
        });
    }

    let topic = message_topic_from_id(topic_id, topic_kind.as_deref());
    let search_from_message_id = from_message_id
        .map(tdlib_message_id_from_app_id)
        .unwrap_or(0);
    let requested_limit = limit.unwrap_or(50).clamp(1, 100);

    match functions::search_chat_messages(
        chat_id,
        topic,
        trimmed_query,
        None,
        search_from_message_id,
        0,
        requested_limit,
        None,
        client_id,
    )
    .await
    {
        Ok(tdlib_rs::enums::FoundChatMessages::FoundChatMessages(found)) => {
            let raw_messages: Vec<tdlib_rs::types::Message> = found
                .messages
                .into_iter()
                .filter(|message| !is_hidden_membership_service_message(&message.content))
                .collect();

            let mut messages: Vec<TdlibMessageInfo> =
                raw_messages.into_iter().map(tdlib_message_to_app).collect();
            resolve_message_sender_names(client_id, &mut messages).await;
            messages.sort_by_key(|message| {
                let id_order = if message.id < 0 {
                    (1_u8, message.id.unsigned_abs())
                } else {
                    (0_u8, message.id as u64)
                };
                (message.date, id_order)
            });

            Ok(TdlibSearchMessagesResult {
                success: true,
                messages,
                total_count: found.total_count,
                next_from_message_id: if found.next_from_message_id != 0 {
                    Some(app_message_id_from_tdlib(found.next_from_message_id))
                } else {
                    None
                },
                error: None,
            })
        }
        Err(error) => Ok(TdlibSearchMessagesResult {
            success: false,
            messages: Vec::new(),
            total_count: 0,
            next_from_message_id: None,
            error: Some(error.message),
        }),
    }
}

/// Searches messages across every chat visible to the current account.
pub async fn tdlib_search_messages(
    state: &TdlibManager,
    query: String,
    limit: Option<i32>,
    offset: Option<String>,
) -> plasma_core::Result<TdlibGlobalSearchResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibGlobalSearchResult {
                success: false,
                messages: Vec::new(),
                total_count: 0,
                next_offset: String::new(),
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let trimmed_query = query.trim().to_string();
    if trimmed_query.is_empty() {
        return Ok(TdlibGlobalSearchResult {
            success: true,
            messages: Vec::new(),
            total_count: 0,
            next_offset: String::new(),
            error: None,
        });
    }

    let requested_limit = limit.unwrap_or(50).clamp(1, 100);
    let search_offset = offset.unwrap_or_default();
    match functions::search_messages(
        None,
        trimmed_query,
        search_offset,
        requested_limit,
        Some(tdlib_rs::enums::SearchMessagesFilter::Empty),
        None,
        0,
        0,
        client_id,
    )
    .await
    {
        Ok(tdlib_rs::enums::FoundMessages::FoundMessages(found)) => {
            let mut entries: Vec<TdlibGlobalSearchMessage> = found
                .messages
                .into_iter()
                .filter(|message| !is_hidden_membership_service_message(&message.content))
                .map(|message| TdlibGlobalSearchMessage {
                    chat_id: message.chat_id.to_string(),
                    chat_title: String::new(),
                    message: tdlib_message_to_app(message),
                })
                .collect();

            let mut messages: Vec<TdlibMessageInfo> =
                entries.iter().map(|entry| entry.message.clone()).collect();
            resolve_message_sender_names(client_id, &mut messages).await;
            for (entry, message) in entries.iter_mut().zip(messages) {
                entry.message = message;
            }
            resolve_chat_titles(client_id, &mut entries).await;
            entries.sort_by(|left, right| {
                right
                    .message
                    .date
                    .cmp(&left.message.date)
                    .then_with(|| right.message.id.cmp(&left.message.id))
            });

            Ok(TdlibGlobalSearchResult {
                success: true,
                messages: entries,
                total_count: found.total_count,
                next_offset: found.next_offset,
                error: None,
            })
        }
        Err(error) => Ok(TdlibGlobalSearchResult {
            success: false,
            messages: Vec::new(),
            total_count: 0,
            next_offset: String::new(),
            error: Some(error.message),
        }),
    }
}
