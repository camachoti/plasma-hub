use std::collections::HashSet;

use tdlib_rs::{enums::ChatType, functions};

use super::*;

/// True when the chat is a forum supergroup.
///
/// This only resolves the chat and its supergroup. It does not page the topic
/// list, so the UI can mark a forum before the topics themselves arrive.
pub async fn tdlib_chat_is_forum(state: &TdlibManager, chat_id: i64) -> plasma_core::Result<bool> {
    with_priority(
        &state.queue,
        TdlibPriority::Background,
        "chat_is_forum",
        || async move {
            let client_id = ready_client_id(state)
                .await
                .map_err(|status| status.error.unwrap_or(status.state))?;
            chat_is_forum(client_id, chat_id).await
        },
    )
    .await
}

async fn chat_is_forum(client_id: i32, chat_id: i64) -> plasma_core::Result<bool> {
    match functions::get_chat(chat_id, client_id).await {
        Ok(tdlib_rs::enums::Chat::Chat(chat)) => match chat.r#type {
            ChatType::Supergroup(group) => {
                match functions::get_supergroup(group.supergroup_id, client_id).await {
                    Ok(tdlib_rs::enums::Supergroup::Supergroup(info)) => Ok(info.is_forum),
                    Err(error) => Err(error.message.into()),
                }
            }
            _ => Ok(false),
        },
        Err(error) => Err(error.message.into()),
    }
}

pub async fn tdlib_get_forum_topics(
    state: &TdlibManager,
    chat_id: i64,
    limit: Option<i32>,
    assume_forum: bool,
) -> plasma_core::Result<TdlibForumTopicsResult> {
    with_priority(
        &state.queue,
        TdlibPriority::Background,
        "get_forum_topics",
        || async move {
            let client_id = match ready_client_id(state).await {
                Ok(client_id) => client_id,
                Err(status) => {
                    return Ok(TdlibForumTopicsResult {
                        success: false,
                        is_forum: false,
                        topics: Vec::new(),
                        error: status.error.or(Some(status.state)),
                    });
                }
            };

            let is_forum = if assume_forum {
                true
            } else {
                match chat_is_forum(client_id, chat_id).await {
                    Ok(is_forum) => is_forum,
                    Err(error) => {
                        return Ok(TdlibForumTopicsResult {
                            success: false,
                            is_forum: false,
                            topics: Vec::new(),
                            error: Some(error.to_string()),
                        });
                    }
                }
            };

            if !is_forum {
                return Ok(TdlibForumTopicsResult {
                    success: true,
                    is_forum: false,
                    topics: Vec::new(),
                    error: None,
                });
            }

            let page_size = limit.unwrap_or(100).clamp(1, 100);
            // `limit` caps the whole list. Walking every page before returning made a
            // forum look like an ordinary group until the last page arrived.
            let max_topics = limit.map(|value| usize::try_from(value.max(1)).unwrap_or(usize::MAX));
            let mut offset_date = 0;
            let mut offset_message_id = 0;
            let mut offset_forum_topic_id = 0;
            let mut seen_cursors = HashSet::new();
            let mut all_topics = Vec::new();
            loop {
                let cursor = (offset_date, offset_message_id, offset_forum_topic_id);
                if !seen_cursors.insert(cursor) {
                    break;
                }

                match functions::get_forum_topics(
                    chat_id,
                    String::new(),
                    offset_date,
                    offset_message_id,
                    offset_forum_topic_id,
                    page_size,
                    client_id,
                )
                .await
                {
                    Ok(tdlib_rs::enums::ForumTopics::ForumTopics(page)) => {
                        if page.topics.is_empty() {
                            break;
                        }

                        all_topics.extend(page.topics);
                        if max_topics.is_some_and(|max| all_topics.len() >= max) {
                            break;
                        }
                        let next_cursor = (
                            page.next_offset_date,
                            page.next_offset_message_id,
                            page.next_offset_forum_topic_id,
                        );
                        if next_cursor == cursor || next_cursor == (0, 0, 0) {
                            break;
                        }
                        offset_date = next_cursor.0;
                        offset_message_id = next_cursor.1;
                        offset_forum_topic_id = next_cursor.2;
                    }
                    Err(error) => {
                        return Ok(TdlibForumTopicsResult {
                            success: false,
                            is_forum: true,
                            topics: Vec::new(),
                            error: Some(error.message),
                        });
                    }
                }
            }

            let mut seen_topics = HashSet::new();
            all_topics.retain(|topic| seen_topics.insert(topic.info.forum_topic_id));
            if let Some(max) = max_topics {
                all_topics.truncate(max);
            }
            Ok(TdlibForumTopicsResult {
                success: true,
                is_forum: true,
                topics: all_topics
                    .into_iter()
                    .map(|topic| TdlibForumTopicInfo {
                        id: i64::from(topic.info.forum_topic_id),
                        kind: "forum".to_string(),
                        title: topic.info.name,
                        is_closed: topic.info.is_closed,
                        is_pinned: topic.is_pinned,
                        unread_count: topic.unread_count,
                    })
                    .collect(),
                error: None,
            })
        },
    )
    .await
}

pub async fn tdlib_topic_has_new_media(
    state: &TdlibManager,
    chat_id: i64,
    topic_id: i64,
    topic_kind: Option<String>,
    after_message_id: i64,
) -> plasma_core::Result<bool> {
    let client_id = ready_client_id(state)
        .await
        .map_err(|status| status.error.unwrap_or(status.state))?;
    let baseline_id = tdlib_message_id_from_app_id(after_message_id);
    let mut from_message_id = 0;

    loop {
        let result = if topic_kind.as_deref() == Some("thread") {
            functions::get_message_thread_history(
                chat_id,
                tdlib_message_id_from_app_id(topic_id),
                from_message_id,
                0,
                100,
                client_id,
            )
            .await
        } else {
            let forum_topic_id = i32::try_from(topic_id).map_err(|_| "ID de tópico inválido")?;
            functions::get_forum_topic_history(
                chat_id,
                forum_topic_id,
                from_message_id,
                0,
                100,
                client_id,
            )
            .await
        }
        .map_err(|error| error.message)?;

        let tdlib_rs::enums::Messages::Messages(history) = result;
        if history.messages.is_empty() {
            return Ok(false);
        }

        let mut next_from_message_id = from_message_id;
        let mut reached_baseline = false;
        for message in history.messages.into_iter().flatten() {
            next_from_message_id = message.id;
            if message.id <= baseline_id {
                reached_baseline = true;
                continue;
            }
            if message_media_file(message.content, message.id).is_some() {
                return Ok(true);
            }
        }
        if reached_baseline || next_from_message_id == 0 || next_from_message_id == from_message_id
        {
            return Ok(false);
        }
        from_message_id = next_from_message_id;
    }
}
