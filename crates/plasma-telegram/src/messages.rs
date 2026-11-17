use base64::Engine as _;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{LazyLock, Mutex as StdMutex};

use plasma_core::AppEnv;
use tdlib_rs::{
    enums::{
        ChatMemberStatus, ChatType, InputFile, InputMessageContent, InputMessageReplyTo,
        MessageContent, MessageReplyTo, MessageSender, MessageTopic, ReactionType,
    },
    functions,
    types::{
        ChatPermissions, File, FormattedText, InputFileLocal, InputMessageDocument,
        InputMessagePhoto, InputMessageReplyToMessage, InputMessageVideo, MessageTopicForum,
        MessageTopicThread, Photo,
    },
};
use tokio::task::JoinSet;

use crate::media::{
    document_is_video, file_name_or_default, mime_extension, sanitize_filename,
    sanitize_folder_name,
};
use crate::types::*;

pub(crate) const SENDER_NAME_RESOLUTION_CONCURRENCY: usize = 8;

pub(crate) static THUMBNAIL_FAILURES: LazyLock<StdMutex<HashSet<(i64, i64)>>> =
    LazyLock::new(|| StdMutex::new(HashSet::new()));

pub(crate) fn log_thumbnail_failure(chat_id: i64, message_id: i64) {
    let should_log = THUMBNAIL_FAILURES
        .lock()
        .map(|mut failures| {
            if failures.len() >= 1024 {
                failures.clear();
            }
            failures.insert((chat_id, message_id))
        })
        .unwrap_or(true);
    if should_log {
        tracing::debug!(chat_id, message_id, "Telegram thumbnail unavailable");
    }
}

pub(crate) fn best_photo_file(photo: tdlib_rs::types::Photo) -> Option<File> {
    photo
        .sizes
        .into_iter()
        .max_by_key(|size| {
            let area = i64::from(size.width) * i64::from(size.height);
            (area, size.photo.size)
        })
        .map(|size| size.photo)
}

pub(crate) fn thumbnail_photo_file(photo: Photo) -> Option<File> {
    const TARGET_AREA: i64 = 320 * 320;
    photo
        .sizes
        .into_iter()
        .filter(|size| size.width > 0 && size.height > 0)
        .min_by_key(|size| {
            let area = i64::from(size.width) * i64::from(size.height);
            (area - TARGET_AREA).abs()
        })
        .map(|size| size.photo)
}

pub(crate) struct MessageMediaFile {
    pub(crate) file: File,
    pub(crate) file_name: String,
    pub(crate) mime_type: String,
    pub(crate) thumbnail: Option<File>,
}

pub(crate) fn effective_file_size(file: &File) -> i64 {
    if file.size > 0 {
        file.size
    } else {
        file.expected_size
    }
}

pub(crate) fn message_media_file(
    content: MessageContent,
    message_id: i64,
) -> Option<MessageMediaFile> {
    match content {
        MessageContent::MessageAnimation(content) => {
            let extension = mime_extension(&content.animation.mime_type, ".mp4");
            let file_name =
                file_name_or_default(content.animation.file_name, message_id, &extension);
            Some(MessageMediaFile {
                file: content.animation.animation,
                file_name,
                mime_type: content.animation.mime_type,
                thumbnail: content.animation.thumbnail.map(|thumbnail| thumbnail.file),
            })
        }
        MessageContent::MessageAudio(content) => {
            let extension = mime_extension(&content.audio.mime_type, ".mp3");
            let file_name = file_name_or_default(content.audio.file_name, message_id, &extension);
            Some(MessageMediaFile {
                file: content.audio.audio,
                file_name,
                mime_type: content.audio.mime_type,
                thumbnail: content
                    .audio
                    .album_cover_thumbnail
                    .map(|thumbnail| thumbnail.file),
            })
        }
        MessageContent::MessageDocument(content) => {
            let extension = mime_extension(&content.document.mime_type, ".bin");
            let file_name =
                file_name_or_default(content.document.file_name, message_id, &extension);
            Some(MessageMediaFile {
                file: content.document.document,
                file_name,
                mime_type: content.document.mime_type,
                thumbnail: content.document.thumbnail.map(|thumbnail| thumbnail.file),
            })
        }
        MessageContent::MessagePhoto(content) => {
            best_photo_file(content.photo).map(|file| MessageMediaFile {
                thumbnail: Some(file.clone()),
                file,
                file_name: sanitize_filename(&format!("media_{message_id}.jpg")),
                mime_type: "image/jpeg".to_string(),
            })
        }
        MessageContent::MessageVideo(content) => {
            let extension = mime_extension(&content.video.mime_type, ".mp4");
            let file_name = file_name_or_default(content.video.file_name, message_id, &extension);
            Some(MessageMediaFile {
                file: content.video.video,
                file_name,
                mime_type: content.video.mime_type,
                thumbnail: content.video.thumbnail.map(|thumbnail| thumbnail.file),
            })
        }
        MessageContent::MessageVideoNote(content) => Some(MessageMediaFile {
            file: content.video_note.video,
            file_name: sanitize_filename(&format!("media_{message_id}.mp4")),
            mime_type: "video/mp4".to_string(),
            thumbnail: content.video_note.thumbnail.map(|thumbnail| thumbnail.file),
        }),
        MessageContent::MessageVoiceNote(content) => {
            let extension = mime_extension(&content.voice_note.mime_type, ".ogg");
            Some(MessageMediaFile {
                file: content.voice_note.voice,
                file_name: sanitize_filename(&format!("media_{message_id}{extension}")),
                mime_type: content.voice_note.mime_type,
                thumbnail: None,
            })
        }
        _ => None,
    }
}

pub(crate) fn message_thumbnail_file(content: MessageContent) -> Option<File> {
    match content {
        MessageContent::MessageAnimation(content) => {
            content.animation.thumbnail.map(|thumbnail| thumbnail.file)
        }
        MessageContent::MessageAudio(content) => content
            .audio
            .album_cover_thumbnail
            .map(|thumbnail| thumbnail.file),
        MessageContent::MessageDocument(content) => {
            content.document.thumbnail.map(|thumbnail| thumbnail.file)
        }
        MessageContent::MessagePhoto(content) => thumbnail_photo_file(content.photo),
        MessageContent::MessageVideo(content) => {
            content.video.thumbnail.map(|thumbnail| thumbnail.file)
        }
        MessageContent::MessageVideoNote(content) => {
            content.video_note.thumbnail.map(|thumbnail| thumbnail.file)
        }
        _ => None,
    }
}

/// Small embedded JPEG thumbnail provided by TDLib when no regular thumbnail
/// is available (common for documents such as `.mov` files).
pub(crate) fn message_minithumbnail(content: &MessageContent) -> Option<&str> {
    let minithumbnail = match content {
        MessageContent::MessageAnimation(content) => content.animation.minithumbnail.as_ref(),
        MessageContent::MessageAudio(content) => content.audio.album_cover_minithumbnail.as_ref(),
        MessageContent::MessageDocument(content) => content.document.minithumbnail.as_ref(),
        MessageContent::MessagePhoto(content) => content.photo.minithumbnail.as_ref(),
        MessageContent::MessageVideo(content) => content.video.minithumbnail.as_ref(),
        MessageContent::MessageVideoNote(content) => content.video_note.minithumbnail.as_ref(),
        _ => None,
    }?;
    (!minithumbnail.data.is_empty()).then_some(minithumbnail.data.as_str())
}

pub(crate) fn write_minithumbnail(data: &str, media_dir: &Path) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    if bytes.is_empty() {
        return None;
    }
    let path = media_dir.join("thumbnail.jpg");
    std::fs::write(&path, bytes).ok()?;
    Some(path.to_string_lossy().to_string())
}

pub(crate) fn local_thumbnail_path(thumbnail: Option<File>) -> Option<String> {
    let thumbnail = thumbnail?;
    if thumbnail.local.is_downloading_completed && !thumbnail.local.path.is_empty() {
        Some(thumbnail.local.path)
    } else {
        None
    }
}

pub(crate) fn tdlib_message_id_from_app_id(message_id: i64) -> i64 {
    if message_id > 0 && message_id < (1_i64 << 20) {
        message_id << 20
    } else {
        message_id
    }
}

pub(crate) fn app_message_id_from_tdlib(message_id: i64) -> i64 {
    if message_id > (1_i64 << 20) {
        message_id >> 20
    } else {
        message_id
    }
}

pub(crate) fn sender_id_to_string(sender: &MessageSender) -> String {
    match sender {
        MessageSender::User(sender) => sender.user_id.to_string(),
        MessageSender::Chat(sender) => sender.chat_id.to_string(),
    }
}

pub(crate) fn is_hidden_membership_service_message(content: &MessageContent) -> bool {
    matches!(
        content,
        MessageContent::MessageChatAddMembers(_)
            | MessageContent::MessageChatJoinByLink
            | MessageContent::MessageChatJoinByRequest
            | MessageContent::MessageChatDeleteMember(_)
    )
}

fn sender_name_cache() -> &'static std::sync::Mutex<HashMap<String, String>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, String>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

pub(crate) async fn resolve_sender_name(client_id: i32, sender_id: &str) -> String {
    if let Ok(cache) = sender_name_cache().lock() {
        if let Some(name) = cache.get(sender_id) {
            return name.clone();
        }
    }
    let name = lookup_sender_name(client_id, sender_id).await;
    if !name.trim().is_empty() {
        if let Ok(mut cache) = sender_name_cache().lock() {
            if cache.len() >= 4_000 {
                let victims: Vec<String> = cache.keys().take(cache.len() / 2).cloned().collect();
                for key in victims {
                    cache.remove(&key);
                }
            }
            cache.insert(sender_id.to_string(), name.clone());
        }
    }
    name
}

async fn lookup_sender_name(client_id: i32, sender_id: &str) -> String {
    match sender_id.parse::<i64>() {
        Ok(sender_id) if sender_id > 0 => match functions::get_user(sender_id, client_id).await {
            Ok(tdlib_rs::enums::User::User(user)) => {
                let full_name = format!("{} {}", user.first_name, user.last_name)
                    .trim()
                    .to_string();
                if full_name.is_empty() {
                    user.usernames
                        .and_then(|usernames| usernames.active_usernames.into_iter().next())
                        .unwrap_or_default()
                } else {
                    full_name
                }
            }
            Err(_) => String::new(),
        },
        Ok(sender_id) => match functions::get_chat(sender_id, client_id).await {
            Ok(tdlib_rs::enums::Chat::Chat(chat)) => chat.title,
            Err(_) => String::new(),
        },
        Err(_) => String::new(),
    }
}

pub(crate) async fn resolve_sender_folder_name(
    client_id: i32,
    sender: &MessageSender,
    sender_cache: &mut HashMap<String, String>,
) -> String {
    let fallback = match sender {
        MessageSender::User(sender) => format!("ID_{}", sender.user_id),
        MessageSender::Chat(sender) => format!("Chat_{}", sender.chat_id),
    };
    let sender_id = sender_id_to_string(sender);
    if let Some(cached_name) = sender_cache.get(&sender_id) {
        return cached_name.clone();
    }

    let resolved_name = resolve_sender_name(client_id, &sender_id).await;
    let folder_name = sanitize_folder_name(if resolved_name.trim().is_empty() {
        &fallback
    } else {
        resolved_name.trim()
    });
    sender_cache.insert(sender_id, folder_name.clone());
    folder_name
}

pub(crate) fn message_topic_id(topic: &Option<MessageTopic>) -> Option<i64> {
    match topic {
        Some(MessageTopic::Forum(topic)) => Some(i64::from(topic.forum_topic_id)),
        Some(MessageTopic::Thread(topic)) => {
            Some(app_message_id_from_tdlib(topic.message_thread_id))
        }
        _ => None,
    }
}

pub(crate) fn message_topic_kind(topic: &Option<MessageTopic>) -> Option<String> {
    match topic {
        Some(MessageTopic::Forum(_)) => Some("forum".to_string()),
        Some(MessageTopic::Thread(_)) => Some("thread".to_string()),
        Some(MessageTopic::DirectMessages(_)) => Some("direct_messages".to_string()),
        Some(MessageTopic::SavedMessages(_)) => Some("saved_messages".to_string()),
        None => None,
    }
}

pub(crate) fn reply_message_id(reply_to: &Option<MessageReplyTo>) -> Option<i64> {
    match reply_to {
        Some(MessageReplyTo::Message(reply)) => Some(app_message_id_from_tdlib(reply.message_id)),
        _ => None,
    }
}

pub(crate) fn content_text(content: &MessageContent) -> String {
    match content {
        MessageContent::MessageText(content) => content.text.text.clone(),
        MessageContent::MessageAnimation(content) => content.caption.text.clone(),
        MessageContent::MessageAudio(content) => content.caption.text.clone(),
        MessageContent::MessageDocument(content) => content.caption.text.clone(),
        MessageContent::MessagePhoto(content) => content.caption.text.clone(),
        MessageContent::MessageVideo(content) => content.caption.text.clone(),
        MessageContent::MessageVoiceNote(content) => content.caption.text.clone(),
        _ => String::new(),
    }
}

pub(crate) fn is_likely_album_separator_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.len() > 48 {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("www.")
        || lower.contains("t.me/")
    {
        return false;
    }

    let compact: String = trimmed.chars().filter(|ch| !ch.is_whitespace()).collect();
    if compact.is_empty() {
        return false;
    }

    let letters_or_numbers = compact.chars().filter(|ch| ch.is_alphanumeric()).count();
    let symbols = compact.chars().count().saturating_sub(letters_or_numbers);
    if letters_or_numbers == 0 {
        return true;
    }
    if compact.chars().count() <= 12 && symbols >= letters_or_numbers {
        return true;
    }

    compact.chars().count() <= 16
        && trimmed.chars().all(|ch| {
            ch.is_alphanumeric() || ch.is_whitespace() || matches!(ch, '.' | '_' | '#' | '-')
        })
}

/// Local file path of a playable video, including videos sent as documents.
pub(crate) fn local_video_path(content: &MessageContent) -> Option<String> {
    let path = match content {
        MessageContent::MessageAnimation(content) => &content.animation.animation.local.path,
        MessageContent::MessageVideo(content) => &content.video.video.local.path,
        MessageContent::MessageVideoNote(content) => &content.video_note.video.local.path,
        MessageContent::MessageDocument(content)
            if document_is_video(&content.document.mime_type, &content.document.file_name) =>
        {
            &content.document.document.local.path
        }
        _ => return None,
    };
    (!path.is_empty()).then(|| path.clone())
}

pub(crate) fn content_media_flags(
    content: &MessageContent,
) -> (bool, bool, bool, Option<i32>, Option<i64>) {
    match content {
        MessageContent::MessageAnimation(content) => (
            true,
            false,
            true,
            Some(content.animation.duration),
            Some(effective_file_size(&content.animation.animation)),
        ),
        MessageContent::MessageAudio(content) => (
            true,
            false,
            false,
            None,
            Some(effective_file_size(&content.audio.audio)),
        ),
        MessageContent::MessageDocument(content) => (
            true,
            false,
            document_is_video(&content.document.mime_type, &content.document.file_name),
            None,
            Some(effective_file_size(&content.document.document)),
        ),
        MessageContent::MessagePhoto(content) => {
            let size =
                best_photo_file(content.photo.clone()).map(|file| effective_file_size(&file));
            (true, true, false, None, size)
        }
        MessageContent::MessageVideo(content) => (
            true,
            false,
            true,
            Some(content.video.duration),
            Some(effective_file_size(&content.video.video)),
        ),
        MessageContent::MessageVideoNote(content) => (
            true,
            false,
            true,
            Some(content.video_note.duration),
            Some(effective_file_size(&content.video_note.video)),
        ),
        MessageContent::MessageVoiceNote(content) => (
            true,
            false,
            false,
            None,
            Some(effective_file_size(&content.voice_note.voice)),
        ),
        _ => (false, false, false, None, None),
    }
}

pub(crate) fn tdlib_message_to_app(message: tdlib_rs::types::Message) -> TdlibMessageInfo {
    let text = content_text(&message.content);
    let (has_media, is_photo, is_video, video_duration, media_size) =
        content_media_flags(&message.content);
    let thumbnail_path = local_thumbnail_path(message_thumbnail_file(message.content.clone()));

    TdlibMessageInfo {
        id: app_message_id_from_tdlib(message.id),
        message: text.clone(),
        date: message.date,
        out: message.is_outgoing,
        sender_id: sender_id_to_string(&message.sender_id),
        sender_name: String::new(),
        reply_to_msg_id: reply_message_id(&message.reply_to),
        topic_id: message_topic_id(&message.topic_id),
        topic_kind: message_topic_kind(&message.topic_id),
        media: has_media,
        text,
        has_media,
        is_photo,
        is_video,
        grouped_id: if message.media_album_id != 0 {
            Some(message.media_album_id.to_string())
        } else {
            None
        },
        video_duration,
        media_size,
        thumbnail_path,
        is_deleted: false,
        reactions: message_reactions(&message),
    }
}

/// Maps the TDLib reaction list to the flat shape used by the UI.
///
/// Custom-emoji and paid reactions are skipped: the shell only renders
/// standard emoji chips today.
pub(crate) fn message_reactions(message: &tdlib_rs::types::Message) -> Vec<TdlibReactionInfo> {
    let Some(interactions) = message.interaction_info.as_ref() else {
        return Vec::new();
    };
    let Some(reactions) = interactions.reactions.as_ref() else {
        return Vec::new();
    };
    reactions
        .reactions
        .iter()
        .filter_map(|reaction| {
            let ReactionType::Emoji(emoji) = &reaction.r#type else {
                return None;
            };
            if reaction.total_count <= 0 {
                return None;
            }
            Some(TdlibReactionInfo {
                emoji: emoji.emoji.clone(),
                count: reaction.total_count,
                mine: reaction.is_chosen,
            })
        })
        .collect()
}

pub(crate) async fn resolve_message_sender_names(
    client_id: i32,
    messages: &mut [TdlibMessageInfo],
) {
    let mut sender_names = HashMap::<String, String>::new();
    let sender_ids = messages
        .iter()
        .filter(|message| !message.out)
        .map(|message| message.sender_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    for sender_batch in sender_ids.chunks(SENDER_NAME_RESOLUTION_CONCURRENCY) {
        let mut tasks = JoinSet::new();
        for sender_id in sender_batch {
            let sender_id = sender_id.clone();
            tasks.spawn(async move {
                let sender_name = resolve_sender_name(client_id, &sender_id).await;
                (sender_id, sender_name)
            });
        }

        while let Some(result) = tasks.join_next().await {
            if let Ok((sender_id, sender_name)) = result {
                sender_names.insert(sender_id, sender_name);
            }
        }
    }

    for message in messages {
        if !message.out {
            message.sender_name = sender_names
                .get(&message.sender_id)
                .cloned()
                .unwrap_or_default();
        }
    }
}

/// Fills the chat title of cross-chat search hits.
///
/// Search results only carry the chat id. Resolving the titles here keeps the
/// UI from showing "Conversa" for every chat that is not in the loaded list.
pub(crate) async fn resolve_chat_titles(client_id: i32, entries: &mut [TdlibGlobalSearchMessage]) {
    let chat_ids = entries
        .iter()
        .filter(|entry| entry.chat_title.trim().is_empty())
        .map(|entry| entry.chat_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if chat_ids.is_empty() {
        return;
    }

    let mut titles = HashMap::<String, String>::new();
    for batch in chat_ids.chunks(SENDER_NAME_RESOLUTION_CONCURRENCY) {
        let mut tasks = JoinSet::new();
        for chat_id in batch {
            let chat_id = chat_id.clone();
            tasks.spawn(async move {
                let title = match chat_id.parse::<i64>() {
                    Ok(id) => match functions::get_chat(id, client_id).await {
                        Ok(tdlib_rs::enums::Chat::Chat(chat)) => chat.title,
                        _ => String::new(),
                    },
                    Err(_) => String::new(),
                };
                (chat_id, title)
            });
        }

        while let Some(result) = tasks.join_next().await {
            if let Ok((chat_id, title)) = result {
                if !title.trim().is_empty() {
                    titles.insert(chat_id, title);
                }
            }
        }
    }

    for entry in entries {
        if entry.chat_title.trim().is_empty() {
            if let Some(title) = titles.get(&entry.chat_id) {
                entry.chat_title = title.clone();
            }
        }
    }
}

pub(crate) fn plain_formatted_text(text: impl Into<String>) -> FormattedText {
    FormattedText {
        text: text.into(),
        entities: Vec::new(),
    }
}

pub(crate) fn optional_caption(text: &str) -> Option<FormattedText> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(plain_formatted_text(trimmed.to_string()))
    }
}

pub(crate) fn message_topic_from_id(
    topic_id: Option<i64>,
    topic_kind: Option<&str>,
) -> Option<MessageTopic> {
    topic_id.and_then(|topic_id| match topic_kind.unwrap_or("forum") {
        "thread" => Some(MessageTopic::Thread(MessageTopicThread {
            message_thread_id: tdlib_message_id_from_app_id(topic_id),
        })),
        "forum" => i32::try_from(topic_id)
            .ok()
            .map(|forum_topic_id| MessageTopic::Forum(MessageTopicForum { forum_topic_id })),
        _ => None,
    })
}

pub(crate) fn reply_to_from_id(reply_to_id: Option<i64>) -> Option<InputMessageReplyTo> {
    reply_to_id.map(|message_id| {
        InputMessageReplyTo::Message(InputMessageReplyToMessage {
            message_id: tdlib_message_id_from_app_id(message_id),
            quote: None,
            checklist_task_id: 0,
        })
    })
}

pub(crate) fn input_file_local(path: &str) -> InputFile {
    InputFile::Local(InputFileLocal {
        path: path.to_string(),
    })
}

pub(crate) fn input_content_for_local_media(file_path: &str, caption: &str) -> InputMessageContent {
    let lower = file_path.to_lowercase();
    let file = input_file_local(file_path);
    if lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".png")
        || lower.ends_with(".webp")
    {
        InputMessageContent::InputMessagePhoto(InputMessagePhoto {
            photo: file,
            thumbnail: None,
            added_sticker_file_ids: Vec::new(),
            width: 0,
            height: 0,
            caption: optional_caption(caption),
            show_caption_above_media: false,
            self_destruct_type: None,
            has_spoiler: false,
        })
    } else if lower.ends_with(".mp4")
        || lower.ends_with(".mov")
        || lower.ends_with(".m4v")
        || lower.ends_with(".webm")
        || lower.ends_with(".mkv")
    {
        InputMessageContent::InputMessageVideo(InputMessageVideo {
            video: file,
            thumbnail: None,
            cover: None,
            start_timestamp: 0,
            added_sticker_file_ids: Vec::new(),
            duration: 0,
            width: 0,
            height: 0,
            supports_streaming: true,
            caption: optional_caption(caption),
            show_caption_above_media: false,
            self_destruct_type: None,
            has_spoiler: false,
        })
    } else {
        InputMessageContent::InputMessageDocument(InputMessageDocument {
            document: file,
            thumbnail: None,
            disable_content_type_detection: false,
            caption: optional_caption(caption),
        })
    }
}

pub(crate) fn permissions_can_send_media(permissions: &ChatPermissions) -> bool {
    permissions.can_send_audios
        || permissions.can_send_documents
        || permissions.can_send_photos
        || permissions.can_send_videos
        || permissions.can_send_video_notes
        || permissions.can_send_voice_notes
        || permissions.can_send_other_messages
}

pub(crate) fn member_status_capabilities(
    status: &ChatMemberStatus,
    is_channel: bool,
    permissions: &ChatPermissions,
) -> (bool, bool, bool) {
    match status {
        ChatMemberStatus::Creator(status) => (status.is_member, true, true),
        ChatMemberStatus::Administrator(status) => {
            if is_channel {
                (
                    true,
                    status.rights.can_post_messages,
                    status.rights.can_post_messages,
                )
            } else {
                (true, true, true)
            }
        }
        ChatMemberStatus::Member(_) => (
            true,
            !is_channel && permissions.can_send_basic_messages,
            !is_channel && permissions_can_send_media(permissions),
        ),
        ChatMemberStatus::Restricted(status) => (
            status.is_member,
            status.permissions.can_send_basic_messages,
            permissions_can_send_media(&status.permissions),
        ),
        ChatMemberStatus::Left | ChatMemberStatus::Banned(_) => (false, false, false),
    }
}

pub(crate) fn chat_send_capabilities(
    chat_type: &ChatType,
    permissions: &ChatPermissions,
) -> (bool, bool, bool) {
    match chat_type {
        ChatType::Private(_) | ChatType::Secret(_) => (true, true, true),
        ChatType::BasicGroup(_) => (
            true,
            permissions.can_send_basic_messages,
            permissions_can_send_media(permissions),
        ),
        ChatType::Supergroup(group) => (
            true,
            !group.is_channel && permissions.can_send_basic_messages,
            !group.is_channel && permissions_can_send_media(permissions),
        ),
    }
}

pub(crate) fn tdlib_chat_to_app(chat: tdlib_rs::types::Chat) -> TdlibChatInfo {
    let (is_group, is_channel) = match &chat.r#type {
        ChatType::Private(_) | ChatType::Secret(_) => (false, false),
        ChatType::BasicGroup(_) => (true, false),
        ChatType::Supergroup(chat_type) => (!chat_type.is_channel, chat_type.is_channel),
    };
    let (is_member, can_send_messages, can_send_media) =
        chat_send_capabilities(&chat.r#type, &chat.permissions);

    let (
        last_message_id,
        last_message_text,
        last_message_date,
        last_message_has_media,
        last_message_is_video,
        last_message_is_photo,
    ) = match chat.last_message {
        Some(message) => {
            let text = content_text(&message.content);
            let (has_media, is_photo, is_video, _, _) = content_media_flags(&message.content);
            (
                Some(message.id),
                text,
                message.date,
                has_media,
                is_video,
                is_photo,
            )
        }
        None => (None, String::new(), 0, false, false, false),
    };

    TdlibChatInfo {
        id: chat.id.to_string(),
        title: chat.title,
        date: last_message_date,
        unread_count: chat.unread_count,
        is_group,
        is_channel,
        is_member,
        can_send_messages,
        can_send_media,
        has_topics: chat.view_as_topics,
        last_message_id,
        last_message_text,
        last_message_date,
        last_message_has_media,
        last_message_is_video,
        last_message_is_photo,
    }
}

pub(crate) fn tdlib_user_to_contact_chat(user: tdlib_rs::types::User) -> TdlibChatInfo {
    let title = format!("{} {}", user.first_name, user.last_name)
        .trim()
        .to_string();
    let title = if title.is_empty() {
        user.usernames
            .and_then(|usernames| usernames.active_usernames.into_iter().next())
            .unwrap_or_else(|| user.phone_number.clone())
    } else {
        title
    };

    TdlibChatInfo {
        id: user.id.to_string(),
        title,
        date: 0,
        unread_count: 0,
        is_group: false,
        is_channel: false,
        is_member: true,
        can_send_messages: true,
        can_send_media: true,
        has_topics: false,
        last_message_id: None,
        last_message_text: String::new(),
        last_message_date: 0,
        last_message_has_media: false,
        last_message_is_video: false,
        last_message_is_photo: false,
    }
}

pub(crate) fn emit_mass_progress(
    app: &AppEnv,
    chat_id: i64,
    total: usize,
    downloaded: f64,
    current_file: impl Into<String>,
    is_scanning: bool,
    items: &[TdlibDownloadItem],
) {
    app.emit(
        "tdlib-download-progress",
        TdlibDownloadProgress {
            chat_id,
            total,
            downloaded,
            current_file: current_file.into(),
            is_scanning,
            items: items.to_vec(),
        },
    );
}

pub(crate) fn emit_new_message(app: &AppEnv, chat_id: i64, message: TdlibMessageInfo) {
    app.emit(
        "tdlib-new-message",
        TdlibNewMessageEvent {
            chat_id,
            topic_id: message.topic_id,
            topic_kind: message.topic_kind.clone(),
            message,
        },
    );
}

pub(crate) fn emit_deleted_messages(app: &AppEnv, chat_id: i64, message_ids: Vec<i64>) {
    app.emit(
        "tdlib-messages-deleted",
        TdlibDeletedMessagesEvent {
            chat_id,
            message_ids,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forum_topic_round_trips_without_becoming_a_thread() {
        let topic = message_topic_from_id(Some(42), Some("forum"));

        assert_eq!(message_topic_kind(&topic).as_deref(), Some("forum"));
        assert_eq!(message_topic_id(&topic), Some(42));
    }

    #[test]
    fn thread_topic_round_trips_using_app_message_ids() {
        let app_message_id = 941;
        let topic = message_topic_from_id(Some(app_message_id), Some("thread"));

        assert_eq!(message_topic_kind(&topic).as_deref(), Some("thread"));
        assert_eq!(message_topic_id(&topic), Some(app_message_id));
    }

    #[test]
    fn unsupported_topic_kinds_do_not_create_fake_topics() {
        assert!(message_topic_from_id(Some(7), Some("direct_messages")).is_none());
        assert!(message_topic_from_id(Some(7), Some("saved_messages")).is_none());
        assert!(message_topic_from_id(None, Some("forum")).is_none());
    }

    #[test]
    fn oversized_forum_topic_ids_are_rejected() {
        assert!(message_topic_from_id(Some(i64::from(i32::MAX) + 1), Some("forum")).is_none());
    }

    #[test]
    fn minithumbnail_is_written_as_jpeg() {
        let root = std::env::temp_dir().join(format!("plasma-minithumb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let bytes = [0xff, 0xd8, 0xff, 0xd9];
        let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);

        let path = write_minithumbnail(&encoded, &root).unwrap();
        assert!(path.ends_with("thumbnail.jpg"));
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(&root);
    }
}
