use tdlib_rs::{enums::ChatType, functions};

use super::*;

pub async fn tdlib_get_chat_capabilities(
    state: &TdlibManager,
    chat_id: i64,
) -> plasma_core::Result<TdlibChatCapabilitiesResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibChatCapabilitiesResult {
                success: false,
                chat_id,
                is_member: false,
                can_send_messages: false,
                can_send_media: false,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let chat = match functions::get_chat(chat_id, client_id).await {
        Ok(tdlib_rs::enums::Chat::Chat(chat)) => chat,
        Err(error) => {
            return Ok(TdlibChatCapabilitiesResult {
                success: false,
                chat_id,
                is_member: false,
                can_send_messages: false,
                can_send_media: false,
                error: Some(error.message),
            });
        }
    };

    let (is_member, can_send_messages, can_send_media) = match &chat.r#type {
        ChatType::BasicGroup(group) => {
            match functions::get_basic_group(group.basic_group_id, client_id).await {
                Ok(tdlib_rs::enums::BasicGroup::BasicGroup(group_info)) => {
                    member_status_capabilities(&group_info.status, false, &chat.permissions)
                }
                Err(_) => chat_send_capabilities(&chat.r#type, &chat.permissions),
            }
        }
        ChatType::Supergroup(group) => {
            match functions::get_supergroup(group.supergroup_id, client_id).await {
                Ok(tdlib_rs::enums::Supergroup::Supergroup(group_info)) => {
                    member_status_capabilities(
                        &group_info.status,
                        group_info.is_channel,
                        &chat.permissions,
                    )
                }
                Err(_) => chat_send_capabilities(&chat.r#type, &chat.permissions),
            }
        }
        ChatType::Private(_) | ChatType::Secret(_) => {
            chat_send_capabilities(&chat.r#type, &chat.permissions)
        }
    };

    Ok(TdlibChatCapabilitiesResult {
        success: true,
        chat_id,
        is_member,
        can_send_messages,
        can_send_media,
        error: None,
    })
}

pub async fn tdlib_get_shared_media(
    state: &TdlibManager,
    chat_id: i64,
    limit: Option<i32>,
) -> plasma_core::Result<TdlibSharedMediaResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibSharedMediaResult {
                success: false,
                media: Vec::new(),
                next_from_message_id: 0,
                has_more: false,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let requested_limit = limit.unwrap_or(12).clamp(1, 50) as usize;
    let mut from_message_id = 0;
    let mut scanned = 0usize;
    let mut media = Vec::new();
    let mut exhausted = false;

    while media.len() < requested_limit && scanned < 1000 {
        let history =
            match functions::get_chat_history(chat_id, from_message_id, 0, 100, false, client_id)
                .await
            {
                Ok(tdlib_rs::enums::Messages::Messages(messages)) => messages,
                Err(error) => {
                    return Ok(TdlibSharedMediaResult {
                        success: false,
                        media,
                        next_from_message_id: app_message_id_from_tdlib(from_message_id),
                        has_more: from_message_id != 0,
                        error: Some(error.message),
                    });
                }
            };

        if history.messages.is_empty() {
            exhausted = true;
            break;
        }

        let mut next_from_message_id = from_message_id;
        for message in history.messages.into_iter().flatten() {
            next_from_message_id = message.id;
            scanned += 1;

            let (has_media, is_photo, is_video, _, media_size) =
                content_media_flags(&message.content);
            if has_media && (is_photo || is_video) {
                media.push(TdlibSharedMediaItem {
                    id: app_message_id_from_tdlib(message.id),
                    date: message.date,
                    sender_id: sender_id_to_string(&message.sender_id),
                    sender_name: String::new(),
                    has_media,
                    is_photo,
                    is_video,
                    grouped_id: if message.media_album_id != 0 {
                        Some(message.media_album_id.to_string())
                    } else {
                        None
                    },
                    media_size,
                });
                if media.len() >= requested_limit {
                    break;
                }
            }
        }

        if next_from_message_id == from_message_id {
            exhausted = true;
            break;
        }
        from_message_id = next_from_message_id;
    }

    Ok(TdlibSharedMediaResult {
        success: true,
        media,
        next_from_message_id: app_message_id_from_tdlib(from_message_id),
        has_more: !exhausted && from_message_id != 0,
        error: None,
    })
}

pub async fn tdlib_search_user_media(
    state: &TdlibManager,
    chat_id: i64,
    user_id: String,
    limit: Option<i32>,
    from_message_id: Option<i64>,
) -> plasma_core::Result<TdlibSharedMediaResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibSharedMediaResult {
                success: false,
                media: Vec::new(),
                next_from_message_id: 0,
                has_more: false,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let requested_limit = limit.unwrap_or(30).clamp(1, 100) as usize;
    let target_user_id = user_id
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect::<String>();
    // The cursor travels as an app message id so the UI can pass it back
    // opaquely; TDLib needs its own id space.
    let mut from_message_id = from_message_id
        .filter(|cursor| *cursor > 0)
        .map(tdlib_message_id_from_app_id)
        .unwrap_or(0);
    let mut scanned = 0usize;
    let mut media = Vec::new();
    let mut exhausted = false;

    // Scan a bounded chunk per page. The panel renders the first page as soon
    // as it arrives and keeps requesting pages in the background, so a sender
    // with media deep in the history no longer blocks the first result.
    while media.len() < requested_limit && scanned < 600 {
        let history =
            match functions::get_chat_history(chat_id, from_message_id, 0, 100, false, client_id)
                .await
            {
                Ok(tdlib_rs::enums::Messages::Messages(messages)) => messages,
                Err(error) => {
                    return Ok(TdlibSharedMediaResult {
                        success: false,
                        media,
                        next_from_message_id: app_message_id_from_tdlib(from_message_id),
                        has_more: from_message_id != 0,
                        error: Some(error.message),
                    });
                }
            };

        if history.messages.is_empty() {
            exhausted = true;
            break;
        }

        let mut next_from_message_id = from_message_id;
        for message in history.messages.into_iter().flatten() {
            next_from_message_id = message.id;
            scanned += 1;

            if sender_id_to_string(&message.sender_id) != target_user_id {
                continue;
            }

            let (has_media, is_photo, is_video, _, media_size) =
                content_media_flags(&message.content);
            if has_media && (is_photo || is_video) {
                media.push(TdlibSharedMediaItem {
                    id: app_message_id_from_tdlib(message.id),
                    date: message.date,
                    sender_id: target_user_id.clone(),
                    sender_name: String::new(),
                    has_media,
                    is_photo,
                    is_video,
                    grouped_id: if message.media_album_id != 0 {
                        Some(message.media_album_id.to_string())
                    } else {
                        None
                    },
                    media_size,
                });
                if media.len() >= requested_limit {
                    break;
                }
            }
        }

        if next_from_message_id == 0 || next_from_message_id == from_message_id {
            exhausted = true;
            break;
        }
        from_message_id = next_from_message_id;
    }

    Ok(TdlibSharedMediaResult {
        success: true,
        media,
        next_from_message_id: app_message_id_from_tdlib(from_message_id),
        has_more: !exhausted && from_message_id != 0,
        error: None,
    })
}
