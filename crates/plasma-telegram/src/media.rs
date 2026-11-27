use std::borrow::Cow;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::types::*;
use plasma_core::AppEnv;

pub(crate) const STALE_NATIVE_PARTIAL_AGE: Duration = Duration::from_secs(24 * 60 * 60);

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

pub(crate) fn mime_extension(mime_type: &str, fallback: &str) -> String {
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
        "image/heic" | "image/heif" => ".heic".to_string(),
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
pub(crate) fn document_is_video(mime_type: &str, file_name: &str) -> bool {
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

/// Whether a document should use the photo tile.
///
/// HEIC and HEIF count: the desktop playback path converts them to JPEG before
/// the lightbox asks WebKit to paint.
pub(crate) fn document_is_image(mime_type: &str, file_name: &str) -> bool {
    let mime_type = mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .to_ascii_lowercase();
    if matches!(
        mime_type.as_str(),
        "image/jpeg" | "image/png" | "image/webp" | "image/gif" | "image/heic" | "image/heif"
    ) {
        return true;
    }
    if mime_type.starts_with("image/") {
        return false;
    }
    if !mime_type.is_empty() && mime_type != "application/octet-stream" {
        return false;
    }

    Path::new(file_name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| {
            matches!(
                extension.as_str(),
                "jpg" | "jpeg" | "png" | "webp" | "gif" | "heic" | "heif"
            )
        })
}

/// WebKitGTK cannot decode these stills. Playback swaps in a JPEG preview.
pub(crate) fn still_needs_jpeg_preview(mime_type: Option<&str>, file_name: Option<&str>) -> bool {
    let mime = mime_type
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or(value)
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    if matches!(mime.as_str(), "image/heic" | "image/heif") {
        return true;
    }
    let name = file_name.unwrap_or_default();
    Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| matches!(extension.as_str(), "heic" | "heif"))
}

pub(crate) fn playback_extension(mime_type: Option<&str>, file_name: Option<&str>) -> String {
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

pub(crate) fn normalize_playback_mime_type(
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
        ".heic" | ".heif" => Some("image/heic".to_string()),
        ".png" => Some("image/png".to_string()),
        ".webp" => Some("image/webp".to_string()),
        _ => Some("video/mp4".to_string()),
    }
}

pub(crate) fn existing_faststart_playback_file(
    media_dir: &Path,
    mime_type: Option<&str>,
) -> Option<PathBuf> {
    if mime_type != Some("video/mp4") {
        return None;
    }

    let destination = media_dir.join("playback.faststart.mp4");
    if file_is_complete(&destination, None) {
        return Some(destination);
    }
    None
}

fn jpeg_still_preview(source: &Path, media_dir: &Path, thumbnail: Option<&str>) -> Option<PathBuf> {
    let output = media_dir.join("playback.jpg");
    if output.is_file() || plasma_media::jpeg_preview_for_still(source, &output).is_ok() {
        return Some(output);
    }
    thumbnail
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .or_else(|| {
            let cached = media_dir.join("thumbnail.jpg");
            cached.is_file().then_some(cached)
        })
}

pub(crate) fn ensure_native_playback_file(
    app: &AppEnv,
    meta: &NativeMediaAssetMeta,
) -> plasma_core::Result<(String, Option<String>)> {
    let Some(source_path) = meta.native_file_path.as_ref() else {
        return Err("Mídia nativa sem arquivo.".into());
    };
    let source = Path::new(source_path);
    if !file_is_complete(source, meta.total_bytes) {
        return Err("Arquivo de mídia incompleto no cache.".into());
    }

    let mime_type =
        normalize_playback_mime_type(meta.mime_type.as_deref(), meta.file_name.as_deref());
    let media_dir = native_media_dir(app, meta.chat_id, meta.message_id)?;
    if still_needs_jpeg_preview(mime_type.as_deref(), meta.file_name.as_deref()) {
        if let Some(preview) =
            jpeg_still_preview(source, &media_dir, meta.thumbnail_path.as_deref())
        {
            return Ok((
                preview.to_string_lossy().to_string(),
                Some("image/jpeg".to_string()),
            ));
        }
    }
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

pub(crate) fn parse_plasma_media_path(path: &str) -> Option<(i64, i64)> {
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

pub(crate) fn parse_range_header(range_header: &str, len: u64) -> Option<(u64, u64)> {
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
            .body(Cow::Owned(b"resposta de midia indisponivel".to_vec()))
            .unwrap_or_else(|_| http::Response::new(Cow::Owned(Vec::new())))
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
) -> plasma_core::Result<http::Response<Cow<'static, [u8]>>> {
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
    let mut file = std::fs::File::open(&playback_path)
        .map_err(|error| plasma_core::Error::msg(error.to_string()))?;
    let len = file
        .metadata()
        .map_err(|error| plasma_core::Error::msg(error.to_string()))?
        .len();
    if request.method() == http::Method::HEAD {
        return http::Response::builder()
            .status(http::StatusCode::OK)
            .header(http::header::ACCEPT_RANGES, "bytes")
            .header(http::header::CONTENT_TYPE, content_type)
            .header(http::header::CONTENT_LENGTH, len)
            .body(Cow::Owned(Vec::new()))
            .map_err(|error| plasma_core::Error::msg(error.to_string()));
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
                .map_err(|error| plasma_core::Error::msg(error.to_string()));
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
            .map_err(|error| plasma_core::Error::msg(error.to_string()))?;
        let read = file
            .read(&mut buffer)
            .map_err(|error| plasma_core::Error::msg(error.to_string()))?;
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
            .map_err(|error| plasma_core::Error::msg(error.to_string()));
    }

    let max_initial = len.min(1024 * 1024);
    let mut buffer = vec![0_u8; max_initial as usize];
    let read = file
        .read(&mut buffer)
        .map_err(|error| plasma_core::Error::msg(error.to_string()))?;
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
        .map_err(|error| plasma_core::Error::msg(error.to_string()))
}

pub(crate) fn sanitize_filename(value: &str) -> String {
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

/// Builds a unique file path inside a user-chosen folder so a "save like a
/// browser" never overwrites existing files ("foto_123.jpg", "(1)", "(2)", ...).
pub(crate) fn unique_file_destination(folder: &Path, file_name: &str) -> PathBuf {
    let safe = sanitize_filename(file_name);
    let mut path = folder.join(&safe);
    if path.exists() {
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("arquivo")
            .to_string();
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| format!(".{value}"))
            .unwrap_or_default();
        for index in 1..10_000 {
            path = folder.join(format!("{stem} ({index}){extension}"));
            if !path.exists() {
                break;
            }
        }
    }
    path
}

pub(crate) fn sanitize_folder_name(value: &str) -> String {
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

pub(crate) fn file_name_or_default(
    file_name: String,
    message_id: i64,
    fallback_extension: &str,
) -> String {
    if file_name.trim().is_empty() {
        sanitize_filename(&format!("media_{message_id}{fallback_extension}"))
    } else {
        sanitize_filename(&file_name)
    }
}

pub(crate) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

pub(crate) fn native_media_dir(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<PathBuf> {
    Ok(app
        .paths()
        .cache_dir()
        .join("plasma-media")
        .join("telegram")
        .join(chat_id.to_string())
        .join(message_id.to_string()))
}

pub(crate) fn native_media_root_dir(app: &AppEnv) -> plasma_core::Result<PathBuf> {
    Ok(app
        .paths()
        .cache_dir()
        .join("plasma-media")
        .join("telegram"))
}

pub(crate) fn native_media_meta_path(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<PathBuf> {
    Ok(native_media_dir(app, chat_id, message_id)?.join("meta.json"))
}

pub(crate) fn empty_native_media_meta(chat_id: i64, message_id: i64) -> NativeMediaAssetMeta {
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

pub(crate) fn read_native_media_meta(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<NativeMediaAssetMeta> {
    let meta_path = native_media_meta_path(app, chat_id, message_id)?;
    if !meta_path.exists() {
        return Ok(empty_native_media_meta(chat_id, message_id));
    }

    let raw = std::fs::read_to_string(&meta_path)
        .map_err(|error| plasma_core::Error::msg(error.to_string()))?;
    serde_json::from_str(&raw).map_err(|error| plasma_core::Error::msg(error.to_string()))
}

pub(crate) fn write_native_media_meta(
    app: &AppEnv,
    meta: &NativeMediaAssetMeta,
) -> plasma_core::Result<()> {
    let meta_path = native_media_meta_path(app, meta.chat_id, meta.message_id)?;
    if let Some(parent) = meta_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| plasma_core::Error::msg(error.to_string()))?;
    }

    let tmp_path = meta_path.with_extension("json.tmp");
    let raw = serde_json::to_vec_pretty(meta)
        .map_err(|error| plasma_core::Error::msg(error.to_string()))?;
    std::fs::write(&tmp_path, raw).map_err(|error| plasma_core::Error::msg(error.to_string()))?;
    std::fs::rename(&tmp_path, &meta_path).map_err(|error| {
        let _ = std::fs::remove_file(&tmp_path);
        plasma_core::Error::from(error)
    })
}

pub(crate) fn complete_native_media_meta(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<Option<NativeMediaAssetMeta>> {
    let meta = read_native_media_meta(app, chat_id, message_id)?;
    let Some(path) = meta.native_file_path.as_ref() else {
        return Ok(None);
    };
    if meta.state == "complete" && file_is_complete(Path::new(path), meta.total_bytes) {
        return Ok(Some(meta));
    }
    Ok(None)
}

pub(crate) fn partial_file_path(destination: &Path) -> PathBuf {
    destination.with_extension(format!(
        "{}part",
        destination
            .extension()
            .map(|extension| format!("{}.", extension.to_string_lossy()))
            .unwrap_or_default()
    ))
}

pub(crate) fn file_is_complete(path: &Path, expected_size: Option<i64>) -> bool {
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

pub(crate) fn cleanup_incomplete_file(destination: &Path, expected_size: Option<i64>) {
    let partial_destination = partial_file_path(destination);
    let _ = std::fs::remove_file(&partial_destination);
    if destination.exists() && !file_is_complete(destination, expected_size) {
        let _ = std::fs::remove_file(destination);
    }
}

pub(crate) fn is_stale_file(metadata: &std::fs::Metadata, max_age: Duration) -> bool {
    metadata
        .modified()
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age >= max_age)
}

pub(crate) fn is_native_partial_artifact(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension, "part" | "tmp"))
}

pub(crate) fn cleanup_stale_native_partials_in_dir(path: &Path, max_age: Duration) -> usize {
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

pub(crate) fn cleanup_stale_native_partials(app: &AppEnv) -> usize {
    let Ok(root) = native_media_root_dir(app) else {
        return 0;
    };
    if !root.exists() {
        return 0;
    }
    cleanup_stale_native_partials_in_dir(&root, STALE_NATIVE_PARTIAL_AGE)
}

pub(crate) fn directory_size(path: &Path) -> i64 {
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

pub(crate) fn native_media_meta_from_path(path: &Path) -> Option<NativeMediaAssetMeta> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

pub(crate) fn collect_native_media_items(
    app: &AppEnv,
) -> plasma_core::Result<Vec<(PathBuf, NativeMediaAssetMeta, i64)>> {
    let root = native_media_root_dir(app)?;
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut items = Vec::new();
    for chat_entry in
        std::fs::read_dir(root).map_err(|error| plasma_core::Error::msg(error.to_string()))?
    {
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

pub(crate) fn native_media_cache_stats(app: &AppEnv) -> NativeMediaCacheStats {
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
            error: Some(error.to_string()),
        },
    }
}

pub(crate) fn evict_native_media_cache(
    app: &AppEnv,
    max_cache_size: i64,
) -> plasma_core::Result<NativeMediaCacheStats> {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unique_file_destination_appends_numeric_suffix() {
        let temp = std::env::temp_dir().join(format!("plasma-suffix-{}", now_millis()));
        std::fs::create_dir_all(&temp).unwrap();
        let first = unique_file_destination(&temp, "Foto welcome.png");
        assert_eq!(first.file_name().unwrap(), "Foto_welcome.png");
        std::fs::write(&first, b"x").unwrap();

        let second = unique_file_destination(&temp, "Foto welcome.png");
        assert_eq!(second.file_name().unwrap(), "Foto_welcome (1).png");

        let third = unique_file_destination(&temp, "sem_extensao");
        std::fs::write(&third, b"x").unwrap();
        assert_eq!(third.file_name().unwrap(), "sem_extensao");
        assert_eq!(
            unique_file_destination(&temp, "sem_extensao")
                .file_name()
                .unwrap(),
            "sem_extensao (1)"
        );

        std::fs::remove_dir_all(&temp).unwrap();
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
    fn image_documents_are_detected_as_photos() {
        assert!(document_is_image("image/jpeg", "foto.jpg"));
        assert!(document_is_image("image/png; charset=binary", "foto.png"));
        assert!(document_is_image("application/octet-stream", "foto.JPEG"));
        assert!(document_is_image("", "foto.webp"));
        assert!(document_is_image("image/heic", "foto.heic"));
        assert!(document_is_image("application/octet-stream", "foto.HEIF"));
        assert!(still_needs_jpeg_preview(
            Some("image/heic"),
            Some("foto.heic")
        ));
        assert!(still_needs_jpeg_preview(None, Some("foto.HEIF")));
        assert!(!still_needs_jpeg_preview(
            Some("image/jpeg"),
            Some("foto.jpg")
        ));
        assert!(!document_is_image("application/pdf", "foto.jpg"));
        assert!(!document_is_image("video/mp4", "clip.mp4"));
        assert!(!document_is_image("application/octet-stream", "notes.txt"));
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
