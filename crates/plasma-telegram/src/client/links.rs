//! Opens a Telegram link without joining.
//!
//! Previewable chats are returned as-is. Joining and join requests stay behind
//! an explicit `tdlib_join_chat` / `tdlib_join_invite` call.

use base64::Engine as _;
use tdlib_rs::{
    enums::{ChatInviteLinkInfo, ChatType, ForumTopic, InviteLinkChatType, MessageLinkInfo},
    functions,
};

use super::*;
use crate::links::{parse_telegram_link, MessageLinkFallback, ParsedTelegramLink};

pub async fn tdlib_resolve_link(
    state: &TdlibManager,
    url: String,
) -> plasma_core::Result<TdlibLinkTarget> {
    let Some(parsed) = parse_telegram_link(&url) else {
        return Ok(link_error("Link do Telegram não reconhecido."));
    };
    with_priority(
        &state.queue,
        TdlibPriority::Conversation,
        "resolve_link",
        || async move {
            let client_id = match ready_client_id(state).await {
                Ok(client_id) => client_id,
                Err(status) => {
                    return Ok(link_error(
                        status.error.unwrap_or_else(|| status.state.clone()),
                    ));
                }
            };
            match parsed {
                ParsedTelegramLink::Invite { link } => {
                    resolve_invite(state, client_id, &link).await
                }
                ParsedTelegramLink::Message {
                    canonical,
                    fallback,
                } => resolve_message_link(state, client_id, &canonical, fallback).await,
            }
        },
    )
    .await
}

async fn resolve_invite(
    state: &TdlibManager,
    client_id: i32,
    link: &str,
) -> plasma_core::Result<TdlibLinkTarget> {
    let info = match functions::check_chat_invite_link(link.to_string(), client_id).await {
        Ok(ChatInviteLinkInfo::ChatInviteLinkInfo(info)) => info,
        Err(error) => return Ok(link_error(error.message)),
    };
    let photo = photo_data_url(client_id, info.photo.clone()).await;
    let member_count = (info.member_count > 0).then_some(info.member_count);
    if info.chat_id != 0 {
        if let Ok(mut target) = open_chat(state, client_id, info.chat_id, None, None, None).await {
            if target.success {
                target.invite_link = Some(link.to_string());
                target.join_by_request = target.join_by_request || info.creates_join_request;
                if target.description.is_none() && !info.description.is_empty() {
                    target.description = Some(info.description);
                }
                if target.member_count.is_none() {
                    target.member_count = member_count;
                }
                if target.photo_data_url.is_none() {
                    target.photo_data_url = photo;
                }
                if target.topic_title.is_none() {
                    target.topic_title = Some(info.title);
                }
                return Ok(target);
            }
        }
    }
    Ok(TdlibLinkTarget {
        success: true,
        chat: None,
        topic_id: None,
        topic_kind: None,
        topic_title: Some(info.title),
        message_id: None,
        needs_join: true,
        join_by_request: info.creates_join_request,
        invite_link: Some(link.to_string()),
        description: (!info.description.is_empty()).then_some(info.description),
        member_count,
        photo_data_url: photo,
        is_channel: matches!(info.r#type, InviteLinkChatType::Channel),
        is_group: !matches!(info.r#type, InviteLinkChatType::Channel),
        error: None,
    })
}

pub async fn tdlib_join_chat(
    state: &TdlibManager,
    chat_id: i64,
) -> plasma_core::Result<TdlibJoinResult> {
    with_priority(
        &state.queue,
        TdlibPriority::Conversation,
        "join_chat",
        || async move {
            let client_id = match ready_client_id(state).await {
                Ok(client_id) => client_id,
                Err(status) => {
                    return Ok(join_error(status.error.unwrap_or(status.state)));
                }
            };
            if let Err(error) = functions::join_chat(chat_id, client_id).await {
                return Ok(join_error(membership_error(&error.message)));
            }
            Ok(join_outcome(state, client_id, chat_id).await)
        },
    )
    .await
}

pub async fn tdlib_join_invite(
    state: &TdlibManager,
    invite_link: String,
) -> plasma_core::Result<TdlibJoinResult> {
    with_priority(
        &state.queue,
        TdlibPriority::Conversation,
        "join_invite",
        || async move {
            let client_id = match ready_client_id(state).await {
                Ok(client_id) => client_id,
                Err(status) => {
                    return Ok(join_error(status.error.unwrap_or(status.state)));
                }
            };
            let chat_id = match functions::join_chat_by_invite_link(invite_link, client_id).await {
                Ok(tdlib_rs::enums::Chat::Chat(chat)) => chat.id,
                Err(error) => return Ok(join_error(membership_error(&error.message))),
            };
            Ok(join_outcome(state, client_id, chat_id).await)
        },
    )
    .await
}

async fn resolve_message_link(
    state: &TdlibManager,
    client_id: i32,
    canonical: &str,
    fallback: MessageLinkFallback,
) -> plasma_core::Result<TdlibLinkTarget> {
    if let Ok(MessageLinkInfo::MessageLinkInfo(info)) =
        functions::get_message_link_info(canonical.to_string(), client_id).await
    {
        if info.chat_id != 0 {
            let topic_id = message_topic_id(&info.topic_id).or(fallback.topic_id);
            let topic_kind = message_topic_kind(&info.topic_id).or(fallback.topic_kind.clone());
            let message_id = info
                .message
                .as_ref()
                .map(|message| app_message_id_from_tdlib(message.id))
                .or(fallback.message_id);
            return open_chat(
                state,
                client_id,
                info.chat_id,
                topic_id,
                topic_kind,
                message_id,
            )
            .await;
        }
    }
    if let Some(username) = fallback.username.clone() {
        return match functions::search_public_chat(username, client_id).await {
            Ok(tdlib_rs::enums::Chat::Chat(chat)) => {
                open_chat(
                    state,
                    client_id,
                    chat.id,
                    fallback.topic_id,
                    fallback.topic_kind,
                    fallback.message_id,
                )
                .await
            }
            Err(error) => Ok(link_error(error.message)),
        };
    }
    let Some(chat_id) = fallback.chat_id else {
        return Ok(link_error("Link do Telegram não reconhecido."));
    };
    open_chat(
        state,
        client_id,
        chat_id,
        fallback.topic_id,
        fallback.topic_kind,
        fallback.message_id,
    )
    .await
}

async fn open_chat(
    state: &TdlibManager,
    client_id: i32,
    chat_id: i64,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
    message_id: Option<i64>,
) -> plasma_core::Result<TdlibLinkTarget> {
    let chat = match functions::get_chat(chat_id, client_id).await {
        Ok(tdlib_rs::enums::Chat::Chat(chat)) => chat,
        Err(error) => return Ok(link_error(membership_error(&error.message))),
    };
    let (join_by_request, member_count) = supergroup_access(client_id, &chat).await;
    let photo = photo_data_url(client_id, chat.photo.clone()).await;
    let mut app = tdlib_chat_to_app(chat);
    let membership = tdlib_get_chat_capabilities(state, chat_id).await.ok();
    let is_member = membership
        .as_ref()
        .filter(|result| result.success)
        .map(|result| result.is_member)
        .unwrap_or(true);
    if let Some(result) = membership.filter(|result| result.success) {
        app.is_member = result.is_member;
        app.can_send_messages = result.can_send_messages;
        app.can_send_media = result.can_send_media;
    }
    let topic_title = forum_topic_title(client_id, chat_id, topic_id, topic_kind.as_deref()).await;
    let is_channel = app.is_channel;
    let is_group = app.is_group;
    let title = app.title.clone();
    Ok(TdlibLinkTarget {
        success: true,
        topic_title: topic_title.or(Some(title)),
        chat: Some(app),
        topic_id,
        topic_kind,
        message_id,
        needs_join: !is_member,
        join_by_request: !is_member && join_by_request,
        invite_link: None,
        description: None,
        member_count,
        photo_data_url: photo,
        is_channel,
        is_group,
        error: None,
    })
}

async fn join_outcome(state: &TdlibManager, client_id: i32, chat_id: i64) -> TdlibJoinResult {
    let membership = tdlib_get_chat_capabilities(state, chat_id).await.ok();
    let is_member = membership
        .as_ref()
        .filter(|result| result.success)
        .is_some_and(|result| result.is_member);
    let chat = functions::get_chat(chat_id, client_id).await.ok().map(
        |tdlib_rs::enums::Chat::Chat(chat)| {
            let mut app = tdlib_chat_to_app(chat);
            if let Some(result) = membership.filter(|result| result.success) {
                app.is_member = result.is_member;
                app.can_send_messages = result.can_send_messages;
                app.can_send_media = result.can_send_media;
            }
            app
        },
    );
    TdlibJoinResult {
        success: true,
        requested: !is_member,
        chat,
        error: None,
    }
}

async fn supergroup_access(client_id: i32, chat: &tdlib_rs::types::Chat) -> (bool, Option<i32>) {
    let ChatType::Supergroup(group) = &chat.r#type else {
        return (false, None);
    };
    match functions::get_supergroup(group.supergroup_id, client_id).await {
        Ok(tdlib_rs::enums::Supergroup::Supergroup(info)) => (
            info.join_by_request,
            (info.member_count > 0).then_some(info.member_count),
        ),
        Err(_) => (false, None),
    }
}

async fn photo_data_url(
    client_id: i32,
    photo: Option<tdlib_rs::types::ChatPhotoInfo>,
) -> Option<String> {
    let photo = photo?;
    let mini = photo
        .minithumbnail
        .as_ref()
        .map(|thumb| thumb.data.clone())
        .filter(|data| !data.is_empty());
    if let Some(path) = download_thumbnail_path(client_id, Some(photo.small)).await {
        if let Ok(bytes) = std::fs::read(path) {
            if !bytes.is_empty() {
                return Some(format!(
                    "data:image/jpeg;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ));
            }
        }
    }
    mini.map(|data| format!("data:image/jpeg;base64,{data}"))
}

async fn forum_topic_title(
    client_id: i32,
    chat_id: i64,
    topic_id: Option<i64>,
    topic_kind: Option<&str>,
) -> Option<String> {
    if topic_kind != Some("forum") {
        return None;
    }
    let topic_id = i32::try_from(topic_id?).ok()?;
    match functions::get_forum_topic(chat_id, topic_id, client_id).await {
        Ok(ForumTopic::ForumTopic(topic)) => Some(topic.info.name),
        Err(_) => None,
    }
}

fn membership_error(message: &str) -> String {
    let upper = message.to_ascii_uppercase();
    if upper.contains("INVITE")
        || upper.contains("CHANNEL_PRIVATE")
        || upper.contains("USER_NOT_PARTICIPANT")
        || upper.contains("CHAT_NOT_FOUND")
    {
        "Este canal é privado. Peça um convite para entrar.".to_string()
    } else {
        message.to_string()
    }
}

fn link_error(error: impl Into<String>) -> TdlibLinkTarget {
    TdlibLinkTarget {
        success: false,
        chat: None,
        topic_id: None,
        topic_kind: None,
        topic_title: None,
        message_id: None,
        needs_join: false,
        join_by_request: false,
        invite_link: None,
        description: None,
        member_count: None,
        photo_data_url: None,
        is_channel: false,
        is_group: false,
        error: Some(error.into()),
    }
}

fn join_error(error: impl Into<String>) -> TdlibJoinResult {
    TdlibJoinResult {
        success: false,
        requested: false,
        chat: None,
        error: Some(error.into()),
    }
}
