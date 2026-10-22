use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::OnceLock;
use std::time::Duration;

use plasma_core::AppEnv;

use super::{
    cached_chat_avatar, complete_native_media_meta, ensure_native_playback_file,
    parse_plasma_media_path, parse_range_header,
};

static PLASMA_MEDIA_HTTP_PORT: OnceLock<u16> = OnceLock::new();
static PLASMA_MEDIA_HTTP_TOKEN: OnceLock<String> = OnceLock::new();

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

fn request_has_token(target: &str, expected: &str) -> bool {
    target
        .split_once('?')
        .and_then(|(_, query)| {
            query
                .split('&')
                .find_map(|parameter| parameter.strip_prefix("token="))
        })
        .is_some_and(|provided| provided == expected)
}

pub fn playback_url(chat_id: i64, message_id: i64) -> String {
    if let (Some(port), Some(token)) = (PLASMA_MEDIA_HTTP_PORT.get(), PLASMA_MEDIA_HTTP_TOKEN.get())
    {
        return format!("http://127.0.0.1:{port}/telegram/{chat_id}/{message_id}?token={token}");
    }
    format!("plasma-media://localhost/telegram/{chat_id}/{message_id}")
}

pub fn start_server(app: AppEnv) -> Result<(), String> {
    if PLASMA_MEDIA_HTTP_PORT.get().is_some() {
        return Ok(());
    }

    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let random_bytes: [u8; 32] = rand::random();
    let token = random_bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let _ = PLASMA_MEDIA_HTTP_TOKEN.set(token);
    let _ = PLASMA_MEDIA_HTTP_PORT.set(port);
    println!("[plasma-media-http] listening on http://127.0.0.1:{port}");

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let app = app.clone();
                    std::thread::spawn(move || handle_stream(app, stream));
                }
                Err(error) => {
                    println!("[plasma-media-http] accept error={error}");
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
            println!("[plasma-media-http] clone stream error={error}");
            return;
        }
    };
    // Bound request headers before parsing; a local client must not allocate
    // arbitrarily large buffers while waiting for the access token check.
    let mut reader = BufReader::new(cloned.take(16 * 1024));
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).is_err() || first_line.trim().is_empty() {
        return;
    }

    let mut method = "";
    let mut path = "";
    let mut authorized = false;
    let mut first_parts = first_line.split_whitespace();
    if let (Some(request_method), Some(request_path)) = (first_parts.next(), first_parts.next()) {
        method = request_method;
        path = request_path.split('?').next().unwrap_or(request_path);
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

    if let Err(error) = write_response(&app, &mut stream, method, path, range_header.as_deref()) {
        println!("[plasma-media-http] write error={error}");
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
    range_header: Option<&str>,
) -> std::io::Result<()> {
    if let Some(chat_id) = parse_avatar_path(path) {
        return write_avatar_response(app, stream, method, chat_id);
    }

    let Some((chat_id, message_id)) = parse_plasma_media_path(path) else {
        return write_text_response(stream, 400, "URL inválida".to_string());
    };

    let meta = match complete_native_media_meta(app, chat_id, message_id) {
        Ok(Some(meta)) => meta,
        Ok(None) => return write_text_response(stream, 404, "Mídia não encontrada".to_string()),
        Err(error) => return write_text_response(stream, 500, error),
    };

    let (playback_path, playback_mime_type) = match ensure_native_playback_file(app, &meta) {
        Ok(result) => result,
        Err(error) => return write_text_response(stream, 500, error),
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
    use super::request_has_token;

    #[test]
    fn local_media_requires_the_session_token() {
        assert!(request_has_token("/telegram/1/2?token=secret", "secret"));
        assert!(!request_has_token("/telegram/1/2", "secret"));
        assert!(!request_has_token("/telegram/1/2?token=wrong", "secret"));
    }
}
