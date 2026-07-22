use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::OnceLock;

use tauri::AppHandle;

use super::{
    complete_native_media_meta, ensure_native_playback_file, parse_plasma_media_path,
    parse_range_header,
};

static PLASMA_MEDIA_HTTP_PORT: OnceLock<u16> = OnceLock::new();

pub fn playback_url(chat_id: i64, message_id: i64) -> String {
    if let Some(port) = PLASMA_MEDIA_HTTP_PORT.get() {
        return format!("http://127.0.0.1:{port}/telegram/{chat_id}/{message_id}");
    }
    format!("plasma-media://localhost/telegram/{chat_id}/{message_id}")
}

pub fn start_server(app: AppHandle) -> Result<(), String> {
    if PLASMA_MEDIA_HTTP_PORT.get().is_some() {
        return Ok(());
    }

    let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
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

fn handle_stream(app: AppHandle, mut stream: TcpStream) {
    let cloned = match stream.try_clone() {
        Ok(stream) => stream,
        Err(error) => {
            println!("[plasma-media-http] clone stream error={error}");
            return;
        }
    };
    let mut reader = BufReader::new(cloned);
    let mut first_line = String::new();
    if reader.read_line(&mut first_line).is_err() || first_line.trim().is_empty() {
        return;
    }

    let mut method = "";
    let mut path = "";
    let mut first_parts = first_line.split_whitespace();
    if let (Some(request_method), Some(request_path)) = (first_parts.next(), first_parts.next()) {
        method = request_method;
        path = request_path.split('?').next().unwrap_or(request_path);
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

    if let Err(error) = write_response(&app, &mut stream, method, path, range_header.as_deref()) {
        println!("[plasma-media-http] write error={error}");
    }
}

fn http_status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        404 => "Not Found",
        416 => "Range Not Satisfiable",
        _ => "Internal Server Error",
    }
}

fn build_http_response(status: u16, headers: Vec<(String, String)>, body: Vec<u8>) -> Vec<u8> {
    let mut response = format!("HTTP/1.1 {status} {}\r\n", http_status_text(status));
    response.push_str("Access-Control-Allow-Origin: *\r\n");
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
    response.push_str("Access-Control-Allow-Origin: *\r\n");
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

fn write_response(
    app: &AppHandle,
    stream: &mut TcpStream,
    method: &str,
    path: &str,
    range_header: Option<&str>,
) -> std::io::Result<()> {
    println!(
        "[plasma-media-http] request method={} path={} range={:?}",
        method, path, range_header
    );
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
    println!(
        "[plasma-media-http] resolved chat_id={} message_id={} path={} len={} content_type={}",
        chat_id, message_id, playback_path, len, content_type
    );

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
    let sent = copy_file_range(&mut file, stream, start, content_len)?;
    println!(
        "[plasma-media-http] response status={} range={}-{} len={} sent={}",
        status, start, end, len, sent
    );
    stream.flush()
}
