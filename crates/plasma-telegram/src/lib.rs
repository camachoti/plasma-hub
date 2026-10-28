use base64::Engine as _;
use plasma_core::AppEnv;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, LazyLock, Mutex as StdMutex,
};
#[cfg(debug_assertions)]
use std::time::Instant;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tdlib_rs::{
    enums::{
        AuthorizationState, ChatList, ChatMemberStatus, ChatType, ConnectionState, InputFile,
        InputMessageContent, InputMessageReplyTo, LogStream, MessageContent, MessageReplyTo,
        MessageSender, MessageTopic, ReactionType, Update,
    },
    functions,
    types::{
        ChatPermissions, File, FormattedText, InputFileLocal, InputMessageDocument,
        InputMessagePhoto, InputMessageReplyToMessage, InputMessageText, InputMessageVideo,
        LogStreamFile, MessageTopicForum, MessageTopicThread, Photo, ReactionTypeEmoji,
    },
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::Mutex,
    task::JoinSet,
};

const STALE_NATIVE_PARTIAL_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const SENDER_NAME_RESOLUTION_CONCURRENCY: usize = 8;
const TDLIB_LOG_MAX_BYTES: i64 = 10 * 1024 * 1024;

#[cfg(debug_assertions)]
static STARTUP_TIMING_LOGGED: AtomicBool = AtomicBool::new(false);
static TDLIB_LOG_NOTICE: AtomicBool = AtomicBool::new(false);
static THUMBNAIL_FAILURES: LazyLock<StdMutex<HashSet<(i64, i64)>>> =
    LazyLock::new(|| StdMutex::new(HashSet::new()));

fn log_thumbnail_failure(chat_id: i64, message_id: i64) {
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

mod media_http;
pub use media_http::avatar_url as chat_avatar_url;
pub use media_http::start_server as start_plasma_media_http_server;

/// Directory that stores the chat avatars downloaded from TDLib.
pub fn telegram_avatar_dir(app: &AppEnv) -> PathBuf {
    app.paths()
        .cache_dir()
        .join("plasma-media")
        .join("telegram-avatars")
}

/// Returns the avatar already stored on disk for a chat, when there is one.
pub fn cached_chat_avatar(app: &AppEnv, chat_id: i64) -> Option<PathBuf> {
    let avatar_dir = telegram_avatar_dir(app);
    let entries = std::fs::read_dir(&avatar_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(|value| value == chat_id.to_string())
            && path.exists()
        {
            return Some(path);
        }
    }
    None
}

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
    init_lock: Arc<Mutex<()>>,
    media_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    media_abort_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    receiver_started: AtomicBool,
    download_aborted: AtomicBool,
}

#[derive(Clone, Serialize)]
pub struct TdlibStatus {
    pub success: bool,
    pub ready: bool,
    pub state: String,
    pub error: Option<String>,
}

pub type TdlibCommandResult = Result<TdlibStatus, String>;

#[derive(Serialize)]
pub struct TdlibUserInfo {
    pub success: bool,
    pub id: Option<i64>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone_number: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibDownloadResult {
    pub success: bool,
    pub skipped: bool,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
    pub size: i64,
    pub mime_type: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMediaAssetMeta {
    pub success: bool,
    pub state: String,
    pub chat_id: i64,
    pub message_id: i64,
    pub total_bytes: Option<i64>,
    pub downloaded_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub file_name: Option<String>,
    pub native_file_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub completed_at: Option<i64>,
    pub updated_at: i64,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePlaybackSource {
    pub success: bool,
    pub kind: String,
    pub url: Option<String>,
    pub file_path: Option<String>,
    pub mime_type: Option<String>,
    pub cache_state: String,
    pub total_bytes: Option<i64>,
    pub downloaded_bytes: Option<i64>,
    pub file_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSavedMedia {
    pub success: bool,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
    pub size: Option<i64>,
    pub skipped: bool,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMediaCacheStats {
    pub success: bool,
    pub total_size: i64,
    pub media_count: usize,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeMediaProgress {
    chat_id: i64,
    message_id: i64,
    stage: String,
    progress: u8,
    downloaded_bytes: Option<i64>,
    total_bytes: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibThumbnailResult {
    pub success: bool,
    pub file_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibAvatarResult {
    pub success: bool,
    pub file_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibChatInfo {
    pub id: String,
    pub title: String,
    pub date: i32,
    pub unread_count: i32,
    pub is_group: bool,
    pub is_channel: bool,
    pub is_member: bool,
    pub can_send_messages: bool,
    pub can_send_media: bool,
    pub has_topics: bool,
    pub last_message_text: String,
    pub last_message_date: i32,
    pub last_message_has_media: bool,
    pub last_message_is_video: bool,
    pub last_message_is_photo: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibChatsResult {
    pub success: bool,
    pub dialogs: Vec<TdlibChatInfo>,
    pub has_more: bool,
    pub error: Option<String>,
}

/// Emoji reaction attached to a message.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibReactionInfo {
    pub emoji: String,
    pub count: i32,
    pub mine: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibMessageInfo {
    pub id: i64,
    pub message: String,
    pub date: i32,
    pub out: bool,
    pub sender_id: String,
    pub sender_name: String,
    pub reply_to_msg_id: Option<i64>,
    pub topic_id: Option<i64>,
    pub topic_kind: Option<String>,
    pub media: bool,
    pub text: String,
    pub has_media: bool,
    pub is_photo: bool,
    pub is_video: bool,
    pub grouped_id: Option<String>,
    pub video_duration: Option<i32>,
    pub media_size: Option<i64>,
    pub thumbnail_path: Option<String>,
    pub is_deleted: bool,
    /// Emoji reactions; `#[serde(default)]` keeps old cached rows readable.
    #[serde(default)]
    pub reactions: Vec<TdlibReactionInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibMessagesResult {
    pub success: bool,
    pub messages: Vec<TdlibMessageInfo>,
    pub has_more: bool,
    pub oldest_message_id: Option<i64>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSearchMessagesResult {
    pub success: bool,
    pub messages: Vec<TdlibMessageInfo>,
    pub total_count: i32,
    pub next_from_message_id: Option<i64>,
    pub error: Option<String>,
}

/// One message returned by a cross-chat search.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibGlobalSearchMessage {
    pub chat_id: String,
    pub chat_title: String,
    pub message: TdlibMessageInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibGlobalSearchResult {
    pub success: bool,
    pub messages: Vec<TdlibGlobalSearchMessage>,
    pub total_count: i32,
    pub next_offset: String,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSharedMediaItem {
    pub id: i64,
    pub date: i32,
    pub sender_id: String,
    pub sender_name: String,
    pub has_media: bool,
    pub is_photo: bool,
    pub is_video: bool,
    pub grouped_id: Option<String>,
    pub media_size: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSharedMediaResult {
    pub success: bool,
    pub media: Vec<TdlibSharedMediaItem>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSendResult {
    pub success: bool,
    pub message: Option<TdlibMessageInfo>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibForwardResult {
    pub success: bool,
    pub messages: Vec<TdlibMessageInfo>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibForumTopicInfo {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub is_closed: bool,
    pub is_pinned: bool,
    pub unread_count: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibForumTopicsResult {
    pub success: bool,
    pub is_forum: bool,
    pub topics: Vec<TdlibForumTopicInfo>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibChatCapabilitiesResult {
    pub success: bool,
    pub chat_id: i64,
    pub is_member: bool,
    pub can_send_messages: bool,
    pub can_send_media: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibMassDownloadRequest {
    pub chat_id: i64,
    pub folder_path: String,
    pub topic_id: Option<i64>,
    pub topic_kind: Option<String>,
    pub split_by_user: bool,
    pub split_by_album: bool,
    pub album_split_mode: Option<String>,
}

#[derive(Serialize)]
pub struct TdlibMassDownloadResult {
    pub success: bool,
    pub downloaded_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub total: usize,
    pub aborted: bool,
    pub error: Option<String>,
    pub tracking_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibDownloadItem {
    pub name: String,
    pub message_id: i64,
    pub status: String,
    pub progress: u8,
    pub size: i64,
    pub file_path: Option<String>,
    pub thumbnail_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibDownloadProgress {
    pub chat_id: i64,
    pub total: usize,
    pub downloaded: f64,
    pub current_file: String,
    pub is_scanning: bool,
    pub items: Vec<TdlibDownloadItem>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TdlibNewMessageEvent {
    chat_id: i64,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
    message: TdlibMessageInfo,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TdlibDeletedMessagesEvent {
    chat_id: i64,
    message_ids: Vec<i64>,
}

struct PendingMassDownload {
    file: File,
    thumbnail: Option<File>,
    destination: PathBuf,
    item_index: usize,
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
    let mime_type = mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase();
    match mime_type.as_str() {
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

/// Whether a document is a video that the lightbox can preview and play.
///
/// Telegram sends some videos (notably `.mov`/QuickTime) as documents instead
/// of `MessageVideo`. They still need a thumbnail and the video player.
fn document_is_video(mime_type: &str, file_name: &str) -> bool {
    let mime_type = mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase();
    if mime_type.starts_with("video/") {
        return true;
    }
    if !mime_type.is_empty() && mime_type != "application/octet-stream" {
        return false;
    }

    Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| {
            matches!(extension.as_str(), "mov" | "qt" | "mp4" | "m4v" | "webm")
        })
}

fn playback_extension(mime_type: Option<&str>, file_name: Option<&str>) -> String {
    let mime_extension = mime_type
        .map(|value| mime_extension(value, ""))
        .filter(|value| !value.is_empty());
    if let Some(extension) = mime_extension {
        return extension;
    }

    let extension = file_name
        .and_then(|value| Path::new(value).extension())
        .and_then(|value| value.to_str())
        .map(|value| format!(".{}", value.to_ascii_lowercase()))
        .filter(|value| !value.is_empty());
    extension.unwrap_or_else(|| ".mp4".to_string())
}

fn normalize_playback_mime_type(
    mime_type: Option<&str>,
    file_name: Option<&str>,
) -> Option<String> {
    let mime_type = mime_type
        .map(|value| value.split(';').next().unwrap_or(value).trim())
        .filter(|value| !value.is_empty() && *value != "application/octet-stream");
    if let Some(mime_type) = mime_type {
        return Some(mime_type.to_ascii_lowercase());
    }

    match playback_extension(None, file_name).as_str() {
        ".mp4" | ".m4v" => Some("video/mp4".to_string()),
        ".webm" => Some("video/webm".to_string()),
        ".mov" => Some("video/quicktime".to_string()),
        ".m4a" => Some("audio/mp4".to_string()),
        ".mp3" => Some("audio/mpeg".to_string()),
        ".ogg" => Some("audio/ogg".to_string()),
        ".jpg" | ".jpeg" => Some("image/jpeg".to_string()),
        ".png" => Some("image/png".to_string()),
        ".webp" => Some("image/webp".to_string()),
        _ => Some("video/mp4".to_string()),
    }
}

fn existing_faststart_playback_file(media_dir: &Path, mime_type: Option<&str>) -> Option<PathBuf> {
    if mime_type != Some("video/mp4") {
        return None;
    }

    let destination = media_dir.join("playback.faststart.mp4");
    if file_is_complete(&destination, None) {
        return Some(destination);
    }
    None
}

fn ensure_native_playback_file(
    app: &AppEnv,
    meta: &NativeMediaAssetMeta,
) -> Result<(String, Option<String>), String> {
    let Some(source_path) = meta.native_file_path.as_ref() else {
        return Err("Mídia nativa sem arquivo.".to_string());
    };
    let source = Path::new(source_path);
    if !file_is_complete(source, meta.total_bytes) {
        return Err("Arquivo de mídia incompleto no cache.".to_string());
    }

    let mime_type =
        normalize_playback_mime_type(meta.mime_type.as_deref(), meta.file_name.as_deref());
    let media_dir = native_media_dir(app, meta.chat_id, meta.message_id)?;
    if let Some(faststart_path) = existing_faststart_playback_file(&media_dir, mime_type.as_deref())
    {
        return Ok((faststart_path.to_string_lossy().to_string(), mime_type));
    }

    let extension = playback_extension(mime_type.as_deref(), meta.file_name.as_deref());
    let playback_path = media_dir.join(format!("playback{extension}"));

    if source == playback_path {
        return Ok((source_path.clone(), mime_type));
    }

    if file_is_complete(&playback_path, meta.total_bytes) {
        return Ok((playback_path.to_string_lossy().to_string(), mime_type));
    }

    let _ = std::fs::remove_file(&playback_path);
    match std::fs::hard_link(source, &playback_path) {
        Ok(_) => Ok((playback_path.to_string_lossy().to_string(), mime_type)),
        Err(_) => Ok((source_path.clone(), mime_type)),
    }
}

fn parse_plasma_media_path(path: &str) -> Option<(i64, i64)> {
    let mut parts = path.trim_start_matches('/').split('/');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("telegram"), Some(chat_id), Some(message_id), None) => {
            let chat_id = chat_id.parse::<i64>().ok()?;
            let message_id = message_id.parse::<i64>().ok()?;
            Some((chat_id, message_id))
        }
        _ => None,
    }
}

fn parse_range_header(range_header: &str, len: u64) -> Option<(u64, u64)> {
    if len == 0 {
        return None;
    }
    let range = range_header.strip_prefix("bytes=")?;
    let first = range.split(',').next()?.trim();
    let (start_raw, end_raw) = first.split_once('-')?;

    if start_raw.is_empty() {
        let suffix = end_raw.parse::<u64>().ok()?.min(len);
        let start = len.saturating_sub(suffix);
        return Some((start, len - 1));
    }

    let start = start_raw.parse::<u64>().ok()?;
    if start >= len {
        return None;
    }
    let end = if end_raw.is_empty() {
        len - 1
    } else {
        end_raw.parse::<u64>().ok()?.min(len - 1)
    };
    if end < start {
        return None;
    }
    Some((start, end))
}

pub fn media_protocol_response(
    status: http::StatusCode,
    body: impl Into<Cow<'static, [u8]>>,
    content_type: Option<&str>,
) -> http::Response<Cow<'static, [u8]>> {
    let mut builder = http::Response::builder()
        .status(status)
        .header(http::header::ACCEPT_RANGES, "bytes")
        .header(http::header::CACHE_CONTROL, "no-store");
    if let Some(content_type) = content_type {
        builder = builder.header(http::header::CONTENT_TYPE, content_type);
    }
    builder.body(body.into()).unwrap_or_else(|_| {
        http::Response::builder()
            .status(http::StatusCode::INTERNAL_SERVER_ERROR)
            .body(Cow::Owned(Vec::new()))
            .unwrap()
    })
}

pub fn media_protocol_error(
    status: http::StatusCode,
    message: &str,
) -> http::Response<Cow<'static, [u8]>> {
    media_protocol_response(
        status,
        Cow::Owned(message.as_bytes().to_vec()),
        Some("text/plain; charset=utf-8"),
    )
}

pub fn get_plasma_media_response(
    app: &AppEnv,
    request: http::Request<Vec<u8>>,
) -> Result<http::Response<Cow<'static, [u8]>>, String> {
    let Some((chat_id, message_id)) = parse_plasma_media_path(request.uri().path()) else {
        tracing::debug!(path = %request.uri().path(), "invalid media path");
        return Ok(media_protocol_error(
            http::StatusCode::BAD_REQUEST,
            "URL de mídia inválida.",
        ));
    };

    let Some(meta) = complete_native_media_meta(app, chat_id, message_id)? else {
        tracing::debug!(chat_id, message_id, "media missing from cache");
        return Ok(media_protocol_error(
            http::StatusCode::NOT_FOUND,
            "Mídia não encontrada no cache.",
        ));
    };
    let (playback_path, playback_mime_type) = ensure_native_playback_file(app, &meta)?;
    let content_type = playback_mime_type
        .or(meta.mime_type.clone())
        .unwrap_or_else(|| "video/mp4".to_string());
    let mut file = std::fs::File::open(&playback_path).map_err(|error| error.to_string())?;
    let len = file.metadata().map_err(|error| error.to_string())?.len();
    if request.method() == http::Method::HEAD {
        return http::Response::builder()
            .status(http::StatusCode::OK)
            .header(http::header::ACCEPT_RANGES, "bytes")
            .header(http::header::CONTENT_TYPE, content_type)
            .header(http::header::CONTENT_LENGTH, len)
            .body(Cow::Owned(Vec::new()))
            .map_err(|error| error.to_string());
    }

    if let Some(range_header) = request
        .headers()
        .get(http::header::RANGE)
        .and_then(|value| value.to_str().ok())
    {
        let Some((start, mut end)) = parse_range_header(range_header, len) else {
            tracing::debug!(
                chat_id,
                message_id,
                range = %range_header,
                len,
                "invalid media range"
            );
            return http::Response::builder()
                .status(http::StatusCode::RANGE_NOT_SATISFIABLE)
                .header(http::header::ACCEPT_RANGES, "bytes")
                .header(http::header::CONTENT_RANGE, format!("bytes */{len}"))
                .body(Cow::Owned(Vec::new()))
                .map_err(|error| error.to_string());
        };

        const MAX_RANGE_BYTES: u64 = 1024 * 1024;
        const MIN_INITIAL_RANGE_BYTES: u64 = 256 * 1024;
        if start == 0 {
            end = end.max(
                MIN_INITIAL_RANGE_BYTES
                    .saturating_sub(1)
                    .min(len.saturating_sub(1)),
            );
        }
        end = start + (end - start).min(MAX_RANGE_BYTES - 1);
        let read_len = end + 1 - start;
        let mut buffer = vec![0_u8; read_len as usize];
        file.seek(SeekFrom::Start(start))
            .map_err(|error| error.to_string())?;
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        buffer.truncate(read);
        return http::Response::builder()
            .status(http::StatusCode::PARTIAL_CONTENT)
            .header(http::header::ACCEPT_RANGES, "bytes")
            .header(http::header::CONTENT_TYPE, content_type)
            .header(
                http::header::CONTENT_RANGE,
                format!("bytes {start}-{end}/{len}"),
            )
            .header(http::header::CONTENT_LENGTH, buffer.len())
            .body(Cow::Owned(buffer))
            .map_err(|error| error.to_string());
    }

    let max_initial = len.min(1024 * 1024);
    let mut buffer = vec![0_u8; max_initial as usize];
    let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
    buffer.truncate(read);
    http::Response::builder()
        .status(if read as u64 == len {
            http::StatusCode::OK
        } else {
            http::StatusCode::PARTIAL_CONTENT
        })
        .header(http::header::ACCEPT_RANGES, "bytes")
        .header(http::header::CONTENT_TYPE, content_type)
        .header(
            http::header::CONTENT_RANGE,
            format!("bytes 0-{}/{len}", read.saturating_sub(1)),
        )
        .header(http::header::CONTENT_LENGTH, buffer.len())
        .body(Cow::Owned(buffer))
        .map_err(|error| error.to_string())
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

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

fn native_media_dir(app: &AppEnv, chat_id: i64, message_id: i64) -> Result<PathBuf, String> {
    Ok(app
        .paths()
        .cache_dir()
        .join("plasma-media")
        .join("telegram")
        .join(chat_id.to_string())
        .join(message_id.to_string()))
}

fn native_media_root_dir(app: &AppEnv) -> Result<PathBuf, String> {
    Ok(app
        .paths()
        .cache_dir()
        .join("plasma-media")
        .join("telegram"))
}

fn native_media_meta_path(app: &AppEnv, chat_id: i64, message_id: i64) -> Result<PathBuf, String> {
    Ok(native_media_dir(app, chat_id, message_id)?.join("meta.json"))
}

fn empty_native_media_meta(chat_id: i64, message_id: i64) -> NativeMediaAssetMeta {
    NativeMediaAssetMeta {
        success: false,
        state: "empty".to_string(),
        chat_id,
        message_id,
        total_bytes: None,
        downloaded_bytes: None,
        mime_type: None,
        file_name: None,
        native_file_path: None,
        thumbnail_path: None,
        completed_at: None,
        updated_at: now_millis(),
        error: None,
    }
}

fn read_native_media_meta(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
) -> Result<NativeMediaAssetMeta, String> {
    let meta_path = native_media_meta_path(app, chat_id, message_id)?;
    if !meta_path.exists() {
        return Ok(empty_native_media_meta(chat_id, message_id));
    }

    let raw = std::fs::read_to_string(&meta_path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

fn write_native_media_meta(app: &AppEnv, meta: &NativeMediaAssetMeta) -> Result<(), String> {
    let meta_path = native_media_meta_path(app, meta.chat_id, meta.message_id)?;
    if let Some(parent) = meta_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let tmp_path = meta_path.with_extension("json.tmp");
    let raw = serde_json::to_vec_pretty(meta).map_err(|error| error.to_string())?;
    std::fs::write(&tmp_path, raw).map_err(|error| error.to_string())?;
    std::fs::rename(&tmp_path, &meta_path).map_err(|error| {
        let _ = std::fs::remove_file(&tmp_path);
        error.to_string()
    })
}

fn complete_native_media_meta(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
) -> Result<Option<NativeMediaAssetMeta>, String> {
    let meta = read_native_media_meta(app, chat_id, message_id)?;
    let Some(path) = meta.native_file_path.as_ref() else {
        return Ok(None);
    };
    if meta.state == "complete" && file_is_complete(Path::new(path), meta.total_bytes) {
        return Ok(Some(meta));
    }
    Ok(None)
}

fn partial_file_path(destination: &Path) -> PathBuf {
    destination.with_extension(format!(
        "{}part",
        destination
            .extension()
            .map(|extension| format!("{}.", extension.to_string_lossy()))
            .unwrap_or_default()
    ))
}

fn file_is_complete(path: &Path, expected_size: Option<i64>) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    let Some(expected_size) = expected_size.filter(|size| *size > 0) else {
        return true;
    };
    metadata.len() >= expected_size as u64
}

fn cleanup_incomplete_file(destination: &Path, expected_size: Option<i64>) {
    let partial_destination = partial_file_path(destination);
    let _ = std::fs::remove_file(&partial_destination);
    if destination.exists() && !file_is_complete(destination, expected_size) {
        let _ = std::fs::remove_file(destination);
    }
}

fn is_stale_file(metadata: &std::fs::Metadata, max_age: Duration) -> bool {
    metadata
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= max_age)
}

fn is_native_partial_artifact(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "part" | "tmp"))
}

fn cleanup_stale_native_partials_in_dir(path: &Path, max_age: Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };

    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(metadata) = entry.metadata() else {
            continue;
        };

        if metadata.is_dir() {
            removed += cleanup_stale_native_partials_in_dir(&path, max_age);
            continue;
        }

        if metadata.is_file()
            && is_native_partial_artifact(&path)
            && is_stale_file(&metadata, max_age)
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

fn cleanup_stale_native_partials(app: &AppEnv) -> usize {
    let Ok(root) = native_media_root_dir(app) else {
        return 0;
    };
    if !root.exists() {
        return 0;
    }
    cleanup_stale_native_partials_in_dir(&root, STALE_NATIVE_PARTIAL_AGE)
}

fn directory_size(path: &Path) -> i64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| {
            let path = entry.path();
            match entry.metadata() {
                Ok(metadata) if metadata.is_file() => metadata.len().min(i64::MAX as u64) as i64,
                Ok(metadata) if metadata.is_dir() => directory_size(&path),
                _ => 0,
            }
        })
        .sum()
}

fn native_media_meta_from_path(path: &Path) -> Option<NativeMediaAssetMeta> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn collect_native_media_items(
    app: &AppEnv,
) -> Result<Vec<(PathBuf, NativeMediaAssetMeta, i64)>, String> {
    let root = native_media_root_dir(app)?;
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for chat_entry in std::fs::read_dir(root).map_err(|error| error.to_string())? {
        let Ok(chat_entry) = chat_entry else {
            continue;
        };
        let chat_path = chat_entry.path();
        if !chat_path.is_dir() {
            continue;
        }

        let Ok(message_entries) = std::fs::read_dir(chat_path) else {
            continue;
        };
        for message_entry in message_entries.flatten() {
            let message_path = message_entry.path();
            if !message_path.is_dir() {
                continue;
            }
            let meta_path = message_path.join("meta.json");
            let Some(meta) = native_media_meta_from_path(&meta_path) else {
                continue;
            };
            let size = directory_size(&message_path);
            items.push((message_path, meta, size));
        }
    }

    Ok(items)
}

fn native_media_cache_stats(app: &AppEnv) -> NativeMediaCacheStats {
    let _ = cleanup_stale_native_partials(app);
    match collect_native_media_items(app) {
        Ok(items) => NativeMediaCacheStats {
            success: true,
            total_size: items.iter().map(|(_, _, size)| *size).sum(),
            media_count: items.len(),
            error: None,
        },
        Err(error) => NativeMediaCacheStats {
            success: false,
            total_size: 0,
            media_count: 0,
            error: Some(error),
        },
    }
}

fn evict_native_media_cache(
    app: &AppEnv,
    max_cache_size: i64,
) -> Result<NativeMediaCacheStats, String> {
    let _ = cleanup_stale_native_partials(app);
    if max_cache_size <= 0 {
        return Ok(native_media_cache_stats(app));
    }

    let mut items = collect_native_media_items(app)?;
    let mut total_size: i64 = items.iter().map(|(_, _, size)| *size).sum();
    if total_size <= max_cache_size {
        return Ok(NativeMediaCacheStats {
            success: true,
            total_size,
            media_count: items.len(),
            error: None,
        });
    }

    items.sort_by_key(|(_, meta, _)| {
        meta.completed_at
            .or(Some(meta.updated_at))
            .unwrap_or_default()
    });
    let mut remaining_count = items.len();
    for (path, _meta, size) in items {
        if total_size <= max_cache_size {
            break;
        }
        if std::fs::remove_dir_all(&path).is_ok() {
            total_size = total_size.saturating_sub(size);
            remaining_count = remaining_count.saturating_sub(1);
        }
    }

    Ok(NativeMediaCacheStats {
        success: true,
        total_size,
        media_count: remaining_count,
        error: None,
    })
}

async fn native_media_lock(
    manager: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> Arc<Mutex<()>> {
    let key = format!("{chat_id}:{message_id}");
    let mut locks = manager.media_locks.lock().await;
    locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

async fn native_media_abort_flag(
    manager: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> Arc<AtomicBool> {
    let key = format!("{chat_id}:{message_id}");
    let mut flags = manager.media_abort_flags.lock().await;
    flags
        .entry(key)
        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
        .clone()
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

fn thumbnail_photo_file(photo: Photo) -> Option<File> {
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

struct MessageMediaFile {
    file: File,
    file_name: String,
    mime_type: String,
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

fn message_thumbnail_file(content: MessageContent) -> Option<File> {
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
fn message_minithumbnail(content: &MessageContent) -> Option<&str> {
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

fn write_minithumbnail(data: &str, media_dir: &Path) -> Option<String> {
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

fn local_thumbnail_path(thumbnail: Option<File>) -> Option<String> {
    let thumbnail = thumbnail?;
    if thumbnail.local.is_downloading_completed && !thumbnail.local.path.is_empty() {
        Some(thumbnail.local.path)
    } else {
        None
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

fn is_hidden_membership_service_message(content: &MessageContent) -> bool {
    matches!(
        content,
        MessageContent::MessageChatAddMembers(_)
            | MessageContent::MessageChatJoinByLink
            | MessageContent::MessageChatJoinByRequest
            | MessageContent::MessageChatDeleteMember(_)
    )
}

async fn resolve_sender_name(client_id: i32, sender_id: &str) -> String {
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

async fn resolve_sender_folder_name(
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

fn message_topic_id(topic: &Option<MessageTopic>) -> Option<i64> {
    match topic {
        Some(MessageTopic::Forum(topic)) => Some(i64::from(topic.forum_topic_id)),
        Some(MessageTopic::Thread(topic)) => {
            Some(app_message_id_from_tdlib(topic.message_thread_id))
        }
        _ => None,
    }
}

fn message_topic_kind(topic: &Option<MessageTopic>) -> Option<String> {
    match topic {
        Some(MessageTopic::Forum(_)) => Some("forum".to_string()),
        Some(MessageTopic::Thread(_)) => Some("thread".to_string()),
        Some(MessageTopic::DirectMessages(_)) => Some("direct_messages".to_string()),
        Some(MessageTopic::SavedMessages(_)) => Some("saved_messages".to_string()),
        None => None,
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

fn is_likely_album_separator_text(text: &str) -> bool {
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
fn local_video_path(content: &MessageContent) -> Option<String> {
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

fn tdlib_message_to_app(message: tdlib_rs::types::Message) -> TdlibMessageInfo {
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
fn message_reactions(message: &tdlib_rs::types::Message) -> Vec<TdlibReactionInfo> {
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

async fn resolve_message_sender_names(client_id: i32, messages: &mut [TdlibMessageInfo]) {
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
async fn resolve_chat_titles(client_id: i32, entries: &mut [TdlibGlobalSearchMessage]) {
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

fn plain_formatted_text(text: impl Into<String>) -> FormattedText {
    FormattedText {
        text: text.into(),
        entities: Vec::new(),
    }
}

fn optional_caption(text: &str) -> Option<FormattedText> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(plain_formatted_text(trimmed.to_string()))
    }
}

fn message_topic_from_id(topic_id: Option<i64>, topic_kind: Option<&str>) -> Option<MessageTopic> {
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

fn reply_to_from_id(reply_to_id: Option<i64>) -> Option<InputMessageReplyTo> {
    reply_to_id.map(|message_id| {
        InputMessageReplyTo::Message(InputMessageReplyToMessage {
            message_id: tdlib_message_id_from_app_id(message_id),
            quote: None,
            checklist_task_id: 0,
        })
    })
}

fn input_file_local(path: &str) -> InputFile {
    InputFile::Local(InputFileLocal {
        path: path.to_string(),
    })
}

fn input_content_for_local_media(file_path: &str, caption: &str) -> InputMessageContent {
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

fn permissions_can_send_media(permissions: &ChatPermissions) -> bool {
    permissions.can_send_audios
        || permissions.can_send_documents
        || permissions.can_send_photos
        || permissions.can_send_videos
        || permissions.can_send_video_notes
        || permissions.can_send_voice_notes
        || permissions.can_send_other_messages
}

fn member_status_capabilities(
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

fn chat_send_capabilities(
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

fn tdlib_chat_to_app(chat: tdlib_rs::types::Chat) -> TdlibChatInfo {
    let (is_group, is_channel) = match &chat.r#type {
        ChatType::Private(_) | ChatType::Secret(_) => (false, false),
        ChatType::BasicGroup(_) => (true, false),
        ChatType::Supergroup(chat_type) => (!chat_type.is_channel, chat_type.is_channel),
    };
    let (is_member, can_send_messages, can_send_media) =
        chat_send_capabilities(&chat.r#type, &chat.permissions);

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
        is_member,
        can_send_messages,
        can_send_media,
        has_topics: chat.view_as_topics,
        last_message_text,
        last_message_date,
        last_message_has_media,
        last_message_is_video,
        last_message_is_photo,
    }
}

fn tdlib_user_to_contact_chat(user: tdlib_rs::types::User) -> TdlibChatInfo {
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
        last_message_text: String::new(),
        last_message_date: 0,
        last_message_has_media: false,
        last_message_is_video: false,
        last_message_is_photo: false,
    }
}

fn emit_mass_progress(
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

fn emit_new_message(app: &AppEnv, chat_id: i64, message: TdlibMessageInfo) {
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

fn emit_deleted_messages(app: &AppEnv, chat_id: i64, message_ids: Vec<i64>) {
    app.emit(
        "tdlib-messages-deleted",
        TdlibDeletedMessagesEvent {
            chat_id,
            message_ids,
        },
    );
}

async fn set_state(inner: &Arc<Mutex<TdlibInner>>, app: &AppEnv, state: AuthorizationState) {
    let label = state_label(&state).to_string();
    {
        let mut guard = inner.lock().await;
        guard.auth_state = label.clone();
    }
    app.emit("tdlib-auth-state", &label);
}

fn connection_state_label(state: &ConnectionState) -> &'static str {
    match state {
        ConnectionState::WaitingForNetwork => "waiting_for_network",
        ConnectionState::ConnectingToProxy => "connecting_to_proxy",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Updating => "updating",
        ConnectionState::Ready => "ready",
    }
}

async fn client_id_or_status(state: &TdlibManager) -> Result<i32, TdlibStatus> {
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

async fn ready_client_id(state: &TdlibManager) -> Result<i32, TdlibStatus> {
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

fn start_receiver(app: AppEnv, manager: &TdlibManager) {
    if manager.receiver_started.swap(true, Ordering::AcqRel) {
        return;
    }

    let inner = manager.inner.clone();
    tokio::spawn(async move {
        loop {
            let result = tokio::task::spawn_blocking(tdlib_rs::receive).await;
            match result {
                Ok(Some((Update::AuthorizationState(update), _client_id))) => {
                    set_state(&inner, &app, update.authorization_state).await;
                }
                Ok(Some((Update::ConnectionState(update), _client_id))) => {
                    app.emit(
                        "tdlib-connection-state",
                        connection_state_label(&update.state),
                    );
                }
                Ok(Some((Update::NewMessage(update), client_id))) => {
                    if is_hidden_membership_service_message(&update.message.content) {
                        continue;
                    }

                    let chat_id = update.message.chat_id;
                    let message = tdlib_message_to_app(update.message);
                    let sender_id = message.sender_id.clone();
                    if message.out {
                        emit_new_message(&app, chat_id, message);
                    } else {
                        let app = app.clone();
                        tokio::spawn(async move {
                            let sender_name = tokio::time::timeout(
                                Duration::from_millis(800),
                                resolve_sender_name(client_id, &sender_id),
                            )
                            .await
                            .unwrap_or_default();
                            let message = if sender_name.trim().is_empty() {
                                message
                            } else {
                                TdlibMessageInfo {
                                    sender_name,
                                    ..message
                                }
                            };
                            emit_new_message(&app, chat_id, message);
                        });
                    }
                }
                Ok(Some((Update::DeleteMessages(update), _client_id))) => {
                    if update.is_permanent && !update.from_cache {
                        emit_deleted_messages(
                            &app,
                            update.chat_id,
                            update
                                .message_ids
                                .into_iter()
                                .map(app_message_id_from_tdlib)
                                .collect(),
                        );
                    }
                }
                Ok(Some((_update, _client_id))) => {}
                Ok(None) => {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                Err(error) => {
                    app.emit("tdlib-error", format!("TDLib receiver failed: {error}"));
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

fn td_timeout(message: impl Into<String>) -> TdlibStatus {
    TdlibStatus {
        success: false,
        ready: false,
        state: "timeout".to_string(),
        error: Some(message.into()),
    }
}

async fn download_media_file_to_path(
    client_id: i32,
    media_file: File,
    destination: &Path,
    mut on_progress: impl FnMut(u8, &'static str),
    should_abort: impl Fn() -> bool,
) -> Result<(i64, bool), String> {
    let expected_size = effective_file_size(&media_file);
    if file_is_complete(destination, Some(expected_size)) {
        return Ok((expected_size, true));
    }
    cleanup_incomplete_file(destination, Some(expected_size));

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
    on_progress(0, "downloading");

    while !downloaded_file.local.is_downloading_completed {
        if should_abort() {
            let _ = functions::cancel_download_file(file_id, false, client_id).await;
            return Err("STOP_ABORTED".to_string());
        }

        let downloaded_size = downloaded_file.local.downloaded_size.max(0);
        if total_size > 0 {
            let percent = ((downloaded_size.saturating_mul(95)) / total_size).clamp(0, 95) as u8;
            if percent != last_percent {
                last_percent = percent;
                on_progress(percent, "downloading");
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        downloaded_file = match functions::get_file(file_id, client_id).await {
            Ok(tdlib_rs::enums::File::File(file)) => file,
            Err(error) => return Err(error.message),
        };
    }

    if downloaded_file.local.path.is_empty() || !downloaded_file.local.is_downloading_completed {
        return Err("TDLib nao concluiu o download do arquivo.".to_string());
    }

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let partial_destination = partial_file_path(destination);
    let _ = std::fs::remove_file(&partial_destination);

    let copy_result = copy_file_with_progress(
        Path::new(&downloaded_file.local.path),
        &partial_destination,
        effective_file_size(&downloaded_file),
        &mut on_progress,
        &should_abort,
    )
    .await;
    if let Err(error) = copy_result {
        let _ = std::fs::remove_file(&partial_destination);
        return Err(error);
    }
    std::fs::rename(&partial_destination, destination).map_err(|error| {
        let _ = std::fs::remove_file(&partial_destination);
        error.to_string()
    })?;

    Ok((effective_file_size(&downloaded_file), false))
}

async fn copy_file_with_progress(
    source: &Path,
    destination: &Path,
    total_size: i64,
    on_progress: &mut impl FnMut(u8, &'static str),
    should_abort: &impl Fn() -> bool,
) -> Result<(), String> {
    on_progress(96, "saving");
    let mut input = tokio::fs::File::open(source)
        .await
        .map_err(|error| error.to_string())?;
    let mut output = tokio::fs::File::create(destination)
        .await
        .map_err(|error| error.to_string())?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut copied = 0_i64;
    let mut last_percent = 96_u8;

    loop {
        if should_abort() {
            return Err("STOP_ABORTED".to_string());
        }
        let read = input
            .read(&mut buffer)
            .await
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .await
            .map_err(|error| error.to_string())?;
        copied = copied.saturating_add(read as i64);

        if total_size > 0 {
            let percent = 96 + ((copied.saturating_mul(4)) / total_size).clamp(0, 4) as u8;
            if percent != last_percent {
                last_percent = percent;
                on_progress(percent, "saving");
            }
        }
    }

    output.flush().await.map_err(|error| error.to_string())?;
    on_progress(100, "saving");
    Ok(())
}

async fn download_thumbnail_path(client_id: i32, thumbnail: Option<File>) -> Option<String> {
    let thumbnail = thumbnail?;
    if thumbnail.local.is_downloading_completed && !thumbnail.local.path.is_empty() {
        return Some(thumbnail.local.path);
    }

    let file_id = thumbnail.id;
    let mut file = match functions::download_file(file_id, 16, 0, 0, true, client_id).await {
        Ok(tdlib_rs::enums::File::File(file)) => file,
        _ => return None,
    };

    for _ in 0..50 {
        if file.local.is_downloading_completed && !file.local.path.is_empty() {
            return Some(file.local.path);
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        file = match functions::get_file(file_id, client_id).await {
            Ok(tdlib_rs::enums::File::File(file)) => file,
            _ => return None,
        };
    }

    None
}

async fn download_thumbnail_to_cache(
    client_id: i32,
    thumbnail: Option<File>,
    media_dir: &Path,
) -> Option<String> {
    let source_path = download_thumbnail_path(client_id, thumbnail).await?;
    if !Path::new(&source_path).exists() {
        return None;
    }

    let extension = Path::new(&source_path)
        .extension()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "jpg".to_string());
    let destination = media_dir.join(format!("thumbnail.{extension}"));

    if destination.exists() {
        return Some(destination.to_string_lossy().to_string());
    }

    if std::fs::create_dir_all(media_dir).is_err() {
        return None;
    }

    match std::fs::copy(&source_path, &destination) {
        Ok(_) => Some(destination.to_string_lossy().to_string()),
        Err(_) => Some(source_path),
    }
}

fn generate_video_thumbnail_to_cache(source_path: &str, destination: &Path) -> Option<String> {
    if destination.is_file() {
        return Some(destination.to_string_lossy().to_string());
    }
    if !Path::new(source_path).is_file() {
        return None;
    }
    std::fs::create_dir_all(destination.parent()?).ok()?;

    plasma_media::generate_video_thumbnail_to_path(Path::new(source_path), destination).ok()?;
    destination
        .is_file()
        .then(|| destination.to_string_lossy().to_string())
}

async fn download_avatar_to_cache(
    app: &AppEnv,
    client_id: i32,
    chat_id: i64,
    photo: Option<File>,
) -> Option<String> {
    let source_path = download_thumbnail_path(client_id, photo).await?;
    if !Path::new(&source_path).exists() {
        return None;
    }

    let avatar_dir = app
        .paths()
        .cache_dir()
        .join("plasma-media")
        .join("telegram-avatars");
    if std::fs::create_dir_all(&avatar_dir).is_err() {
        return None;
    }

    let extension = Path::new(&source_path)
        .extension()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "jpg".to_string());
    let destination = avatar_dir.join(format!("{chat_id}.{extension}"));
    if destination.exists() {
        return Some(destination.to_string_lossy().to_string());
    }

    match std::fs::copy(&source_path, &destination) {
        Ok(_) => Some(destination.to_string_lossy().to_string()),
        Err(_) => Some(source_path),
    }
}

async fn resolve_message_media(
    client_id: i32,
    chat_id: i64,
    message_id: i64,
) -> Result<MessageMediaFile, String> {
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
            Err(error) => return Err(error.message),
        },
    };

    message_media_file(message.content, message.id)
        .ok_or_else(|| "Mensagem sem midia baixavel pelo TDLib.".to_string())
}

fn emit_native_media_progress(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
    stage: &str,
    progress: u8,
    total_bytes: Option<i64>,
) {
    let downloaded_bytes = total_bytes
        .map(|total| ((total.max(0) as f64) * (f64::from(progress) / 100.0)).round() as i64);
    app.emit(
        "telegram-media-progress",
        NativeMediaProgress {
            chat_id,
            message_id,
            stage: stage.to_string(),
            progress,
            downloaded_bytes,
            total_bytes,
        },
    );
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

pub async fn tdlib_init(
    app: &AppEnv,
    state: &TdlibManager,
    api_id: i32,
    api_hash: String,
) -> TdlibCommandResult {
    // Requests are completed by `tdlib_rs::receive`; start the receiver before
    // sending the logging requests below, otherwise they time out.
    start_receiver(app.clone(), state);
    let _init_guard = state.init_lock.lock().await;

    let app_data_dir = app.paths().data_dir().to_path_buf();

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

    // Keep TDLib's internal chatter out of the terminal. The file is rotated
    // by TDLib and is the first place to look when Telegram misbehaves.
    let tdlib_log_path = app_data_dir.join("tdlib").join("tdlib.log");
    let log_stream = LogStream::File(LogStreamFile {
        path: tdlib_log_path.to_string_lossy().to_string(),
        max_file_size: TDLIB_LOG_MAX_BYTES,
        redirect_stderr: false,
    });
    match tokio::time::timeout(
        Duration::from_secs(4),
        functions::set_log_stream(log_stream, client_id),
    )
    .await
    {
        Ok(Ok(())) => {
            if !TDLIB_LOG_NOTICE.swap(true, Ordering::Relaxed) {
                tracing::info!(
                    path = %tdlib_log_path.display(),
                    "TDLib internal logs redirected to file"
                );
            }
        }
        Ok(Err(error)) => {
            tracing::warn!(
                error = %error.message,
                "could not redirect TDLib logs to file; keeping stderr"
            );
        }
        Err(_) => {
            tracing::warn!("TDLib timed out while configuring its log file");
        }
    }

    // Level 1 keeps errors and warnings in the file without flooding it with
    // session/socket details. Failures here must not prevent Telegram startup.
    match tokio::time::timeout(
        Duration::from_secs(4),
        functions::set_log_verbosity_level(1, client_id),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(
                error = %error.message,
                "could not set TDLib log verbosity"
            );
        }
        Err(_) => tracing::warn!("TDLib timed out while setting log verbosity"),
    }

    match tokio::time::timeout(Duration::from_secs(8), configure_if_needed(&state.inner)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Ok(td_error(error)),
        Err(_) => return Ok(td_timeout("TDLib demorou para configurar parametros.")),
    }

    drop(_init_guard);
    tdlib_status(state).await
}

pub async fn tdlib_status(state: &TdlibManager) -> TdlibCommandResult {
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

    match tokio::time::timeout(
        Duration::from_secs(4),
        functions::get_authorization_state(client_id),
    )
    .await
    {
        Err(_) => Ok(td_timeout(
            "TDLib demorou para retornar estado de autenticacao.",
        )),
        Ok(result) => match result {
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
        },
    }
}

pub async fn tdlib_set_phone(state: &TdlibManager, phone_number: String) -> TdlibCommandResult {
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

pub async fn tdlib_check_code(state: &TdlibManager, code: String) -> TdlibCommandResult {
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

pub async fn tdlib_check_password(state: &TdlibManager, password: String) -> TdlibCommandResult {
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

pub async fn tdlib_get_me(state: &TdlibManager) -> Result<TdlibUserInfo, String> {
    let client_id = match ready_client_id(state).await {
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

pub async fn tdlib_get_chats(
    state: &TdlibManager,
    limit: Option<i32>,
) -> Result<TdlibChatsResult, String> {
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

    let limit = limit.unwrap_or(80).clamp(50, 5000);
    // Keep initial rendering responsive: request a bounded, balanced window from
    // both lists instead of resolving up to `limit` chats from each list.
    let main_limit = ((limit * 4) / 5).max(1);
    let archive_limit = (limit - main_limit).max(1);
    let load_limit = limit.min(100);
    let _ = functions::load_chats(Some(ChatList::Main), load_limit, client_id).await;
    let _ = functions::load_chats(Some(ChatList::Archive), load_limit, client_id).await;
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

    let archive_chat_ids = if let Ok(tdlib_rs::enums::Chats::Chats(chats)) =
        functions::get_chats(Some(ChatList::Archive), archive_limit, client_id).await
    {
        chats.chat_ids
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
    for chat_id in chat_ids {
        let chat_id_key = chat_id.to_string();
        if seen_chat_ids.contains_key(&chat_id_key) {
            continue;
        }
        if let Ok(tdlib_rs::enums::Chat::Chat(chat)) = functions::get_chat(chat_id, client_id).await
        {
            seen_chat_ids.insert(chat_id_key, true);
            dialogs.push(tdlib_chat_to_app(chat));
        }
    }
    #[cfg(debug_assertions)]
    let chats_resolved_at = Instant::now();
    #[cfg(debug_assertions)]
    let resolved_chats = dialogs.len();

    let remaining_contact_slots = (limit as usize).saturating_sub(dialogs.len());
    if remaining_contact_slots > 0 {
        if let Ok(tdlib_rs::enums::Users::Users(users)) = functions::get_contacts(client_id).await {
            let mut contact_dialogs = Vec::new();
            // Contacts are a fallback for sparse dialog lists. Resolving every contact
            // one by one was the dominant cold-start cost for larger address books.
            for user_id in users.user_ids.into_iter().take(remaining_contact_slots) {
                let chat_id_key = user_id.to_string();
                if seen_chat_ids.contains_key(&chat_id_key) {
                    continue;
                }
                if let Ok(tdlib_rs::enums::User::User(user)) =
                    functions::get_user(user_id, client_id).await
                {
                    if !user.have_access {
                        continue;
                    }
                    seen_chat_ids.insert(chat_id_key, true);
                    contact_dialogs.push(tdlib_user_to_contact_chat(user));
                }
            }
            contact_dialogs.sort_by_key(|left| left.title.to_lowercase());
            dialogs.extend(contact_dialogs);
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
}

/// Fetches one chat as the UI DTO, including capabilities and topic support.
///
/// Global search can return messages from conversations outside the loaded
/// chat list. Opening one of those results needs the real chat metadata,
/// otherwise the composer would be disabled by the fallback DTO.
pub async fn tdlib_get_chat(state: &TdlibManager, chat_id: i64) -> Result<TdlibChatInfo, String> {
    let client_id = ready_client_id(state)
        .await
        .map_err(|status| status.error.unwrap_or(status.state))?;
    match functions::get_chat(chat_id, client_id).await {
        Ok(tdlib_rs::enums::Chat::Chat(chat)) => Ok(tdlib_chat_to_app(chat)),
        Err(error) => Err(error.message),
    }
}

pub async fn tdlib_read_chat(state: &TdlibManager, chat_id: i64) -> Result<(), String> {
    let client_id = ready_client_id(state)
        .await
        .map_err(|status| status.error.unwrap_or(status.state))?;
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
}

pub async fn tdlib_read_all_chats(state: &TdlibManager) -> Result<(), String> {
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
) -> Result<TdlibMessageInfo, String> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => return Err(status.error.unwrap_or(status.state)),
    };
    let tdlib_id = tdlib_message_id_from_app_id(message_id);
    let raw = match functions::get_message(chat_id, tdlib_id, client_id).await {
        Ok(tdlib_rs::enums::Message::Message(message)) => message,
        Err(_) if tdlib_id != message_id => {
            match functions::get_message(chat_id, message_id, client_id).await {
                Ok(tdlib_rs::enums::Message::Message(message)) => message,
                Err(error) => return Err(error.message),
            }
        }
        Err(error) => return Err(error.message),
    };
    let mut message = tdlib_message_to_app(raw);
    let sender_name = resolve_sender_name(client_id, &message.sender_id).await;
    if !sender_name.trim().is_empty() {
        message.sender_name = sender_name;
    }
    Ok(message)
}

pub async fn tdlib_get_messages(
    state: &TdlibManager,
    chat_id: i64,
    limit: Option<i32>,
    offset_id: Option<i64>,
    offset: Option<i32>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibMessagesResult, String> {
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

    // TDLib escolhe quantas mensagens devolver e pode retornar só uma mesmo
    // quando o limite é 50. Continue pela âncora até preencher a página ou
    // chegar ao fim, preservando a busca contextual com offset negativo.
    for request_index in 0..12 {
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
        if request_index == 11 && added > 0 {
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

    resolve_message_sender_names(client_id, &mut messages).await;
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

    Ok(TdlibMessagesResult {
        success: true,
        messages,
        has_more,
        oldest_message_id,
        error: None,
    })
}

pub async fn tdlib_search_chat_messages(
    state: &TdlibManager,
    chat_id: i64,
    query: String,
    limit: Option<i32>,
    from_message_id: Option<i64>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibSearchMessagesResult, String> {
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
) -> Result<TdlibGlobalSearchResult, String> {
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

pub async fn tdlib_get_chat_capabilities(
    state: &TdlibManager,
    chat_id: i64,
) -> Result<TdlibChatCapabilitiesResult, String> {
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
) -> Result<TdlibSharedMediaResult, String> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibSharedMediaResult {
                success: false,
                media: Vec::new(),
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let requested_limit = limit.unwrap_or(12).clamp(1, 50) as usize;
    let mut from_message_id = 0;
    let mut scanned = 0usize;
    let mut media = Vec::new();

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
            break;
        }
        from_message_id = next_from_message_id;
    }

    Ok(TdlibSharedMediaResult {
        success: true,
        media,
        error: None,
    })
}

pub async fn tdlib_search_user_media(
    state: &TdlibManager,
    chat_id: i64,
    user_id: String,
    limit: Option<i32>,
) -> Result<TdlibSharedMediaResult, String> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibSharedMediaResult {
                success: false,
                media: Vec::new(),
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let requested_limit = limit.unwrap_or(100).clamp(1, 200) as usize;
    let target_user_id = user_id
        .chars()
        .filter(|ch| ch.is_ascii_digit() || *ch == '-')
        .collect::<String>();
    let mut from_message_id = 0;
    let mut scanned = 0usize;
    let mut media = Vec::new();

    while media.len() < requested_limit && scanned < 5000 {
        let history =
            match functions::get_chat_history(chat_id, from_message_id, 0, 100, false, client_id)
                .await
            {
                Ok(tdlib_rs::enums::Messages::Messages(messages)) => messages,
                Err(error) => {
                    return Ok(TdlibSharedMediaResult {
                        success: false,
                        media,
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
            break;
        }
        from_message_id = next_from_message_id;
    }

    Ok(TdlibSharedMediaResult {
        success: true,
        media,
        error: None,
    })
}

pub async fn tdlib_send_message(
    state: &TdlibManager,
    chat_id: i64,
    text: String,
    reply_to_id: Option<i64>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibSendResult, String> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibSendResult {
                success: false,
                message: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Ok(TdlibSendResult {
            success: false,
            message: None,
            error: Some("Mensagem vazia.".to_string()),
        });
    }

    let content = InputMessageContent::InputMessageText(InputMessageText {
        text: plain_formatted_text(trimmed),
        link_preview_options: None,
        clear_draft: true,
    });

    match functions::send_message(
        chat_id,
        message_topic_from_id(topic_id, topic_kind.as_deref()),
        reply_to_from_id(reply_to_id),
        None,
        content,
        client_id,
    )
    .await
    {
        Ok(tdlib_rs::enums::Message::Message(message)) => Ok(TdlibSendResult {
            success: true,
            message: Some(tdlib_message_to_app(message)),
            error: None,
        }),
        Err(error) => Ok(TdlibSendResult {
            success: false,
            message: None,
            error: Some(error.message),
        }),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibActionResult {
    pub success: bool,
    pub error: Option<String>,
}

/// Adds or removes an emoji reaction on a message.
pub async fn tdlib_toggle_message_reaction(
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    emoji: String,
    add: bool,
) -> Result<TdlibActionResult, String> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibActionResult {
                success: false,
                error: status.error.or(Some(status.state)),
            });
        }
    };
    let reaction = ReactionType::Emoji(ReactionTypeEmoji { emoji });
    let message_id = tdlib_message_id_from_app_id(message_id);
    let result = if add {
        functions::add_message_reaction(chat_id, message_id, reaction, false, true, client_id).await
    } else {
        functions::remove_message_reaction(chat_id, message_id, reaction, client_id).await
    };
    Ok(match result {
        Ok(()) => TdlibActionResult {
            success: true,
            error: None,
        },
        Err(error) => TdlibActionResult {
            success: false,
            error: Some(error.message),
        },
    })
}

pub async fn tdlib_send_media(
    state: &TdlibManager,
    chat_id: i64,
    file_path: String,
    caption: Option<String>,
    reply_to_id: Option<i64>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibSendResult, String> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibSendResult {
                success: false,
                message: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    if file_path.trim().is_empty() {
        return Ok(TdlibSendResult {
            success: false,
            message: None,
            error: Some("Arquivo inválido.".to_string()),
        });
    }

    let content = input_content_for_local_media(&file_path, caption.as_deref().unwrap_or(""));
    match functions::send_message(
        chat_id,
        message_topic_from_id(topic_id, topic_kind.as_deref()),
        reply_to_from_id(reply_to_id),
        None,
        content,
        client_id,
    )
    .await
    {
        Ok(tdlib_rs::enums::Message::Message(message)) => Ok(TdlibSendResult {
            success: true,
            message: Some(tdlib_message_to_app(message)),
            error: None,
        }),
        Err(error) => Ok(TdlibSendResult {
            success: false,
            message: None,
            error: Some(error.message),
        }),
    }
}

pub async fn tdlib_forward_message(
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    to_chat_id: i64,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibForwardResult, String> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibForwardResult {
                success: false,
                messages: Vec::new(),
                error: status.error.or(Some(status.state)),
            });
        }
    };

    match functions::forward_messages(
        to_chat_id,
        message_topic_from_id(topic_id, topic_kind.as_deref()),
        chat_id,
        vec![tdlib_message_id_from_app_id(message_id)],
        None,
        false,
        false,
        client_id,
    )
    .await
    {
        Ok(tdlib_rs::enums::Messages::Messages(messages)) => Ok(TdlibForwardResult {
            success: true,
            messages: messages
                .messages
                .into_iter()
                .flatten()
                .map(tdlib_message_to_app)
                .collect(),
            error: None,
        }),
        Err(error) => Ok(TdlibForwardResult {
            success: false,
            messages: Vec::new(),
            error: Some(error.message),
        }),
    }
}

pub async fn tdlib_get_forum_topics(
    state: &TdlibManager,
    chat_id: i64,
    limit: Option<i32>,
) -> Result<TdlibForumTopicsResult, String> {
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

    let is_forum = match functions::get_chat(chat_id, client_id).await {
        Ok(tdlib_rs::enums::Chat::Chat(chat)) => match chat.r#type {
            ChatType::Supergroup(group) => {
                match functions::get_supergroup(group.supergroup_id, client_id).await {
                    Ok(tdlib_rs::enums::Supergroup::Supergroup(info)) => info.is_forum,
                    Err(error) => {
                        return Ok(TdlibForumTopicsResult {
                            success: false,
                            is_forum: false,
                            topics: Vec::new(),
                            error: Some(error.message),
                        });
                    }
                }
            }
            _ => false,
        },
        Err(error) => {
            return Ok(TdlibForumTopicsResult {
                success: false,
                is_forum: false,
                topics: Vec::new(),
                error: Some(error.message),
            });
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
}

pub async fn tdlib_topic_has_new_media(
    state: &TdlibManager,
    chat_id: i64,
    topic_id: i64,
    topic_kind: Option<String>,
    after_message_id: i64,
) -> Result<bool, String> {
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

async fn telegram_media_ensure_cached_inner(
    app: &AppEnv,
    manager: &TdlibManager,
    client_id: i32,
    chat_id: i64,
    message_id: i64,
    priority: Option<String>,
) -> NativeMediaAssetMeta {
    let _ = cleanup_stale_native_partials(app);
    if let Ok(Some(meta)) = complete_native_media_meta(app, chat_id, message_id) {
        return meta;
    }

    let lock = native_media_lock(manager, chat_id, message_id).await;
    let _guard = lock.lock().await;
    let abort_flag = native_media_abort_flag(manager, chat_id, message_id).await;
    abort_flag.store(false, Ordering::SeqCst);

    if let Ok(Some(meta)) = complete_native_media_meta(app, chat_id, message_id) {
        return meta;
    }

    let media = match resolve_message_media(client_id, chat_id, message_id).await {
        Ok(media) => media,
        Err(error) => {
            let mut meta = empty_native_media_meta(chat_id, message_id);
            meta.state = "failed".to_string();
            meta.error = Some(error);
            let _ = write_native_media_meta(app, &meta);
            app.emit("telegram-media-error", &meta);
            return meta;
        }
    };

    let file_name = media.file_name.clone();
    let mime_type = media.mime_type.clone();
    let total_bytes = Some(effective_file_size(&media.file));
    let media_dir = match native_media_dir(app, chat_id, message_id) {
        Ok(dir) => dir,
        Err(error) => {
            let mut meta = empty_native_media_meta(chat_id, message_id);
            meta.state = "failed".to_string();
            meta.error = Some(error);
            app.emit("telegram-media-error", &meta);
            return meta;
        }
    };

    if let Err(error) = std::fs::create_dir_all(&media_dir) {
        let mut meta = empty_native_media_meta(chat_id, message_id);
        meta.state = "failed".to_string();
        meta.error = Some(error.to_string());
        app.emit("telegram-media-error", &meta);
        return meta;
    }

    let destination = media_dir.join(sanitize_filename(&file_name));
    cleanup_incomplete_file(&destination, total_bytes);
    let thumbnail_path =
        download_thumbnail_to_cache(client_id, media.thumbnail.clone(), &media_dir).await;
    let mut meta = NativeMediaAssetMeta {
        success: false,
        state: "partial".to_string(),
        chat_id,
        message_id,
        total_bytes,
        downloaded_bytes: None,
        mime_type: Some(mime_type),
        file_name: Some(file_name.clone()),
        native_file_path: Some(destination.to_string_lossy().to_string()),
        thumbnail_path,
        completed_at: None,
        updated_at: now_millis(),
        error: None,
    };
    let _ = write_native_media_meta(app, &meta);
    emit_native_media_progress(app, chat_id, message_id, "downloading", 0, total_bytes);

    let priority_label = priority.unwrap_or_else(|| "user".to_string());
    let download_result = download_media_file_to_path(
        client_id,
        media.file,
        &destination,
        |percent, stage| {
            let stage = if stage == "saving" {
                "saving"
            } else if priority_label == "background" {
                "background"
            } else {
                "downloading"
            };
            emit_native_media_progress(app, chat_id, message_id, stage, percent, total_bytes);
        },
        || abort_flag.load(Ordering::SeqCst) || manager.download_aborted.load(Ordering::SeqCst),
    )
    .await;

    match download_result {
        Ok((size, _skipped)) => {
            meta.success = true;
            meta.state = "complete".to_string();
            meta.total_bytes = Some(size);
            meta.downloaded_bytes = Some(size);
            meta.completed_at = Some(now_millis());
            meta.updated_at = now_millis();
            meta.error = None;
            let _ = write_native_media_meta(app, &meta);
            emit_native_media_progress(app, chat_id, message_id, "ready", 100, Some(size));
            app.emit("telegram-media-ready", &meta);
            meta
        }
        Err(error) => {
            meta.success = false;
            if error == "STOP_ABORTED" {
                meta.state = "partial".to_string();
                meta.error = None;
            } else {
                meta.state = "failed".to_string();
                meta.error = Some(error);
            }
            meta.updated_at = now_millis();
            let _ = write_native_media_meta(app, &meta);
            if meta.state == "failed" {
                app.emit("telegram-media-error", &meta);
            }
            meta
        }
    }
}

pub async fn telegram_media_get_meta(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
) -> Result<NativeMediaAssetMeta, String> {
    read_native_media_meta(app, chat_id, message_id)
}

pub async fn telegram_media_ensure_cached(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    priority: Option<String>,
) -> Result<NativeMediaAssetMeta, String> {
    state.download_aborted.store(false, Ordering::SeqCst);
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            let mut meta = empty_native_media_meta(chat_id, message_id);
            meta.state = "failed".to_string();
            meta.error = status.error.or(Some(status.state));
            return Ok(meta);
        }
    };

    Ok(
        telegram_media_ensure_cached_inner(app, state, client_id, chat_id, message_id, priority)
            .await,
    )
}

pub async fn telegram_media_prepare_playback(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    mode: Option<String>,
) -> Result<NativePlaybackSource, String> {
    if let Ok(Some(meta)) = complete_native_media_meta(app, chat_id, message_id) {
        let (playback_path, playback_mime_type) = ensure_native_playback_file(app, &meta)?;
        return Ok(NativePlaybackSource {
            success: true,
            kind: "file".to_string(),
            url: Some(media_http::playback_url(chat_id, message_id)),
            file_path: Some(playback_path),
            mime_type: playback_mime_type.or(meta.mime_type.clone()),
            cache_state: meta.state,
            total_bytes: meta.total_bytes,
            downloaded_bytes: meta.downloaded_bytes,
            file_name: meta.file_name,
            error: None,
        });
    }

    let priority = mode.map(|value| {
        if value == "background" {
            value
        } else {
            "user".to_string()
        }
    });
    let meta = telegram_media_ensure_cached(app, state, chat_id, message_id, priority).await?;
    let playback = if meta.success {
        ensure_native_playback_file(app, &meta).ok()
    } else {
        None
    };
    let playback_path = playback
        .as_ref()
        .map(|(path, _)| path.clone())
        .or_else(|| meta.native_file_path.clone());
    let playback_mime_type = playback
        .and_then(|(_, mime_type)| mime_type)
        .or(meta.mime_type.clone());
    Ok(NativePlaybackSource {
        success: meta.success,
        kind: if meta.success { "file" } else { "fallback" }.to_string(),
        url: if meta.success {
            Some(media_http::playback_url(chat_id, message_id))
        } else {
            playback_path.clone()
        },
        file_path: playback_path,
        mime_type: playback_mime_type,
        cache_state: meta.state,
        total_bytes: meta.total_bytes,
        downloaded_bytes: meta.downloaded_bytes,
        file_name: meta.file_name,
        error: meta.error,
    })
}

pub async fn telegram_media_save(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    destination_path: Option<String>,
) -> Result<NativeSavedMedia, String> {
    let save_abort_flag = native_media_abort_flag(state, chat_id, message_id).await;
    let meta =
        telegram_media_ensure_cached(app, state, chat_id, message_id, Some("user".to_string()))
            .await?;

    if !meta.success {
        let canceled = meta.state == "partial" && meta.error.is_none();
        return Ok(NativeSavedMedia {
            success: false,
            file_path: None,
            file_name: meta.file_name,
            size: meta.total_bytes,
            skipped: false,
            error: if canceled {
                Some("STOP_ABORTED".to_string())
            } else {
                meta.error
            },
        });
    }

    let Some(source_path) = meta.native_file_path.clone() else {
        return Ok(NativeSavedMedia {
            success: false,
            file_path: None,
            file_name: meta.file_name,
            size: meta.total_bytes,
            skipped: false,
            error: Some("Midia nativa sem caminho de arquivo.".to_string()),
        });
    };

    let file_name = meta
        .file_name
        .clone()
        .unwrap_or_else(|| sanitize_filename(&format!("media_{message_id}")));
    let destination = match destination_path {
        Some(path) if !path.trim().is_empty() => PathBuf::from(path),
        _ => app.paths().download_dir()?.join(&file_name),
    };

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let skipped = destination.exists();
    if !skipped {
        save_abort_flag.store(false, Ordering::SeqCst);
        let tmp_path = partial_file_path(&destination);
        let _ = std::fs::remove_file(&tmp_path);
        let total_bytes = meta.total_bytes.or_else(|| {
            std::fs::metadata(&source_path)
                .ok()
                .map(|metadata| metadata.len() as i64)
        });
        let mut on_progress = |percent, stage| {
            emit_native_media_progress(app, chat_id, message_id, stage, percent, total_bytes);
        };
        copy_file_with_progress(
            Path::new(&source_path),
            &tmp_path,
            total_bytes.unwrap_or_default(),
            &mut on_progress,
            &|| save_abort_flag.load(Ordering::SeqCst),
        )
        .await
        .inspect_err(|_error| {
            let _ = std::fs::remove_file(&tmp_path);
        })?;
        std::fs::rename(&tmp_path, &destination).map_err(|error| {
            let _ = std::fs::remove_file(&tmp_path);
            error.to_string()
        })?;
    }

    Ok(NativeSavedMedia {
        success: true,
        file_path: Some(destination.to_string_lossy().to_string()),
        file_name: Some(file_name),
        size: meta.total_bytes,
        skipped,
        error: None,
    })
}

pub async fn telegram_media_cancel(
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> Result<bool, String> {
    let abort_flag = native_media_abort_flag(state, chat_id, message_id).await;
    abort_flag.store(true, Ordering::SeqCst);
    Ok(true)
}

pub async fn telegram_media_cache_stats(app: &AppEnv) -> Result<NativeMediaCacheStats, String> {
    Ok(native_media_cache_stats(app))
}

pub async fn telegram_media_clear_cache(app: &AppEnv) -> Result<NativeMediaCacheStats, String> {
    let root = native_media_root_dir(app)?;
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
    }
    Ok(native_media_cache_stats(app))
}

pub async fn telegram_media_evict_cache(
    app: &AppEnv,
    max_cache_size: i64,
) -> Result<NativeMediaCacheStats, String> {
    evict_native_media_cache(app, max_cache_size)
}

pub async fn tdlib_download_message_media(
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    folder_path: String,
) -> Result<TdlibDownloadResult, String> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibDownloadResult {
                success: false,
                skipped: false,
                file_path: None,
                file_name: None,
                size: 0,
                mime_type: None,
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
                    mime_type: None,
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
            mime_type: None,
            error: Some("Mensagem sem midia baixavel pelo TDLib.".to_string()),
        });
    };
    let media_file = media.file;
    let file_name = media.file_name;
    let mime_type = media.mime_type;

    let destination = Path::new(&folder_path).join(&file_name);
    let (size, skipped) =
        match download_media_file_to_path(client_id, media_file, &destination, |_, _| {}, || false)
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
                    mime_type: Some(mime_type),
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
        mime_type: Some(mime_type),
        error: None,
    })
}

pub async fn tdlib_cache_message_media(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> Result<TdlibDownloadResult, String> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibDownloadResult {
                success: false,
                skipped: false,
                file_path: None,
                file_name: None,
                size: 0,
                mime_type: None,
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
                    mime_type: None,
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
            mime_type: None,
            error: Some("Mensagem sem midia baixavel pelo TDLib.".to_string()),
        });
    };

    let media_file = media.file;
    let file_name = media.file_name;
    let mime_type = media.mime_type;
    let app_cache_dir = app.paths().cache_dir();
    let destination_dir = app_cache_dir
        .join("telegram-media-cache")
        .join(chat_id.to_string());
    let destination = destination_dir.join(sanitize_filename(&format!("{message_id}_{file_name}")));

    let (size, skipped) =
        match download_media_file_to_path(client_id, media_file, &destination, |_, _| {}, || false)
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
                    mime_type: Some(mime_type),
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
        mime_type: Some(mime_type),
        error: None,
    })
}

pub async fn tdlib_download_message_thumbnail(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> Result<TdlibThumbnailResult, String> {
    if let Ok(meta) = read_native_media_meta(app, chat_id, message_id) {
        if let Some(path) = meta.thumbnail_path {
            if Path::new(&path).exists() {
                return Ok(TdlibThumbnailResult {
                    success: true,
                    file_path: Some(path),
                    error: None,
                });
            }
        }
    }

    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibThumbnailResult {
                success: false,
                file_path: None,
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
                return Ok(TdlibThumbnailResult {
                    success: false,
                    file_path: None,
                    error: Some(error.message),
                });
            }
        },
    };

    let local_video_path = local_video_path(&message.content);
    let minithumbnail = message_minithumbnail(&message.content).map(str::to_string);
    let thumbnail = message_thumbnail_file(message.content);

    let media_dir = match native_media_dir(app, chat_id, message_id) {
        Ok(dir) => dir,
        Err(error) => {
            return Ok(TdlibThumbnailResult {
                success: false,
                file_path: None,
                error: Some(error),
            });
        }
    };

    let thumbnail_path = match thumbnail {
        Some(thumbnail) => {
            download_thumbnail_to_cache(client_id, Some(thumbnail), &media_dir).await
        }
        None => {
            // ffmpeg blocks; keep it off the async worker that serves TDLib.
            let generated = match local_video_path {
                Some(path) => {
                    let thumbnail_dir = media_dir.clone();
                    tokio::task::spawn_blocking(move || {
                        generate_video_thumbnail_to_cache(
                            &path,
                            &thumbnail_dir.join("thumbnail.jpg"),
                        )
                    })
                    .await
                    .ok()
                    .flatten()
                }
                None => None,
            };
            generated
                .or_else(|| minithumbnail.and_then(|data| write_minithumbnail(&data, &media_dir)))
        }
    };

    match thumbnail_path {
        Some(path) => {
            let mut meta = read_native_media_meta(app, chat_id, message_id)
                .unwrap_or_else(|_| empty_native_media_meta(chat_id, message_id));
            meta.thumbnail_path = Some(path.clone());
            meta.updated_at = now_millis();
            let _ = write_native_media_meta(app, &meta);
            Ok(TdlibThumbnailResult {
                success: true,
                file_path: Some(path),
                error: None,
            })
        }
        None => {
            log_thumbnail_failure(chat_id, message_id);
            Ok(TdlibThumbnailResult {
                success: false,
                file_path: None,
                error: Some("TDLib nao disponibilizou uma thumbnail para esta mídia.".to_string()),
            })
        }
    }
}

pub async fn tdlib_download_chat_avatar(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
) -> Result<TdlibAvatarResult, String> {
    if let Some(path) = cached_chat_avatar(app, chat_id) {
        return Ok(TdlibAvatarResult {
            success: true,
            file_path: Some(path.to_string_lossy().to_string()),
            error: None,
        });
    }

    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibAvatarResult {
                success: false,
                file_path: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    let chat = match functions::get_chat(chat_id, client_id).await {
        Ok(tdlib_rs::enums::Chat::Chat(chat)) => chat,
        Err(error) => {
            return Ok(TdlibAvatarResult {
                success: false,
                file_path: None,
                error: Some(error.message),
            });
        }
    };

    let Some(photo) = chat.photo else {
        return Ok(TdlibAvatarResult {
            success: false,
            file_path: None,
            error: Some("Chat sem avatar disponivel pelo TDLib.".to_string()),
        });
    };

    let small = photo.small;
    let big = photo.big;
    let avatar_path = match download_avatar_to_cache(app, client_id, chat_id, Some(small)).await {
        Some(path) => Some(path),
        None => download_avatar_to_cache(app, client_id, chat_id, Some(big)).await,
    };

    match avatar_path {
        Some(path) => Ok(TdlibAvatarResult {
            success: true,
            file_path: Some(path),
            error: None,
        }),
        None => {
            tracing::debug!(chat_id, "Telegram avatar unavailable");
            Ok(TdlibAvatarResult {
                success: false,
                file_path: None,
                error: Some("TDLib nao concluiu o download do avatar.".to_string()),
            })
        }
    }
}

pub async fn tdlib_stop_download(state: &TdlibManager) -> Result<(), String> {
    state.download_aborted.store(true, Ordering::Release);
    Ok(())
}

pub async fn tdlib_start_mass_download(
    app: &AppEnv,
    state: &TdlibManager,
    request: TdlibMassDownloadRequest,
) -> Result<TdlibMassDownloadResult, String> {
    let client_id = match ready_client_id(state).await {
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
                tracking_error: None,
            });
        }
    };

    state.download_aborted.store(false, Ordering::Release);

    let mut from_message_id = 0;
    let mut downloaded_count = 0usize;
    let mut skipped_count = 0usize;
    let mut failed_count = 0usize;
    let mut items: Vec<TdlibDownloadItem> = Vec::new();
    let mut pending_downloads: Vec<PendingMassDownload> = Vec::new();
    let mut scanned_messages: Vec<tdlib_rs::types::Message> = Vec::new();

    emit_mass_progress(
        app,
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
            if request.topic_kind.as_deref() == Some("thread") {
                functions::get_message_thread_history(
                    request.chat_id,
                    tdlib_message_id_from_app_id(topic_id),
                    from_message_id,
                    0,
                    100,
                    client_id,
                )
                .await
            } else {
                let forum_topic_id = match i32::try_from(topic_id) {
                    Ok(topic_id) => topic_id,
                    Err(_) => {
                        return Ok(TdlibMassDownloadResult {
                            success: false,
                            downloaded_count,
                            skipped_count,
                            failed_count,
                            total: items.len(),
                            aborted: false,
                            error: Some("ID de tópico inválido".to_string()),
                            tracking_error: None,
                        });
                    }
                };
                functions::get_forum_topic_history(
                    request.chat_id,
                    forum_topic_id,
                    from_message_id,
                    0,
                    100,
                    client_id,
                )
                .await
            }
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
                    total: items.len(),
                    aborted: false,
                    error: Some(error.message),
                    tracking_error: None,
                });
            }
        };

        if history.messages.is_empty() {
            break;
        }

        let mut next_from_message_id = from_message_id;

        for message in history.messages.into_iter().flatten() {
            next_from_message_id = message.id;
            scanned_messages.push(message);
        }

        if next_from_message_id == 0 || next_from_message_id == from_message_id {
            break;
        }
        from_message_id = next_from_message_id;
    }

    let last_scanned_message_id = scanned_messages
        .iter()
        .map(|message| app_message_id_from_tdlib(message.id))
        .max()
        .unwrap_or(0);
    scanned_messages.reverse();
    let album_split_mode = request.album_split_mode.as_deref().unwrap_or("separator");
    let mut album_folder_names = HashMap::<i64, String>::new();
    let mut last_separator_name: Option<String> = None;

    for message in &scanned_messages {
        let text = content_text(&message.content);
        let (has_media, _, _, _, _) = content_media_flags(&message.content);
        if has_media && request.split_by_album && message.media_album_id != 0 {
            match album_split_mode {
                "comment" => {
                    if !text.trim().is_empty() {
                        album_folder_names
                            .entry(message.media_album_id)
                            .or_insert_with(|| sanitize_folder_name(text.trim()));
                    }
                }
                _ => {
                    if let Some(separator_name) = &last_separator_name {
                        album_folder_names
                            .entry(message.media_album_id)
                            .or_insert_with(|| separator_name.clone());
                    }
                }
            }
        } else if !has_media && is_likely_album_separator_text(&text) {
            last_separator_name = Some(sanitize_folder_name(text.trim()));
        }
    }

    let mut sender_folder_names = HashMap::<String, String>::new();
    for message in scanned_messages {
        let sender_id = message.sender_id.clone();
        let media_album_id = message.media_album_id;
        let Some(media) = message_media_file(message.content, message.id) else {
            continue;
        };
        let media_file = media.file;
        let file_name = media.file_name;

        let mut folder = Path::new(&request.folder_path).to_path_buf();
        if request.split_by_user {
            let sender_folder =
                resolve_sender_folder_name(client_id, &sender_id, &mut sender_folder_names).await;
            folder = folder.join(sender_folder);
        }
        if request.split_by_album && media_album_id != 0 {
            let album_folder = album_folder_names
                .get(&media_album_id)
                .cloned()
                .unwrap_or_else(|| sanitize_folder_name(&format!("Album_{media_album_id}")));
            folder = folder.join(album_folder);
        }

        let destination = folder.join(&file_name);
        let item_index = items.len();
        items.push(TdlibDownloadItem {
            name: file_name.clone(),
            message_id: message.id,
            status: "pending".to_string(),
            progress: 0,
            size: effective_file_size(&media_file),
            file_path: Some(destination.to_string_lossy().to_string()),
            thumbnail_path: None,
        });
        pending_downloads.push(PendingMassDownload {
            file: media_file,
            thumbnail: media.thumbnail,
            destination,
            item_index,
        });
    }

    let total = items.len();
    if !state.download_aborted.load(Ordering::Acquire) {
        emit_mass_progress(
            app,
            request.chat_id,
            total,
            0.0,
            format!("{total} midias encontradas. Iniciando downloads..."),
            false,
            &items,
        );
    }

    for pending in pending_downloads {
        if state.download_aborted.load(Ordering::Acquire) {
            break;
        }

        let item_index = pending.item_index;
        let file_name = items[item_index].name.clone();
        if pending.destination.exists() {
            skipped_count += 1;
            items[item_index].status = "skipped".to_string();
            items[item_index].progress = 100;
            emit_mass_progress(
                app,
                request.chat_id,
                total,
                (downloaded_count + skipped_count + failed_count) as f64,
                format!("Ignorado (ja existe): {file_name}"),
                false,
                &items,
            );
            continue;
        }

        items[item_index].status = "downloading".to_string();
        items[item_index].thumbnail_path =
            download_thumbnail_path(client_id, pending.thumbnail).await;
        emit_mass_progress(
            app,
            request.chat_id,
            total,
            (downloaded_count + skipped_count + failed_count) as f64,
            file_name.clone(),
            false,
            &items,
        );

        let abort_flag = &state.download_aborted;
        let download_result = download_media_file_to_path(
            client_id,
            pending.file,
            &pending.destination,
            |percent, stage| {
                items[item_index].progress = percent;
                let status = if stage == "saving" {
                    format!("Salvando {file_name} ({percent}%)")
                } else {
                    format!("{file_name} ({percent}%)")
                };
                emit_mass_progress(
                    app,
                    request.chat_id,
                    total,
                    (downloaded_count + skipped_count + failed_count) as f64
                        + (f64::from(percent) / 100.0),
                    status,
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
                    app,
                    request.chat_id,
                    total,
                    (downloaded_count + skipped_count + failed_count) as f64,
                    format!("{file_name}: {error}"),
                    false,
                    &items,
                );
                continue;
            }
        }

        emit_mass_progress(
            app,
            request.chat_id,
            total,
            (downloaded_count + skipped_count + failed_count) as f64,
            file_name,
            false,
            &items,
        );
    }

    let aborted = state.download_aborted.load(Ordering::Acquire);
    let current_file = if aborted {
        format!("Parado: {downloaded_count} baixados, {skipped_count} ignorados")
    } else {
        format!("Concluido: {downloaded_count} baixados, {skipped_count} ignorados, {failed_count} falharam")
    };

    emit_mass_progress(
        app,
        request.chat_id,
        total,
        (downloaded_count + skipped_count + failed_count) as f64,
        current_file,
        false,
        &items,
    );

    let tracking_error = if !aborted && failed_count == 0 {
        request.topic_id.and_then(|topic_id| {
            plasma_store::record_topic_mass_download(
                app,
                request.chat_id,
                topic_id,
                request.topic_kind.as_deref().unwrap_or("forum"),
                last_scanned_message_id,
            )
            .err()
            .map(|error| error.to_string())
        })
    } else {
        None
    };

    Ok(TdlibMassDownloadResult {
        success: true,
        downloaded_count,
        skipped_count,
        failed_count,
        total,
        aborted,
        error: None,
        tracking_error,
    })
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
    fn quicktime_documents_are_detected_as_video() {
        assert!(document_is_video("video/quicktime", "clip.mov"));
        assert!(document_is_video(
            "video/quicktime; codecs=h264",
            "clip.mov"
        ));
        assert!(document_is_video("application/octet-stream", "clip.MOV"));
        assert!(document_is_video("application/octet-stream", "clip.qt"));
        assert!(!document_is_video("application/pdf", "document.pdf"));
        assert!(!document_is_video("audio/webm", "clip.webm"));
        assert!(!document_is_video("application/octet-stream", "notes.txt"));
    }

    #[test]
    fn quicktime_playback_uses_mov_extension_and_mime() {
        assert_eq!(
            mime_extension("video/quicktime; codecs=h264", ".bin"),
            ".mov"
        );
        assert_eq!(
            playback_extension(Some("video/quicktime"), Some("clip.mov")),
            ".mov"
        );
        assert_eq!(
            normalize_playback_mime_type(None, Some("clip.MOV")),
            Some("video/quicktime".to_string())
        );
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

    #[test]
    fn quicktime_media_prepares_a_playback_file() {
        let root = std::env::temp_dir().join(format!("plasma-mov-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = plasma_core::AppPaths::new(
            root.join("data"),
            root.join("local"),
            root.join("cache"),
            None,
        )
        .unwrap();
        let app = plasma_core::AppEnv::headless(paths);
        let chat_id = 1;
        let message_id = 2;
        let media_dir = native_media_dir(&app, chat_id, message_id).unwrap();
        std::fs::create_dir_all(&media_dir).unwrap();
        let source = media_dir.join("clip.mov");
        let bytes = b"fake quicktime payload";
        std::fs::write(&source, bytes).unwrap();
        let meta = NativeMediaAssetMeta {
            success: true,
            state: "complete".to_string(),
            chat_id,
            message_id,
            total_bytes: Some(bytes.len() as i64),
            downloaded_bytes: Some(bytes.len() as i64),
            mime_type: Some("video/quicktime".to_string()),
            file_name: Some("clip.mov".to_string()),
            native_file_path: Some(source.to_string_lossy().to_string()),
            thumbnail_path: None,
            completed_at: None,
            updated_at: 0,
            error: None,
        };

        let (path, mime_type) = ensure_native_playback_file(&app, &meta).unwrap();
        assert_eq!(mime_type.as_deref(), Some("video/quicktime"));
        assert!(path.ends_with("playback.mov"));
        assert_eq!(std::fs::read(path).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(&root);
    }
}
