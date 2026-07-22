use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{
    http, AppHandle, Emitter, Manager, Runtime, State, UriSchemeContext, UriSchemeResponder,
};
use tdlib_rs::{
    enums::{
        AuthorizationState, ChatList, ChatType, InputFile, InputMessageContent,
        InputMessageReplyTo, MessageContent, MessageReplyTo, MessageSender, MessageTopic, Update,
    },
    functions,
    types::{
        File, FormattedText, InputFileLocal, InputMessageDocument, InputMessagePhoto,
        InputMessageReplyToMessage, InputMessageText, InputMessageVideo, MessageTopicForum, Photo,
    },
};
use tokio::sync::Mutex;

const STALE_NATIVE_PARTIAL_AGE: Duration = Duration::from_secs(24 * 60 * 60);

mod media_http;
pub use media_http::start_server as start_plasma_media_http_server;

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
    media_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    media_abort_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
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
#[serde(rename_all = "camelCase")]
pub struct TdlibDownloadResult {
    success: bool,
    skipped: bool,
    file_path: Option<String>,
    file_name: Option<String>,
    size: i64,
    mime_type: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMediaAssetMeta {
    success: bool,
    state: String,
    chat_id: i64,
    message_id: i64,
    total_bytes: Option<i64>,
    downloaded_bytes: Option<i64>,
    mime_type: Option<String>,
    file_name: Option<String>,
    native_file_path: Option<String>,
    thumbnail_path: Option<String>,
    completed_at: Option<i64>,
    updated_at: i64,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePlaybackSource {
    success: bool,
    kind: String,
    url: Option<String>,
    file_path: Option<String>,
    mime_type: Option<String>,
    cache_state: String,
    total_bytes: Option<i64>,
    downloaded_bytes: Option<i64>,
    file_name: Option<String>,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSavedMedia {
    success: bool,
    file_path: Option<String>,
    file_name: Option<String>,
    size: Option<i64>,
    skipped: bool,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMediaCacheStats {
    success: bool,
    total_size: i64,
    media_count: usize,
    error: Option<String>,
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
    success: bool,
    file_path: Option<String>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibAvatarResult {
    success: bool,
    file_path: Option<String>,
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
pub struct TdlibSharedMediaItem {
    id: i64,
    is_video: bool,
    media_size: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSharedMediaResult {
    success: bool,
    media: Vec<TdlibSharedMediaItem>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSendResult {
    success: bool,
    message: Option<TdlibMessageInfo>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibForwardResult {
    success: bool,
    messages: Vec<TdlibMessageInfo>,
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
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "application/octet-stream");
    if let Some(mime_type) = mime_type {
        return Some(mime_type.to_string());
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

fn ensure_native_playback_file<R: Runtime>(
    app: &AppHandle<R>,
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

fn media_protocol_response(
    status: http::StatusCode,
    body: impl Into<Cow<'static, [u8]>>,
    content_type: Option<&str>,
) -> http::Response<Cow<'static, [u8]>> {
    let mut builder = http::Response::builder()
        .status(status)
        .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
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

fn media_protocol_error(
    status: http::StatusCode,
    message: &str,
) -> http::Response<Cow<'static, [u8]>> {
    media_protocol_response(
        status,
        Cow::Owned(message.as_bytes().to_vec()),
        Some("text/plain; charset=utf-8"),
    )
}

fn get_plasma_media_response<R: Runtime>(
    app: &AppHandle<R>,
    request: http::Request<Vec<u8>>,
) -> Result<http::Response<Cow<'static, [u8]>>, String> {
    println!(
        "[plasma-media] request method={} uri={} range={:?}",
        request.method(),
        request.uri(),
        request
            .headers()
            .get(http::header::RANGE)
            .and_then(|value| value.to_str().ok())
    );
    let Some((chat_id, message_id)) = parse_plasma_media_path(request.uri().path()) else {
        println!("[plasma-media] invalid path={}", request.uri().path());
        return Ok(media_protocol_error(
            http::StatusCode::BAD_REQUEST,
            "URL de mídia inválida.",
        ));
    };

    let Some(meta) = complete_native_media_meta(app, chat_id, message_id)? else {
        println!(
            "[plasma-media] missing complete cache chat_id={} message_id={}",
            chat_id, message_id
        );
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
    println!(
        "[plasma-media] resolved chat_id={} message_id={} path={} len={} content_type={} meta_mime={:?} file_name={:?}",
        chat_id,
        message_id,
        playback_path,
        len,
        content_type,
        meta.mime_type,
        meta.file_name
    );

    if request.method() == http::Method::HEAD {
        println!(
            "[plasma-media] response HEAD chat_id={} message_id={} status=200 len={}",
            chat_id, message_id, len
        );
        return http::Response::builder()
            .status(http::StatusCode::OK)
            .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
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
            println!(
                "[plasma-media] invalid range chat_id={} message_id={} range={} len={}",
                chat_id, message_id, range_header, len
            );
            return http::Response::builder()
                .status(http::StatusCode::RANGE_NOT_SATISFIABLE)
                .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
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
        println!(
            "[plasma-media] response RANGE chat_id={} message_id={} status=206 range={}-{} len={} read={} content_type={}",
            chat_id,
            message_id,
            start,
            end,
            len,
            buffer.len(),
            content_type
        );

        return http::Response::builder()
            .status(http::StatusCode::PARTIAL_CONTENT)
            .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
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
    println!(
        "[plasma-media] response INITIAL chat_id={} message_id={} status={} len={} read={} content_type={}",
        chat_id,
        message_id,
        if read as u64 == len { 200 } else { 206 },
        len,
        buffer.len(),
        content_type
    );
    http::Response::builder()
        .status(if read as u64 == len {
            http::StatusCode::OK
        } else {
            http::StatusCode::PARTIAL_CONTENT
        })
        .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
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

pub fn plasma_media_protocol<R: Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: http::Request<Vec<u8>>,
    responder: UriSchemeResponder,
) {
    let app = ctx.app_handle().clone();
    std::thread::spawn(move || {
        let response = get_plasma_media_response(&app, request).unwrap_or_else(|error| {
            println!("[plasma-media] internal error={}", error);
            media_protocol_error(http::StatusCode::INTERNAL_SERVER_ERROR, &error)
        });
        responder.respond(response);
    });
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

fn native_media_dir<R: Runtime>(
    app: &AppHandle<R>,
    chat_id: i64,
    message_id: i64,
) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("plasma-media")
        .join("telegram")
        .join(chat_id.to_string())
        .join(message_id.to_string()))
}

fn native_media_root_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("plasma-media")
        .join("telegram"))
}

fn native_media_meta_path(
    app: &AppHandle<impl Runtime>,
    chat_id: i64,
    message_id: i64,
) -> Result<PathBuf, String> {
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
    app: &AppHandle<impl Runtime>,
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

fn write_native_media_meta(
    app: &AppHandle<impl Runtime>,
    meta: &NativeMediaAssetMeta,
) -> Result<(), String> {
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
    app: &AppHandle<impl Runtime>,
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

fn cleanup_stale_native_partials(app: &AppHandle) -> usize {
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
    app: &AppHandle,
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

fn native_media_cache_stats(app: &AppHandle) -> NativeMediaCacheStats {
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
    app: &AppHandle,
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

fn message_topic_from_id(topic_id: Option<i32>) -> Option<MessageTopic> {
    topic_id.map(|forum_topic_id| MessageTopic::Forum(MessageTopicForum { forum_topic_id }))
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

    if let Err(error) = copy_file_with_progress(
        Path::new(&downloaded_file.local.path),
        &partial_destination,
        effective_file_size(&downloaded_file),
        &mut on_progress,
        &should_abort,
    ) {
        let _ = std::fs::remove_file(&partial_destination);
        return Err(error);
    }
    std::fs::rename(&partial_destination, destination).map_err(|error| {
        let _ = std::fs::remove_file(&partial_destination);
        error.to_string()
    })?;

    Ok((effective_file_size(&downloaded_file), false))
}

fn copy_file_with_progress(
    source: &Path,
    destination: &Path,
    total_size: i64,
    on_progress: &mut impl FnMut(u8, &'static str),
    should_abort: &impl Fn() -> bool,
) -> Result<(), String> {
    on_progress(96, "saving");
    let mut input = std::fs::File::open(source).map_err(|error| error.to_string())?;
    let mut output = std::fs::File::create(destination).map_err(|error| error.to_string())?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut copied = 0_i64;
    let mut last_percent = 96_u8;

    loop {
        if should_abort() {
            return Err("STOP_ABORTED".to_string());
        }
        let read = input.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
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

    output.flush().map_err(|error| error.to_string())?;
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

async fn download_avatar_to_cache(
    app: &AppHandle,
    client_id: i32,
    chat_id: i64,
    photo: Option<File>,
) -> Option<String> {
    let source_path = download_thumbnail_path(client_id, photo).await?;
    if !Path::new(&source_path).exists() {
        return None;
    }

    let avatar_dir = app
        .path()
        .app_cache_dir()
        .ok()?
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
    app: &AppHandle,
    chat_id: i64,
    message_id: i64,
    stage: &str,
    progress: u8,
    total_bytes: Option<i64>,
) {
    let downloaded_bytes = total_bytes
        .map(|total| ((total.max(0) as f64) * (f64::from(progress) / 100.0)).round() as i64);
    let _ = app.emit(
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

    let mut sender_names = HashMap::<String, String>::new();
    for message in &mut messages {
        if message.out {
            continue;
        }
        if let Some(sender_name) = sender_names.get(&message.sender_id) {
            message.sender_name = sender_name.clone();
            continue;
        }

        let sender_name = match message.sender_id.parse::<i64>() {
            Ok(sender_id) if sender_id > 0 => match functions::get_user(sender_id, client_id).await
            {
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
        };
        sender_names.insert(message.sender_id.clone(), sender_name.clone());
        message.sender_name = sender_name;
    }
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
pub async fn tdlib_get_shared_media(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    limit: Option<i32>,
) -> Result<TdlibSharedMediaResult, String> {
    let client_id = match ready_client_id(&state).await {
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
                    is_video,
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

#[tauri::command]
pub async fn tdlib_search_user_media(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    user_id: String,
    limit: Option<i32>,
) -> Result<TdlibSharedMediaResult, String> {
    let client_id = match ready_client_id(&state).await {
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
                    is_video,
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

#[tauri::command]
pub async fn tdlib_send_message(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    text: String,
    reply_to_id: Option<i64>,
    topic_id: Option<i32>,
) -> Result<TdlibSendResult, String> {
    let client_id = match ready_client_id(&state).await {
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
        message_topic_from_id(topic_id),
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

#[tauri::command]
pub async fn tdlib_send_media(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    file_path: String,
    caption: Option<String>,
    reply_to_id: Option<i64>,
    topic_id: Option<i32>,
) -> Result<TdlibSendResult, String> {
    let client_id = match ready_client_id(&state).await {
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
        message_topic_from_id(topic_id),
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

#[tauri::command]
pub async fn tdlib_forward_message(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    to_chat_id: i64,
    topic_id: Option<i32>,
) -> Result<TdlibForwardResult, String> {
    let client_id = match ready_client_id(&state).await {
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
        message_topic_from_id(topic_id),
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

async fn telegram_media_ensure_cached_inner(
    app: AppHandle,
    manager: &TdlibManager,
    client_id: i32,
    chat_id: i64,
    message_id: i64,
    priority: Option<String>,
) -> NativeMediaAssetMeta {
    let _ = cleanup_stale_native_partials(&app);
    if let Ok(Some(meta)) = complete_native_media_meta(&app, chat_id, message_id) {
        return meta;
    }

    let lock = native_media_lock(manager, chat_id, message_id).await;
    let _guard = lock.lock().await;
    let abort_flag = native_media_abort_flag(manager, chat_id, message_id).await;
    abort_flag.store(false, Ordering::SeqCst);

    if let Ok(Some(meta)) = complete_native_media_meta(&app, chat_id, message_id) {
        return meta;
    }

    let media = match resolve_message_media(client_id, chat_id, message_id).await {
        Ok(media) => media,
        Err(error) => {
            let mut meta = empty_native_media_meta(chat_id, message_id);
            meta.state = "failed".to_string();
            meta.error = Some(error);
            let _ = write_native_media_meta(&app, &meta);
            let _ = app.emit("telegram-media-error", &meta);
            return meta;
        }
    };

    let file_name = media.file_name.clone();
    let mime_type = media.mime_type.clone();
    let total_bytes = Some(effective_file_size(&media.file));
    let media_dir = match native_media_dir(&app, chat_id, message_id) {
        Ok(dir) => dir,
        Err(error) => {
            let mut meta = empty_native_media_meta(chat_id, message_id);
            meta.state = "failed".to_string();
            meta.error = Some(error);
            let _ = app.emit("telegram-media-error", &meta);
            return meta;
        }
    };

    if let Err(error) = std::fs::create_dir_all(&media_dir) {
        let mut meta = empty_native_media_meta(chat_id, message_id);
        meta.state = "failed".to_string();
        meta.error = Some(error.to_string());
        let _ = app.emit("telegram-media-error", &meta);
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
    let _ = write_native_media_meta(&app, &meta);
    emit_native_media_progress(&app, chat_id, message_id, "downloading", 0, total_bytes);

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
            emit_native_media_progress(&app, chat_id, message_id, stage, percent, total_bytes);
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
            let _ = write_native_media_meta(&app, &meta);
            emit_native_media_progress(&app, chat_id, message_id, "ready", 100, Some(size));
            let _ = app.emit("telegram-media-ready", &meta);
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
            let _ = write_native_media_meta(&app, &meta);
            if meta.state == "failed" {
                let _ = app.emit("telegram-media-error", &meta);
            }
            meta
        }
    }
}

#[tauri::command]
pub async fn telegram_media_get_meta(
    app: AppHandle,
    chat_id: i64,
    message_id: i64,
) -> Result<NativeMediaAssetMeta, String> {
    read_native_media_meta(&app, chat_id, message_id)
}

#[tauri::command]
pub async fn telegram_media_ensure_cached(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    priority: Option<String>,
) -> Result<NativeMediaAssetMeta, String> {
    state.download_aborted.store(false, Ordering::SeqCst);
    let client_id = match ready_client_id(&state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            let mut meta = empty_native_media_meta(chat_id, message_id);
            meta.state = "failed".to_string();
            meta.error = status.error.or(Some(status.state));
            return Ok(meta);
        }
    };

    Ok(
        telegram_media_ensure_cached_inner(app, &state, client_id, chat_id, message_id, priority)
            .await,
    )
}

#[tauri::command]
pub async fn telegram_media_prepare_playback(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    mode: Option<String>,
) -> Result<NativePlaybackSource, String> {
    if let Ok(Some(meta)) = complete_native_media_meta(&app, chat_id, message_id) {
        let (playback_path, playback_mime_type) = ensure_native_playback_file(&app, &meta)?;
        println!(
            "[telegram-media] prepare_playback cached chat_id={} message_id={} url={} path={} mime={:?} total={:?}",
            chat_id,
            message_id,
            media_http::playback_url(chat_id, message_id),
            playback_path,
            playback_mime_type.as_ref().or(meta.mime_type.as_ref()),
            meta.total_bytes
        );
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
    let meta =
        telegram_media_ensure_cached(app.clone(), state, chat_id, message_id, priority).await?;
    let playback = if meta.success {
        ensure_native_playback_file(&app, &meta).ok()
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
    println!(
        "[telegram-media] prepare_playback ensured chat_id={} message_id={} success={} state={} url={:?} path={:?} mime={:?} total={:?} error={:?}",
        chat_id,
        message_id,
        meta.success,
        meta.state,
        if meta.success {
            Some(media_http::playback_url(chat_id, message_id))
        } else {
            playback_path.clone()
        },
        playback_path,
        playback_mime_type,
        meta.total_bytes,
        meta.error
    );
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

#[tauri::command]
pub async fn telegram_media_save(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    destination_path: Option<String>,
) -> Result<NativeSavedMedia, String> {
    let save_abort_flag = native_media_abort_flag(&state, chat_id, message_id).await;
    let meta = telegram_media_ensure_cached(
        app.clone(),
        state,
        chat_id,
        message_id,
        Some("user".to_string()),
    )
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
        _ => app
            .path()
            .download_dir()
            .map_err(|error| error.to_string())?
            .join(&file_name),
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
            emit_native_media_progress(&app, chat_id, message_id, stage, percent, total_bytes);
        };
        copy_file_with_progress(
            Path::new(&source_path),
            &tmp_path,
            total_bytes.unwrap_or_default(),
            &mut on_progress,
            &|| save_abort_flag.load(Ordering::SeqCst),
        )
        .map_err(|error| {
            let _ = std::fs::remove_file(&tmp_path);
            error
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

#[tauri::command]
pub async fn telegram_media_cancel(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
) -> Result<bool, String> {
    let abort_flag = native_media_abort_flag(&state, chat_id, message_id).await;
    abort_flag.store(true, Ordering::SeqCst);
    Ok(true)
}

#[tauri::command]
pub async fn telegram_media_cache_stats(app: AppHandle) -> Result<NativeMediaCacheStats, String> {
    Ok(native_media_cache_stats(&app))
}

#[tauri::command]
pub async fn telegram_media_clear_cache(app: AppHandle) -> Result<NativeMediaCacheStats, String> {
    let root = native_media_root_dir(&app)?;
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(|error| error.to_string())?;
    }
    Ok(native_media_cache_stats(&app))
}

#[tauri::command]
pub async fn telegram_media_evict_cache(
    app: AppHandle,
    max_cache_size: i64,
) -> Result<NativeMediaCacheStats, String> {
    evict_native_media_cache(&app, max_cache_size)
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

#[tauri::command]
pub async fn tdlib_cache_message_media(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
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
    let app_cache_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?;
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

#[tauri::command]
pub async fn tdlib_download_message_thumbnail(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
) -> Result<TdlibThumbnailResult, String> {
    if let Ok(meta) = read_native_media_meta(&app, chat_id, message_id) {
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

    let client_id = match ready_client_id(&state).await {
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

    let Some(thumbnail) = message_thumbnail_file(message.content) else {
        println!(
            "[telegram-thumb] no thumbnail chat_id={} message_id={}",
            chat_id, message_id
        );
        return Ok(TdlibThumbnailResult {
            success: false,
            file_path: None,
            error: Some("Mensagem sem thumbnail disponivel pelo TDLib.".to_string()),
        });
    };

    let media_dir = match native_media_dir(&app, chat_id, message_id) {
        Ok(dir) => dir,
        Err(error) => {
            return Ok(TdlibThumbnailResult {
                success: false,
                file_path: None,
                error: Some(error),
            });
        }
    };

    match download_thumbnail_to_cache(client_id, Some(thumbnail), &media_dir).await {
        Some(path) => {
            println!(
                "[telegram-thumb] ready chat_id={} message_id={} path={}",
                chat_id, message_id, path
            );
            let mut meta = read_native_media_meta(&app, chat_id, message_id)
                .unwrap_or_else(|_| empty_native_media_meta(chat_id, message_id));
            meta.thumbnail_path = Some(path.clone());
            meta.updated_at = now_millis();
            let _ = write_native_media_meta(&app, &meta);
            Ok(TdlibThumbnailResult {
                success: true,
                file_path: Some(path),
                error: None,
            })
        }
        None => {
            println!(
                "[telegram-thumb] failed chat_id={} message_id={}",
                chat_id, message_id
            );
            Ok(TdlibThumbnailResult {
                success: false,
                file_path: None,
                error: Some("TDLib nao concluiu o download da thumbnail.".to_string()),
            })
        }
    }
}

#[tauri::command]
pub async fn tdlib_download_chat_avatar(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
) -> Result<TdlibAvatarResult, String> {
    let avatar_dir = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("plasma-media")
        .join("telegram-avatars");
    if let Ok(entries) = std::fs::read_dir(&avatar_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path
                .file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value == chat_id.to_string())
                && path.exists()
            {
                println!(
                    "[telegram-avatar] cached chat_id={} path={}",
                    chat_id,
                    path.to_string_lossy()
                );
                return Ok(TdlibAvatarResult {
                    success: true,
                    file_path: Some(path.to_string_lossy().to_string()),
                    error: None,
                });
            }
        }
    }

    let client_id = match ready_client_id(&state).await {
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
        println!("[telegram-avatar] no photo chat_id={}", chat_id);
        return Ok(TdlibAvatarResult {
            success: false,
            file_path: None,
            error: Some("Chat sem avatar disponivel pelo TDLib.".to_string()),
        });
    };

    let small = photo.small;
    let big = photo.big;
    let avatar_path = match download_avatar_to_cache(&app, client_id, chat_id, Some(small)).await {
        Some(path) => Some(path),
        None => download_avatar_to_cache(&app, client_id, chat_id, Some(big)).await,
    };

    match avatar_path {
        Some(path) => {
            println!("[telegram-avatar] ready chat_id={} path={}", chat_id, path);
            Ok(TdlibAvatarResult {
                success: true,
                file_path: Some(path),
                error: None,
            })
        }
        None => {
            println!("[telegram-avatar] failed chat_id={}", chat_id);
            Ok(TdlibAvatarResult {
                success: false,
                file_path: None,
                error: Some("TDLib nao concluiu o download do avatar.".to_string()),
            })
        }
    }
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
    let mut downloaded_count = 0usize;
    let mut skipped_count = 0usize;
    let mut failed_count = 0usize;
    let mut items: Vec<TdlibDownloadItem> = Vec::new();
    let mut pending_downloads: Vec<PendingMassDownload> = Vec::new();

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
                    total: items.len(),
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
            let media_file = media.file;
            let file_name = media.file_name;

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
            let item_index = items.len();
            items.push(TdlibDownloadItem {
                name: file_name.clone(),
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

        if next_from_message_id == 0 || next_from_message_id == from_message_id {
            break;
        }
        from_message_id = next_from_message_id;
    }

    let total = items.len();
    if !state.download_aborted.load(Ordering::Acquire) {
        emit_mass_progress(
            &app,
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
                &app,
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
            &app,
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
                    &app,
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
                    &app,
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
            &app,
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
        &app,
        request.chat_id,
        total,
        (downloaded_count + skipped_count + failed_count) as f64,
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
