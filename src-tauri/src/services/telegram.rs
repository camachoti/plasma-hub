use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tdlib_rs::{
    enums::{
        AuthorizationState, ChatList, ChatType, MessageContent, MessageReplyTo, MessageSender,
        MessageTopic, Update,
    },
    functions,
    types::File,
};
use tokio::sync::Mutex;

#[derive(Default)]
struct TdlibInner {
    client_id: Option<i32>,
    api_id: Option<i32>,
    api_hash: Option<String>,
    database_directory: Option<String>,
    files_directory: Option<String>,
    auth_state: String,
}

#[derive(Default)]
pub struct TdlibManager {
    inner: Arc<Mutex<TdlibInner>>,
    receiver_started: AtomicBool,
    download_aborted: AtomicBool,
}

#[derive(Serialize)]
pub struct TdlibStatus {
    success: bool,
    ready: bool,
    state: String,
    error: Option<String>,
}

type TdlibCommandResult = Result<TdlibStatus, String>;

#[derive(Serialize)]
pub struct TdlibUserInfo {
    success: bool,
    id: Option<i64>,
    first_name: Option<String>,
    last_name: Option<String>,
    phone_number: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
pub struct TdlibDownloadResult {
    success: bool,
    skipped: bool,
    file_path: Option<String>,
    file_name: Option<String>,
    size: i64,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibChatInfo {
    id: String,
    title: String,
    date: i32,
    unread_count: i32,
    is_group: bool,
    is_channel: bool,
    has_topics: bool,
    last_message_text: String,
    last_message_date: i32,
    last_message_has_media: bool,
    last_message_is_video: bool,
    last_message_is_photo: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibChatsResult {
    success: bool,
    dialogs: Vec<TdlibChatInfo>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibMessageInfo {
    id: i64,
    message: String,
    date: i32,
    out: bool,
    sender_id: String,
    sender_name: String,
    reply_to_msg_id: Option<i64>,
    topic_id: Option<i32>,
    media: bool,
    text: String,
    has_media: bool,
    is_photo: bool,
    is_video: bool,
    grouped_id: Option<String>,
    video_duration: Option<i32>,
    media_size: Option<i64>,
    is_deleted: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibMessagesResult {
    success: bool,
    messages: Vec<TdlibMessageInfo>,
    has_more: bool,
    oldest_message_id: Option<i64>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibForumTopicInfo {
    id: i32,
    title: String,
    is_closed: bool,
    is_pinned: bool,
    unread_count: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibForumTopicsResult {
    success: bool,
    topics: Vec<TdlibForumTopicInfo>,
    error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibMassDownloadRequest {
    chat_id: i64,
    folder_path: String,
    topic_id: Option<i32>,
    split_by_user: bool,
}

#[derive(Serialize)]
pub struct TdlibMassDownloadResult {
    success: bool,
    downloaded_count: usize,
    skipped_count: usize,
    failed_count: usize,
    total: usize,
    aborted: bool,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TdlibDownloadItem {
    name: String,
    status: String,
    progress: u8,
    size: i64,
    file_path: Option<String>,
    thumbnail_path: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TdlibDownloadProgress {
    chat_id: i64,
    total: usize,
    downloaded: f64,
    current_file: String,
    is_scanning: bool,
    items: Vec<TdlibDownloadItem>,
}

fn state_label(state: &AuthorizationState) -> &'static str {
    match state {
        AuthorizationState::WaitTdlibParameters => "wait_tdlib_parameters",
        AuthorizationState::WaitPhoneNumber => "wait_phone_number",
        AuthorizationState::WaitCode(_) => "wait_code",
        AuthorizationState::WaitPassword(_) => "wait_password",
        AuthorizationState::WaitEmailAddress(_) => "wait_email_address",
        AuthorizationState::WaitEmailCode(_) => "wait_email_code",
        AuthorizationState::WaitOtherDeviceConfirmation(_) => "wait_other_device_confirmation",
        AuthorizationState::WaitRegistration(_) => "wait_registration",
        AuthorizationState::Ready => "ready",
        AuthorizationState::LoggingOut => "logging_out",
        AuthorizationState::Closing => "closing",
        AuthorizationState::Closed => "closed",
        _ => "unknown",
    }
}

fn mime_extension(mime_type: &str, fallback: &str) -> String {
    match mime_type {
        "image/jpeg" => ".jpg".to_string(),
        "image/png" => ".png".to_string(),
        "image/webp" => ".webp".to_string(),
        "image/gif" => ".gif".to_string(),
        "video/mp4" => ".mp4".to_string(),
        "video/webm" => ".webm".to_string(),
        "video/quicktime" => ".mov".to_string(),
        "audio/mpeg" => ".mp3".to_string(),
        "audio/mp4" => ".m4a".to_string(),
        "audio/ogg" => ".ogg".to_string(),
        _ => fallback.to_string(),
    }
}

fn sanitize_filename(value: &str) -> String {
    let name: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect();

    let trimmed = name.trim_matches('_');
    if trimmed.is_empty() {
        "arquivo".to_string()
    } else {
        trimmed.chars().take(160).collect()
    }
}

fn sanitize_folder_name(value: &str) -> String {
    let name: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();

    let trimmed = name.trim_matches('_');
    if trimmed.is_empty() {
        "Desconhecido".to_string()
    } else {
        trimmed.chars().take(160).collect()
    }
}

fn file_name_or_default(file_name: String, message_id: i64, fallback_extension: &str) -> String {
    if file_name.trim().is_empty() {
        sanitize_filename(&format!("media_{message_id}{fallback_extension}"))
    } else {
        sanitize_filename(&file_name)
    }
}

fn best_photo_file(photo: tdlib_rs::types::Photo) -> Option<File> {
    photo
        .sizes
        .into_iter()
        .max_by_key(|size| {
            let area = i64::from(size.width) * i64::from(size.height);
            (area, size.photo.size)
        })
        .map(|size| size.photo)
}

struct MessageMediaFile {
    file: File,
    file_name: String,
    thumbnail: Option<File>,
}

fn effective_file_size(file: &File) -> i64 {
    if file.size > 0 {
        file.size
    } else {
        file.expected_size
    }
}

fn message_media_file(content: MessageContent, message_id: i64) -> Option<MessageMediaFile> {
    match content {
        MessageContent::MessageAnimation(content) => {
            let extension = mime_extension(&content.animation.mime_type, ".mp4");
            let file_name =
                file_name_or_default(content.animation.file_name, message_id, &extension);
            Some(MessageMediaFile {
                file: content.animation.animation,
                file_name,
                thumbnail: content.animation.thumbnail.map(|thumbnail| thumbnail.file),
            })
        }
        MessageContent::MessageAudio(content) => {
            let extension = mime_extension(&content.audio.mime_type, ".mp3");
            let file_name = file_name_or_default(content.audio.file_name, message_id, &extension);
            Some(MessageMediaFile {
                file: content.audio.audio,
                file_name,
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
                thumbnail: content.document.thumbnail.map(|thumbnail| thumbnail.file),
            })
        }
        MessageContent::MessagePhoto(content) => {
            best_photo_file(content.photo).map(|file| MessageMediaFile {
                thumbnail: Some(file.clone()),
                file,
                file_name: sanitize_filename(&format!("media_{message_id}.jpg")),
            })
        }
        MessageContent::MessageVideo(content) => {
            let extension = mime_extension(&content.video.mime_type, ".mp4");
            let file_name = file_name_or_default(content.video.file_name, message_id, &extension);
            Some(MessageMediaFile {
                file: content.video.video,
                file_name,
                thumbnail: content.video.thumbnail.map(|thumbnail| thumbnail.file),
            })
        }
        MessageContent::MessageVideoNote(content) => Some(MessageMediaFile {
            file: content.video_note.video,
            file_name: sanitize_filename(&format!("media_{message_id}.mp4")),
            thumbnail: content.video_note.thumbnail.map(|thumbnail| thumbnail.file),
        }),
        MessageContent::MessageVoiceNote(content) => {
            let extension = mime_extension(&content.voice_note.mime_type, ".ogg");
            Some(MessageMediaFile {
                file: content.voice_note.voice,
                file_name: sanitize_filename(&format!("media_{message_id}{extension}")),
                thumbnail: None,
            })
        }
        _ => None,
    }
}

fn tdlib_message_id_from_app_id(message_id: i64) -> i64 {
    if message_id > 0 && message_id < (1_i64 << 20) {
        message_id << 20
    } else {
        message_id
    }
}

fn app_message_id_from_tdlib(message_id: i64) -> i64 {
    if message_id > (1_i64 << 20) {
        message_id >> 20
    } else {
        message_id
    }
}

fn sender_id_to_string(sender: &MessageSender) -> String {
    match sender {
        MessageSender::User(sender) => sender.user_id.to_string(),
        MessageSender::Chat(sender) => sender.chat_id.to_string(),
    }
}

fn message_topic_id(topic: &Option<MessageTopic>) -> Option<i32> {
    match topic {
        Some(MessageTopic::Forum(topic)) => Some(topic.forum_topic_id),
        _ => None,
    }
}

fn reply_message_id(reply_to: &Option<MessageReplyTo>) -> Option<i64> {
    match reply_to {
        Some(MessageReplyTo::Message(reply)) => Some(app_message_id_from_tdlib(reply.message_id)),
        _ => None,
    }
}

fn content_text(content: &MessageContent) -> String {
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

fn content_media_flags(content: &MessageContent) -> (bool, bool, bool, Option<i32>, Option<i64>) {
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
            false,
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

fn tdlib_message_to_app(message: tdlib_rs::types::Message) -> TdlibMessageInfo {
    let text = content_text(&message.content);
    let (has_media, is_photo, is_video, video_duration, media_size) =
        content_media_flags(&message.content);

    TdlibMessageInfo {
        id: app_message_id_from_tdlib(message.id),
        message: text.clone(),
        date: message.date,
        out: message.is_outgoing,
        sender_id: sender_id_to_string(&message.sender_id),
        sender_name: String::new(),
        reply_to_msg_id: reply_message_id(&message.reply_to),
        topic_id: message_topic_id(&message.topic_id),
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
        is_deleted: false,
    }
}

fn tdlib_chat_to_app(chat: tdlib_rs::types::Chat) -> TdlibChatInfo {
    let (is_group, is_channel) = match &chat.r#type {
        ChatType::Private(_) | ChatType::Secret(_) => (false, false),
        ChatType::BasicGroup(_) => (true, false),
        ChatType::Supergroup(chat_type) => (!chat_type.is_channel, chat_type.is_channel),
    };

    let (
        last_message_text,
        last_message_date,
        last_message_has_media,
        last_message_is_video,
        last_message_is_photo,
    ) = match chat.last_message {
        Some(message) => {
            let text = content_text(&message.content);
            let (has_media, is_photo, is_video, _, _) = content_media_flags(&message.content);
            (text, message.date, has_media, is_video, is_photo)
        }
        None => (String::new(), 0, false, false, false),
    };

    TdlibChatInfo {
        id: chat.id.to_string(),
        title: chat.title,
        date: last_message_date,
        unread_count: chat.unread_count,
        is_group,
        is_channel,
        has_topics: chat.view_as_topics,
        last_message_text,
        last_message_date,
        last_message_has_media,
        last_message_is_video,
        last_message_is_photo,
    }
}

fn emit_mass_progress(
    app: &AppHandle,
    chat_id: i64,
    total: usize,
    downloaded: f64,
    current_file: impl Into<String>,
    is_scanning: bool,
    items: &[TdlibDownloadItem],
) {
    let _ = app.emit(
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

async fn set_state(inner: &Arc<Mutex<TdlibInner>>, app: &AppHandle, state: AuthorizationState) {
    let label = state_label(&state).to_string();
    {
        let mut guard = inner.lock().await;
        guard.auth_state = label.clone();
    }
    let _ = app.emit("tdlib-auth-state", &label);
}

async fn client_id_or_status(state: &State<'_, TdlibManager>) -> Result<i32, TdlibStatus> {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    client_id.ok_or_else(|| TdlibStatus {
        success: false,
        ready: false,
        state: "not_initialized".to_string(),
        error: Some("TDLib nao inicializado.".to_string()),
    })
}

async fn ready_client_id(state: &State<'_, TdlibManager>) -> Result<i32, TdlibStatus> {
    let client_id = client_id_or_status(state).await?;
    match functions::get_authorization_state(client_id).await {
        Ok(AuthorizationState::Ready) => Ok(client_id),
        Ok(auth_state) => Err(TdlibStatus {
            success: false,
            ready: false,
            state: state_label(&auth_state).to_string(),
            error: Some("TDLib ainda nao esta autenticado.".to_string()),
        }),
        Err(error) => Err(td_error(error)),
    }
}

fn start_receiver(app: AppHandle, manager: &TdlibManager) {
    if manager.receiver_started.swap(true, Ordering::AcqRel) {
        return;
    }

    let inner = manager.inner.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let result = tokio::task::spawn_blocking(tdlib_rs::receive).await;
            match result {
                Ok(Some((Update::AuthorizationState(update), _client_id))) => {
                    set_state(&inner, &app, update.authorization_state).await;
                }
                Ok(Some((_update, _client_id))) => {}
                Ok(None) => {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                Err(error) => {
                    let _ = app.emit("tdlib-error", format!("TDLib receiver failed: {error}"));
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }
        }
    });
}

fn td_error(error: tdlib_rs::types::Error) -> TdlibStatus {
    TdlibStatus {
        success: false,
        ready: false,
        state: "error".to_string(),
        error: Some(error.message),
    }
}

async fn download_media_file_to_path(
    client_id: i32,
    media_file: File,
    destination: &Path,
    mut on_progress: impl FnMut(u8),
    should_abort: impl Fn() -> bool,
) -> Result<(i64, bool), String> {
    if destination.exists() {
        return Ok((effective_file_size(&media_file), true));
    }

    let file_id = media_file.id;
    let mut downloaded_file =
        match functions::download_file(file_id, 32, 0, 0, false, client_id).await {
            Ok(tdlib_rs::enums::File::File(file)) => file,
            Err(error) => return Err(error.message),
        };

    let total_size = if media_file.size > 0 {
        media_file.size
    } else if media_file.expected_size > 0 {
        media_file.expected_size
    } else if downloaded_file.size > 0 {
        downloaded_file.size
    } else {
        downloaded_file.expected_size
    };
    let mut last_percent = 0u8;
    on_progress(0);

    while !downloaded_file.local.is_downloading_completed {
        if should_abort() {
            let _ = functions::cancel_download_file(file_id, false, client_id).await;
            return Err("STOP_ABORTED".to_string());
        }

        let downloaded_size = downloaded_file.local.downloaded_size.max(0);
        if total_size > 0 {
            let percent = ((downloaded_size.saturating_mul(100)) / total_size).clamp(0, 99) as u8;
            if percent != last_percent {
                last_percent = percent;
                on_progress(percent);
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        downloaded_file = match functions::get_file(file_id, client_id).await {
            Ok(tdlib_rs::enums::File::File(file)) => file,
            Err(error) => return Err(error.message),
        };
    }

    on_progress(100);

    if downloaded_file.local.path.is_empty() || !downloaded_file.local.is_downloading_completed {
        return Err("TDLib nao concluiu o download do arquivo.".to_string());
    }

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let partial_destination = destination.with_extension(format!(
        "{}part",
        destination
            .extension()
            .map(|extension| format!("{}.", extension.to_string_lossy()))
            .unwrap_or_default()
    ));
    let _ = std::fs::remove_file(&partial_destination);

    std::fs::copy(&downloaded_file.local.path, &partial_destination)
        .map_err(|error| error.to_string())?;
    std::fs::rename(&partial_destination, destination).map_err(|error| {
        let _ = std::fs::remove_file(&partial_destination);
        error.to_string()
    })?;

    Ok((effective_file_size(&downloaded_file), false))
}

async fn download_thumbnail_path(client_id: i32, thumbnail: Option<File>) -> Option<String> {
    let thumbnail = thumbnail?;
    match functions::download_file(thumbnail.id, 1, 0, 0, true, client_id).await {
        Ok(tdlib_rs::enums::File::File(file))
            if file.local.is_downloading_completed && !file.local.path.is_empty() =>
        {
            Some(file.local.path)
        }
        _ => None,
    }
}

async fn configure_if_needed(inner: &Arc<Mutex<TdlibInner>>) -> Result<(), tdlib_rs::types::Error> {
    let (client_id, api_id, api_hash, database_directory, files_directory) = {
        let guard = inner.lock().await;
        (
            guard.client_id.unwrap(),
            guard.api_id.unwrap(),
            guard.api_hash.clone().unwrap(),
            guard.database_directory.clone().unwrap(),
            guard.files_directory.clone().unwrap(),
        )
    };

    let state = functions::get_authorization_state(client_id).await?;
    if matches!(state, AuthorizationState::WaitTdlibParameters) {
        functions::set_tdlib_parameters(
            false,
            database_directory,
            files_directory,
            String::new(),
            true,
            true,
            true,
            false,
            api_id,
            api_hash,
            "pt-BR".to_string(),
            "Linux".to_string(),
            String::new(),
            env!("CARGO_PKG_VERSION").to_string(),
            client_id,
        )
        .await?;
    }

    Ok(())
}

#[tauri::command]
pub async fn tdlib_init(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    api_id: i32,
    api_hash: String,
) -> TdlibCommandResult {
    start_receiver(app.clone(), &state);

    let app_data_dir = match app.path().app_data_dir() {
        Ok(path) => path,
        Err(error) => {
            return Ok(TdlibStatus {
                success: false,
                ready: false,
                state: "error".to_string(),
                error: Some(error.to_string()),
            });
        }
    };

    let database_directory = app_data_dir.join("tdlib").join("database");
    let files_directory = app_data_dir.join("tdlib").join("files");

    if let Err(error) = std::fs::create_dir_all(&database_directory) {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "error".to_string(),
            error: Some(error.to_string()),
        });
    }
    if let Err(error) = std::fs::create_dir_all(&files_directory) {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "error".to_string(),
            error: Some(error.to_string()),
        });
    }

    let client_id = {
        let mut guard = state.inner.lock().await;
        let client_id = guard.client_id.unwrap_or_else(tdlib_rs::create_client);
        guard.client_id = Some(client_id);
        guard.api_id = Some(api_id);
        guard.api_hash = Some(api_hash);
        guard.database_directory = Some(database_directory.to_string_lossy().to_string());
        guard.files_directory = Some(files_directory.to_string_lossy().to_string());
        client_id
    };

    if let Err(error) = functions::set_log_verbosity_level(2, client_id).await {
        return Ok(td_error(error));
    }

    if let Err(error) = configure_if_needed(&state.inner).await {
        return Ok(td_error(error));
    }

    tdlib_status(state).await
}

#[tauri::command]
pub async fn tdlib_status(state: State<'_, TdlibManager>) -> TdlibCommandResult {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    let Some(client_id) = client_id else {
        return Ok(TdlibStatus {
            success: true,
            ready: false,
            state: "not_initialized".to_string(),
            error: None,
        });
    };

    match functions::get_authorization_state(client_id).await {
        Ok(auth_state) => {
            let label = state_label(&auth_state).to_string();
            {
                let mut guard = state.inner.lock().await;
                guard.auth_state = label.clone();
            }
            Ok(TdlibStatus {
                success: true,
                ready: matches!(auth_state, AuthorizationState::Ready),
                state: label,
                error: None,
            })
        }
        Err(error) => Ok(td_error(error)),
    }
}

#[tauri::command]
pub async fn tdlib_set_phone(
    state: State<'_, TdlibManager>,
    phone_number: String,
) -> TdlibCommandResult {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    let Some(client_id) = client_id else {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "not_initialized".to_string(),
            error: Some("TDLib nao inicializado.".to_string()),
        });
    };

    match functions::set_authentication_phone_number(phone_number, None, client_id).await {
        Ok(_) => tdlib_status(state).await,
        Err(error) => Ok(td_error(error)),
    }
}

#[tauri::command]
pub async fn tdlib_check_code(state: State<'_, TdlibManager>, code: String) -> TdlibCommandResult {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    let Some(client_id) = client_id else {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "not_initialized".to_string(),
            error: Some("TDLib nao inicializado.".to_string()),
        });
    };

    match functions::check_authentication_code(code, client_id).await {
        Ok(_) => tdlib_status(state).await,
        Err(error) => Ok(td_error(error)),
    }
}

#[tauri::command]
pub async fn tdlib_check_password(
    state: State<'_, TdlibManager>,
    password: String,
) -> TdlibCommandResult {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    let Some(client_id) = client_id else {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "not_initialized".to_string(),
            error: Some("TDLib nao inicializado.".to_string()),
        });
    };

    match functions::check_authentication_password(password, client_id).await {
        Ok(_) => tdlib_status(state).await,
        Err(error) => Ok(td_error(error)),
    }
}

#[tauri::command]
pub async fn tdlib_get_me(state: State<'_, TdlibManager>) -> Result<TdlibUserInfo, String> {
    let client_id = match ready_client_id(&state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibUserInfo {
                success: false,
                id: None,
                first_name: None,
                last_name: None,
                phone_number: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    match functions::get_me(client_id).await {
        Ok(tdlib_rs::enums::User::User(user)) => Ok(TdlibUserInfo {
            success: true,
            id: Some(user.id),
            first_name: Some(user.first_name),
            last_name: Some(user.last_name),
            phone_number: Some(user.phone_number),
            error: None,
        }),
        Err(error) => Ok(TdlibUserInfo {
            success: false,
            id: None,
            first_name: None,
            last_name: None,
            phone_number: None,
            error: Some(error.message),
        }),
    }
}

#[tauri::command]
pub async fn tdlib_get_chats(
    state: State<'_, TdlibManager>,
    limit: Option<i32>,
) -> Result<TdlibChatsResult, String> {
    let client_id = match ready_client_id(&state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibChatsResult {
                success: false,
                dialogs: Vec::new(),
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let chat_ids =
        match functions::get_chats(Some(ChatList::Main), limit.unwrap_or(100), client_id).await {
            Ok(tdlib_rs::enums::Chats::Chats(chats)) => chats.chat_ids,
            Err(error) => {
                return Ok(TdlibChatsResult {
                    success: false,
                    dialogs: Vec::new(),
                    error: Some(error.message),
                });
            }
        };

    let mut dialogs = Vec::new();
    for chat_id in chat_ids {
        if let Ok(tdlib_rs::enums::Chat::Chat(chat)) = functions::get_chat(chat_id, client_id).await
        {
            dialogs.push(tdlib_chat_to_app(chat));
        }
    }

    Ok(TdlibChatsResult {
        success: true,
        dialogs,
        error: None,
    })
}

#[tauri::command]
pub async fn tdlib_get_messages(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    limit: Option<i32>,
    offset_id: Option<i64>,
    topic_id: Option<i32>,
) -> Result<TdlibMessagesResult, String> {
    let client_id = match ready_client_id(&state).await {
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

    let requested_limit = limit.unwrap_or(50).clamp(1, 100);
    let from_message_id = offset_id.map(tdlib_message_id_from_app_id).unwrap_or(0);
    let history_result = if let Some(topic_id) = topic_id {
        functions::get_forum_topic_history(
            chat_id,
            topic_id,
            from_message_id,
            0,
            requested_limit,
            client_id,
        )
        .await
    } else {
        functions::get_chat_history(
            chat_id,
            from_message_id,
            0,
            requested_limit,
            false,
            client_id,
        )
        .await
    };

    let history = match history_result {
        Ok(tdlib_rs::enums::Messages::Messages(messages)) => messages,
        Err(error) => {
            return Ok(TdlibMessagesResult {
                success: false,
                messages: Vec::new(),
                has_more: false,
                oldest_message_id: None,
                error: Some(error.message),
            });
        }
    };

    let mut messages: Vec<TdlibMessageInfo> = history
        .messages
        .into_iter()
        .flatten()
        .filter(|message| !matches!(message.content, MessageContent::MessageChatDeleteMember(_)))
        .map(tdlib_message_to_app)
        .collect();
    messages.sort_by_key(|message| message.id);
    let oldest_message_id = messages.first().map(|message| message.id);
    let has_more = messages.len() as i32 >= requested_limit;

    Ok(TdlibMessagesResult {
        success: true,
        messages,
        has_more,
        oldest_message_id,
        error: None,
    })
}

#[tauri::command]
pub async fn tdlib_get_forum_topics(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    limit: Option<i32>,
) -> Result<TdlibForumTopicsResult, String> {
    let client_id = match ready_client_id(&state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibForumTopicsResult {
                success: false,
                topics: Vec::new(),
                error: status.error.or(Some(status.state)),
            });
        }
    };

    match functions::get_forum_topics(
        chat_id,
        String::new(),
        0,
        0,
        0,
        limit.unwrap_or(100),
        client_id,
    )
    .await
    {
        Ok(tdlib_rs::enums::ForumTopics::ForumTopics(topics)) => Ok(TdlibForumTopicsResult {
            success: true,
            topics: topics
                .topics
                .into_iter()
                .map(|topic| TdlibForumTopicInfo {
                    id: topic.info.forum_topic_id,
                    title: topic.info.name,
                    is_closed: topic.info.is_closed,
                    is_pinned: topic.is_pinned,
                    unread_count: topic.unread_count,
                })
                .collect(),
            error: None,
        }),
        Err(error) => Ok(TdlibForumTopicsResult {
            success: false,
            topics: Vec::new(),
            error: Some(error.message),
        }),
    }
}

#[tauri::command]
pub async fn tdlib_download_message_media(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    folder_path: String,
) -> Result<TdlibDownloadResult, String> {
    let client_id = match ready_client_id(&state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibDownloadResult {
                success: false,
                skipped: false,
                file_path: None,
                file_name: None,
                size: 0,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let message = match functions::get_message(chat_id, message_id, client_id).await {
        Ok(tdlib_rs::enums::Message::Message(message)) => message,
        Err(_) => match functions::get_message(
            chat_id,
            tdlib_message_id_from_app_id(message_id),
            client_id,
        )
        .await
        {
            Ok(tdlib_rs::enums::Message::Message(message)) => message,
            Err(error) => {
                return Ok(TdlibDownloadResult {
                    success: false,
                    skipped: false,
                    file_path: None,
                    file_name: None,
                    size: 0,
                    error: Some(error.message),
                });
            }
        },
    };

    let Some(media) = message_media_file(message.content, message.id) else {
        return Ok(TdlibDownloadResult {
            success: false,
            skipped: false,
            file_path: None,
            file_name: None,
            size: 0,
            error: Some("Mensagem sem midia baixavel pelo TDLib.".to_string()),
        });
    };
    let media_file = media.file;
    let file_name = media.file_name;

    let destination = Path::new(&folder_path).join(&file_name);
    let (size, skipped) =
        match download_media_file_to_path(client_id, media_file, &destination, |_| {}, || false)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                return Ok(TdlibDownloadResult {
                    success: false,
                    skipped: false,
                    file_path: None,
                    file_name: Some(file_name),
                    size: 0,
                    error: Some(error),
                });
            }
        };

    Ok(TdlibDownloadResult {
        success: true,
        skipped,
        file_path: Some(destination.to_string_lossy().to_string()),
        file_name: Some(file_name),
        size,
        error: None,
    })
}

#[tauri::command]
pub async fn tdlib_stop_download(state: State<'_, TdlibManager>) -> Result<(), String> {
    state.download_aborted.store(true, Ordering::Release);
    Ok(())
}

#[tauri::command]
pub async fn tdlib_start_mass_download(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    request: TdlibMassDownloadRequest,
) -> Result<TdlibMassDownloadResult, String> {
    let client_id = match ready_client_id(&state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibMassDownloadResult {
                success: false,
                downloaded_count: 0,
                skipped_count: 0,
                failed_count: 0,
                total: 0,
                aborted: false,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    state.download_aborted.store(false, Ordering::Release);

    let mut from_message_id = 0;
    let mut total = 0usize;
    let mut downloaded_count = 0usize;
    let mut skipped_count = 0usize;
    let mut failed_count = 0usize;
    let mut items: Vec<TdlibDownloadItem> = Vec::new();

    emit_mass_progress(
        &app,
        request.chat_id,
        0,
        0.0,
        "Escaneando midias com TDLib...",
        true,
        &items,
    );

    loop {
        if state.download_aborted.load(Ordering::Acquire) {
            break;
        }

        let history_result = if let Some(topic_id) = request.topic_id {
            functions::get_forum_topic_history(
                request.chat_id,
                topic_id,
                from_message_id,
                0,
                100,
                client_id,
            )
            .await
        } else {
            functions::get_chat_history(request.chat_id, from_message_id, 0, 100, false, client_id)
                .await
        };

        let history = match history_result {
            Ok(tdlib_rs::enums::Messages::Messages(messages)) => messages,
            Err(error) => {
                return Ok(TdlibMassDownloadResult {
                    success: false,
                    downloaded_count,
                    skipped_count,
                    failed_count,
                    total,
                    aborted: false,
                    error: Some(error.message),
                });
            }
        };

        if history.messages.is_empty() {
            break;
        }

        let mut next_from_message_id = from_message_id;

        for message in history.messages.into_iter().flatten() {
            next_from_message_id = message.id;

            let sender_id = message.sender_id.clone();
            let Some(media) = message_media_file(message.content, message.id) else {
                continue;
            };
            let thumbnail_path = download_thumbnail_path(client_id, media.thumbnail).await;
            let media_file = media.file;
            let file_name = media.file_name;

            total += 1;
            let mut folder = Path::new(&request.folder_path).to_path_buf();
            if request.split_by_user {
                let sender_folder = match sender_id {
                    tdlib_rs::enums::MessageSender::User(sender) => {
                        sanitize_folder_name(&format!("ID_{}", sender.user_id))
                    }
                    tdlib_rs::enums::MessageSender::Chat(sender) => {
                        sanitize_folder_name(&format!("Chat_{}", sender.chat_id))
                    }
                };
                folder = folder.join(sender_folder);
            }

            let destination = folder.join(&file_name);

            if destination.exists() {
                skipped_count += 1;
                items.push(TdlibDownloadItem {
                    name: file_name.clone(),
                    status: "skipped".to_string(),
                    progress: 100,
                    size: effective_file_size(&media_file),
                    file_path: Some(destination.to_string_lossy().to_string()),
                    thumbnail_path: thumbnail_path.clone(),
                });
                emit_mass_progress(
                    &app,
                    request.chat_id,
                    total,
                    (downloaded_count + skipped_count) as f64,
                    format!("Ignorado (ja existe): {file_name}"),
                    false,
                    &items,
                );
                continue;
            }

            let item_index = items.len();
            items.push(TdlibDownloadItem {
                name: file_name.clone(),
                status: "downloading".to_string(),
                progress: 0,
                size: effective_file_size(&media_file),
                file_path: Some(destination.to_string_lossy().to_string()),
                thumbnail_path,
            });

            emit_mass_progress(
                &app,
                request.chat_id,
                total,
                (downloaded_count + skipped_count) as f64,
                file_name.clone(),
                false,
                &items,
            );

            if state.download_aborted.load(Ordering::Acquire) {
                break;
            }

            let abort_flag = &state.download_aborted;
            let download_result = download_media_file_to_path(
                client_id,
                media_file,
                &destination,
                |percent| {
                    items[item_index].progress = percent;
                    emit_mass_progress(
                        &app,
                        request.chat_id,
                        total,
                        (downloaded_count + skipped_count) as f64 + (f64::from(percent) / 100.0),
                        format!("{file_name} ({percent}%)"),
                        false,
                        &items,
                    );
                },
                || abort_flag.load(Ordering::Acquire),
            )
            .await;

            match download_result {
                Ok((size, true)) => {
                    skipped_count += 1;
                    items[item_index].status = "skipped".to_string();
                    items[item_index].progress = 100;
                    if size > 0 {
                        items[item_index].size = size;
                    }
                }
                Ok((size, false)) => {
                    downloaded_count += 1;
                    items[item_index].status = "completed".to_string();
                    items[item_index].progress = 100;
                    if size > 0 {
                        items[item_index].size = size;
                    }
                }
                Err(error) => {
                    if error == "STOP_ABORTED" {
                        items[item_index].status = "stopped".to_string();
                        break;
                    }
                    failed_count += 1;
                    items[item_index].status = "failed".to_string();
                    items[item_index].progress = 0;
                    emit_mass_progress(
                        &app,
                        request.chat_id,
                        total,
                        (downloaded_count + skipped_count) as f64,
                        format!("{file_name}: {error}"),
                        false,
                        &items,
                    );
                    continue;
                }
            }

            emit_mass_progress(
                &app,
                request.chat_id,
                total,
                (downloaded_count + skipped_count) as f64,
                file_name,
                false,
                &items,
            );
        }

        if next_from_message_id == 0 || next_from_message_id == from_message_id {
            break;
        }
        from_message_id = next_from_message_id;
    }

    let aborted = state.download_aborted.load(Ordering::Acquire);
    let current_file = if aborted {
        format!("Parado: {downloaded_count} baixados, {skipped_count} ignorados")
    } else {
        format!("Concluido: {downloaded_count} baixados, {skipped_count} ignorados, {failed_count} falharam")
    };

    emit_mass_progress(
        &app,
        request.chat_id,
        total,
        (downloaded_count + skipped_count) as f64,
        current_file,
        false,
        &items,
    );

    Ok(TdlibMassDownloadResult {
        success: true,
        downloaded_count,
        skipped_count,
        failed_count,
        total,
        aborted,
        error: None,
    })
}
