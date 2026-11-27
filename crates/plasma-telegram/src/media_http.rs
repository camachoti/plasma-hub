use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use plasma_core::AppEnv;

use crate::media::{
    cached_chat_avatar, complete_native_media_meta, ensure_native_playback_file,
    parse_plasma_media_path, parse_range_header,
};

static PLASMA_MEDIA_HTTP_PORT: OnceLock<u16> = OnceLock::new();
static PLASMA_MEDIA_HTTP_TOKEN: OnceLock<String> = OnceLock::new();

// Loopback playback needs very little concurrency. A bounded pool keeps a
// misbehaving local client from spawning an unbounded number of threads.
const MEDIA_SERVER_WORKERS: usize = 8;
const MEDIA_SERVER_QUEUE: usize = 32;

/// URL of the avatar of a chat, served by the local media server.
pub fn avatar_url(chat_id: i64) -> String {
    if let (Some(port), Some(token)) = (PLASMA_MEDIA_HTTP_PORT.get(), PLASMA_MEDIA_HTTP_TOKEN.get())
    {
        return format!("http://127.0.0.1:{port}/avatar/{chat_id}?token={token}");
    }
    format!("plasma-media://localhost/avatar/{chat_id}")
}

fn parse_avatar_path(path: &str) -> Option<i64> {
    let mut parts = path.trim_start_matches('/').split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("avatar"), Some(chat_id), None) => chat_id.parse::<i64>().ok(),
        _ => None,
    }
}

fn avatar_content_type(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "image/jpeg",
    }
}

/// Compares two byte strings without short-circuiting on the first mismatch.
/// The session token is random and fixed-length, so leaking the length is not
/// a concern; leaking the matching prefix would be.
fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    let mut difference = 0_u8;
    for (left, right) in left.iter().zip(right) {
        difference |= left ^ right;
    }
    difference == 0
}

fn request_has_token(target: &str, expected: &str) -> bool {
    target
        .split_once('?')
        .and_then(|(_, query)| {
            query
                .split('&')
                .find_map(|parameter| parameter.strip_prefix("token="))
        })
        .is_some_and(|provided| constant_time_eq(provided.as_bytes(), expected.as_bytes()))
}

pub fn playback_url(chat_id: i64, message_id: i64) -> String {
    if let (Some(port), Some(token)) = (PLASMA_MEDIA_HTTP_PORT.get(), PLASMA_MEDIA_HTTP_TOKEN.get())
    {
        return format!("http://127.0.0.1:{port}/telegram/{chat_id}/{message_id}?token={token}");
    }
    format!("plasma-media://localhost/telegram/{chat_id}/{message_id}")
}

pub fn start_server(app: AppEnv) -> plasma_core::Result<()> {
    if PLASMA_MEDIA_HTTP_PORT.get().is_some() {
        return Ok(());
    }

    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(plasma_core::Error::from)?;
    let port = listener
        .local_addr()
        .map_err(plasma_core::Error::from)?
        .port();
    let random_bytes: [u8; 32] = rand::random();
    let token = random_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let _ = PLASMA_MEDIA_HTTP_TOKEN.set(token);
    let _ = PLASMA_MEDIA_HTTP_PORT.set(port);
    tracing::info!(port, "media server listening on loopback");

    let (sender, receiver) = std::sync::mpsc::sync_channel::<TcpStream>(MEDIA_SERVER_QUEUE);
    let receiver = Arc::new(Mutex::new(receiver));
    for _ in 0..MEDIA_SERVER_WORKERS {
        let receiver = receiver.clone();
        let app = app.clone();
        std::thread::spawn(move || loop {
            let stream = {
                let receiver = receiver
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                receiver.recv()
            };
            match stream {
                Ok(stream) => handle_stream(app.clone(), stream),
                Err(_) => break,
            }
        });
    }

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    if sender.send(stream).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "media server accept failed");
                }
            }
        }
    });

    Ok(())
}

fn handle_stream(app: AppEnv, mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));
    let cloned = match stream.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            tracing::debug!(%error, "media server could not clone stream");
            return;
        }
    };
    // Bound request headers before parsing; a local client must not allocate
    // arbitrarily large buffers while waiting for the access token check.
    let mut reader = BufReader::new(cloned.take(64 * 1024));
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).is_err() || first_line.trim().is_empty() {
        return;
    }

    let mut method = String::new();
    let mut path = String::new();
    let mut target = String::new();
    let mut authorized = false;
    let mut first_parts = first_line.split_whitespace();
    if let (Some(request_method), Some(request_path)) = (first_parts.next(), first_parts.next()) {
        method = request_method.to_string();
        target = request_path.to_string();
        path = request_path
            .split('?')
            .next()
            .unwrap_or(request_path)
            .to_string();
        authorized = PLASMA_MEDIA_HTTP_TOKEN
            .get()
            .is_some_and(|expected| request_has_token(request_path, expected));
    }

    let mut range_header: Option<String> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).is_err() {
            return;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("range") {
                range_header = Some(value.trim().to_string());
            }
        }
    }

    if !authorized || !(method.eq_ignore_ascii_case("GET") || method.eq_ignore_ascii_case("HEAD")) {
        let _ = write_text_response(&mut stream, 403, "Acesso negado".to_string());
        return;
    }

    if let Err(error) = write_response(
        &app,
        &mut stream,
        &method,
        &path,
        &target,
        range_header.as_deref(),
    ) {
        tracing::debug!(%error, "media server write failed");
    }
}

fn http_status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        _ => "Internal Server Error",
    }
}

fn build_http_response(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status} {}\r\n", http_status_text(status));
    response.push_str("Connection: close\r\n");
    for (name, value) in headers {
        response.push_str(&name);
        response.push_str(": ");
        response.push_str(&value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    let mut bytes = response.into_bytes();
    bytes.extend_from_slice(&body);
    bytes
}

fn text_response(status: u16, message: String) -> Vec<u8> {
    build_http_response(
        status,
        vec![
            (
                "Content-Type".to_string(),
                "text/plain; charset=utf-8".to_string(),
            ),
            ("Content-Length".to_string(), message.len().to_string()),
        ],
        message.into_bytes(),
    )
}

fn write_headers(
    stream: &mut TcpStream,
    status: u16,
    headers: Vec<(String, String)>,
) -> std::io::Result<()> {
    let mut response = format!("HTTP/1.1 {status} {}\r\n", http_status_text(status));
    response.push_str("Connection: close\r\n");
    for (name, value) in headers {
        response.push_str(&name);
        response.push_str(": ");
        response.push_str(&value);
        response.push_str("\r\n");
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes())
}

fn write_text_response(
    stream: &mut TcpStream,
    status: u16,
    message: String,
) -> std::io::Result<()> {
    stream.write_all(&text_response(status, message))
}

fn copy_file_range(
    file: &mut std::fs::File,
    stream: &mut TcpStream,
    start: u64,
    bytes_to_send: u64,
) -> std::io::Result<u64> {
    file.seek(SeekFrom::Start(start))?;
    let mut remaining = bytes_to_send;
    let mut sent = 0_u64;
    let mut buffer = vec![0_u8; 256 * 1024];

    while remaining > 0 {
        let read_size = buffer.len().min(remaining as usize);
        let read = file.read(&mut buffer[..read_size])?;
        if read == 0 {
            break;
        }
        stream.write_all(&buffer[..read])?;
        remaining = remaining.saturating_sub(read as u64);
        sent = sent.saturating_add(read as u64);
    }

    Ok(sent)
}

/// Loopback URL that plays an allowlisted HTTPS media address.
///
/// The session token stays on the query. The webview never talks to the CDN
/// directly, so the proxy can attach the Referer each network expects.
pub fn remote_media_url(upstream: &str) -> Option<String> {
    validate_remote_url(upstream).ok()?;
    let port = PLASMA_MEDIA_HTTP_PORT.get()?;
    let token = PLASMA_MEDIA_HTTP_TOKEN.get()?;
    let mut url = reqwest::Url::parse(&format!("http://127.0.0.1:{port}/remote")).ok()?;
    url.query_pairs_mut()
        .append_pair("token", token)
        .append_pair("url", upstream);
    Some(url.to_string())
}

fn remote_upstream(target: &str) -> Result<String, String> {
    let query = target
        .split_once('?')
        .map(|(_, query)| query)
        .filter(|query| !query.is_empty())
        .ok_or_else(|| "URL ausente".to_string())?;
    let parsed = reqwest::Url::parse(&format!("http://127.0.0.1/remote?{query}"))
        .map_err(|_| "URL inválida".to_string())?;
    let upstream = parsed
        .query_pairs()
        .find(|(key, _)| key == "url")
        .map(|(_, value)| value.into_owned())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "URL ausente".to_string())?;
    validate_remote_url(&upstream)?;
    Ok(upstream)
}

fn validate_remote_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "URL inválida".to_string())?;
    if parsed.scheme() != "https" {
        return Err("Só vídeos HTTPS entram no player.".to_string());
    }
    if !parsed.username().is_empty() {
        return Err("URL de vídeo inválida.".to_string());
    }
    if parsed.port().is_some_and(|port| port != 443) {
        return Err("URL de vídeo inválida.".to_string());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "URL de vídeo inválida.".to_string())?;
    if !host_allowed(host) {
        return Err("Este endereço não pode ser reproduzido.".to_string());
    }
    Ok(())
}

fn host_allowed(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() || host.parse::<std::net::IpAddr>().is_ok() {
        return false;
    }
    const SUFFIXES: &[&str] = &[
        "redd.it",
        "redditmedia.com",
        "reddit.com",
        "cdninstagram.com",
        "fbcdn.net",
        "instagram.com",
        "tiktokcdn.com",
        "tiktokcdn-us.com",
        "tiktokcdn-eu.com",
        "tiktokv.com",
        "muscdn.com",
        "byteoversea.com",
        "ibytedtos.com",
        "tikwm.com",
        "tikwmcdn.com",
        "twimg.com",
        "googlevideo.com",
        "youtube.com",
    ];
    SUFFIXES
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
}

fn referer_for(url: &str) -> &'static str {
    let host = reqwest::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host.to_ascii_lowercase()))
        .unwrap_or_default();
    if host.contains("instagram") || host.ends_with("fbcdn.net") {
        "https://www.instagram.com/"
    } else if host.contains("tikwm") {
        "https://www.tikwm.com/"
    } else if host.contains("tiktok")
        || host.ends_with("muscdn.com")
        || host.ends_with("byteoversea.com")
        || host.ends_with("ibytedtos.com")
    {
        "https://www.tiktok.com/"
    } else if host.ends_with("twimg.com") {
        "https://x.com/"
    } else if host.ends_with("googlevideo.com") || host.contains("youtube") {
        "https://www.youtube.com/"
    } else {
        "https://www.reddit.com/"
    }
}

fn upstream_client_identity(url: &str) -> (&'static str, bool) {
    if let Ok(parsed) = reqwest::Url::parse(url) {
        let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
        if host.ends_with("googlevideo.com") {
            let client_param = parsed
                .query_pairs()
                .find(|(k, _)| k == "c")
                .map(|(_, v)| v.to_ascii_uppercase());
            match client_param.as_deref() {
                Some("ANDROID_VR") => {
                    return (
                        "com.google.android.apps.youtube.vr.oculus/1.62.27 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
                        false,
                    );
                }
                Some("ANDROID_TESTSUITE") => {
                    return (
                        "com.google.android.youtube/1.9 (Linux; U; Android 11) gzip",
                        false,
                    );
                }
                Some("IOS") => {
                    return (
                        "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
                        false,
                    );
                }
                Some("ANDROID") => {
                    return (
                        "com.google.android.youtube/19.44.38 (Linux; U; Android 14) gzip",
                        false,
                    );
                }
                _ => {}
            }
        }
    }
    (
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36",
        true,
    )
}

fn acceptable_media_type(value: &str) -> bool {
    let value = value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    value.starts_with("video/")
        || value.starts_with("audio/")
        || value == "application/octet-stream"
        || value == "application/mp4"
        || value == "binary/octet-stream"
}

fn proxy_runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("plasma-reel-proxy")
            .build()
            .expect("reel proxy runtime")
    })
}

fn write_remote_response(
    stream: &mut TcpStream,
    method: &str,
    target: &str,
    range_header: Option<&str>,
) -> std::io::Result<()> {
    let upstream = match remote_upstream(target) {
        Ok(url) => url,
        Err(message) => return write_text_response(stream, 400, message),
    };
    let referer = referer_for(&upstream).to_string();
    let range = range_header.map(str::to_string);
    let head = method.eq_ignore_ascii_case("HEAD");
    let result = proxy_runtime().block_on(stream_remote(stream, upstream, referer, range, head));
    match result {
        Ok(()) => Ok(()),
        Err(RemoteStreamError::BeforeHeaders(message)) => write_text_response(stream, 502, message),
        Err(RemoteStreamError::Streaming) => Ok(()),
    }
}

enum RemoteStreamError {
    BeforeHeaders(String),
    Streaming,
}

async fn stream_remote(
    stream: &mut TcpStream,
    upstream: String,
    referer: String,
    range: Option<String>,
    head: bool,
) -> Result<(), RemoteStreamError> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 5 {
                return attempt.stop();
            }
            match validate_remote_url(attempt.url().as_str()) {
                Ok(()) => attempt.follow(),
                Err(_) => attempt.stop(),
            }
        }))
        .build()
        .map_err(|error| {
            RemoteStreamError::BeforeHeaders(format!("Falha ao abrir o vídeo: {error}"))
        })?;
    let mut request = if head {
        client.head(&upstream)
    } else {
        client.get(&upstream)
    };
    let (user_agent, send_referer) = upstream_client_identity(&upstream);
    request = request.header("user-agent", user_agent);
    if send_referer {
        request = request.header("referer", referer);
    }
    if let Some(range) = range.as_deref() {
        request = request.header("range", range);
    }
    let response = request.send().await.map_err(|error| {
        RemoteStreamError::BeforeHeaders(format!("Falha ao abrir o vídeo: {error}"))
    })?;
    let status = response.status().as_u16();
    if status != 200 && status != 206 && status != 416 {
        return Err(RemoteStreamError::BeforeHeaders(format!(
            "A origem recusou o vídeo ({status})."
        )));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("video/mp4")
        .to_string();
    if status != 416 && !acceptable_media_type(&content_type) {
        return Err(RemoteStreamError::BeforeHeaders(
            "A origem não devolveu um vídeo.".to_string(),
        ));
    }
    let content_length = response
        .headers()
        .get(reqwest::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let content_range = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let mut headers = vec![
        ("Accept-Ranges".to_string(), "bytes".to_string()),
        ("Content-Type".to_string(), content_type),
        ("Cache-Control".to_string(), "private, no-store".to_string()),
    ];
    if let Some(content_length) = content_length {
        headers.push(("Content-Length".to_string(), content_length));
    }
    if let Some(content_range) = content_range {
        headers.push(("Content-Range".to_string(), content_range));
    }
    write_headers(stream, status, headers).map_err(|_| RemoteStreamError::Streaming)?;
    if head || status == 416 {
        return Ok(());
    }
    let mut response = response;
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| RemoteStreamError::Streaming)?
    {
        stream
            .write_all(&chunk)
            .map_err(|_| RemoteStreamError::Streaming)?;
    }
    stream.flush().map_err(|_| RemoteStreamError::Streaming)
}

/// Serves the avatar of a chat from the local cache.
fn write_avatar_response(
    app: &AppEnv,
    stream: &mut TcpStream,
    method: &str,
    chat_id: i64,
) -> std::io::Result<()> {
    let Some(path) = cached_chat_avatar(app, chat_id) else {
        return write_text_response(stream, 404, "Avatar não encontrado".to_string());
    };
    let content_type = avatar_content_type(&path);
    let mut file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(error) => return write_text_response(stream, 404, error.to_string()),
    };
    let len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);

    if method.eq_ignore_ascii_case("HEAD") {
        return write_headers(
            stream,
            200,
            vec![
                ("Accept-Ranges".to_string(), "bytes".to_string()),
                ("Content-Type".to_string(), content_type.to_string()),
                ("Content-Length".to_string(), len.to_string()),
                (
                    "Cache-Control".to_string(),
                    "private, max-age=86400".to_string(),
                ),
            ],
        );
    }

    let mut body = Vec::with_capacity(len as usize);
    file.read_to_end(&mut body)?;
    stream.write_all(&build_http_response(
        200,
        vec![
            ("Content-Type".to_string(), content_type.to_string()),
            ("Content-Length".to_string(), body.len().to_string()),
            (
                "Cache-Control".to_string(),
                "private, max-age=86400".to_string(),
            ),
        ],
        body,
    ))
}

fn write_response(
    app: &AppEnv,
    stream: &mut TcpStream,
    method: &str,
    path: &str,
    target: &str,
    range_header: Option<&str>,
) -> std::io::Result<()> {
    if path == "/remote" {
        return write_remote_response(stream, method, target, range_header);
    }

    if let Some(chat_id) = parse_avatar_path(path) {
        return write_avatar_response(app, stream, method, chat_id);
    }

    let Some((chat_id, message_id)) = parse_plasma_media_path(path) else {
        return write_text_response(stream, 400, "URL inválida".to_string());
    };

    let meta = match complete_native_media_meta(app, chat_id, message_id) {
        Ok(Some(meta)) => meta,
        Ok(None) => return write_text_response(stream, 404, "Mídia não encontrada".to_string()),
        Err(error) => return write_text_response(stream, 500, error.to_string()),
    };

    let (playback_path, playback_mime_type) = match ensure_native_playback_file(app, &meta) {
        Ok(result) => result,
        Err(error) => return write_text_response(stream, 500, error.to_string()),
    };
    let content_type = playback_mime_type
        .or(meta.mime_type.clone())
        .unwrap_or_else(|| "video/mp4".to_string());
    let mut file = match std::fs::File::open(&playback_path) {
        Ok(file) => file,
        Err(error) => return write_text_response(stream, 404, error.to_string()),
    };
    let len = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);

    if method.eq_ignore_ascii_case("HEAD") {
        return write_headers(
            stream,
            200,
            vec![
                ("Accept-Ranges".to_string(), "bytes".to_string()),
                ("Content-Type".to_string(), content_type),
                ("Content-Length".to_string(), len.to_string()),
            ],
        );
    }

    let (status, start, end) = if let Some(range_header) = range_header {
        let Some((start, end)) = parse_range_header(range_header, len) else {
            return write_headers(
                stream,
                416,
                vec![
                    ("Accept-Ranges".to_string(), "bytes".to_string()),
                    ("Content-Range".to_string(), format!("bytes */{len}")),
                    ("Content-Length".to_string(), "0".to_string()),
                ],
            );
        };
        (206, start, end)
    } else {
        (200, 0, len.saturating_sub(1))
    };

    let content_len = end + 1 - start;
    let mut headers = vec![
        ("Accept-Ranges".to_string(), "bytes".to_string()),
        ("Content-Type".to_string(), content_type),
        ("Content-Length".to_string(), content_len.to_string()),
    ];
    if status == 206 {
        headers.push((
            "Content-Range".to_string(),
            format!("bytes {start}-{end}/{len}"),
        ));
    }
    write_headers(stream, status, headers)?;
    copy_file_range(&mut file, stream, start, content_len)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::{
        constant_time_eq, referer_for, remote_upstream, request_has_token, validate_remote_url,
    };

    #[test]
    fn local_media_requires_the_session_token() {
        assert!(request_has_token("/telegram/1/2?token=secret", "secret"));
        assert!(!request_has_token("/telegram/1/2", "secret"));
        assert!(!request_has_token("/telegram/1/2?token=wrong", "secret"));
        assert!(!request_has_token("/telegram/1/2?token=secret2", "secret"));
    }

    #[test]
    fn remote_proxy_allows_reel_hosts_and_rejects_the_rest() {
        assert!(validate_remote_url("https://v.redd.it/clip/DASH_720.mp4").is_ok());
        assert!(validate_remote_url("https://scontent.cdninstagram.com/v/reel.mp4").is_ok());
        assert!(validate_remote_url("https://video.twimg.com/amplify_video/clip.mp4").is_ok());
        assert!(validate_remote_url("https://v16.tiktokcdn.com/video.mp4").is_ok());
        assert!(validate_remote_url("http://v.redd.it/clip.mp4").is_err());
        assert!(validate_remote_url("https://reddit.com.evil.test/clip.mp4").is_err());
        assert!(validate_remote_url("https://evil.example/v.redd.it/clip.mp4").is_err());
        assert!(validate_remote_url("https://127.0.0.1/clip.mp4").is_err());
        assert_eq!(
            remote_upstream(
                "/remote?token=secret&url=https%3A%2F%2Fv.redd.it%2Fclip%2FDASH_720.mp4"
            )
            .unwrap(),
            "https://v.redd.it/clip/DASH_720.mp4"
        );
        assert_eq!(
            referer_for("https://video.twimg.com/clip.mp4"),
            "https://x.com/"
        );
        assert_eq!(
            referer_for("https://scontent.cdninstagram.com/v/reel.mp4"),
            "https://www.instagram.com/"
        );
    }

    #[test]
    fn token_comparison_checks_every_byte() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secret "));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }
}
