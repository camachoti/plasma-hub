use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

use crate::services::download_control::{
    is_cancellation_error, DownloadCancellation, DOWNLOAD_CANCELED_ERROR,
};

const TWITTER_COOKIES_FILE: &str = "twitter-cookies.txt";
// The webview sends file contents through IPC. Keep a single request bounded so
// a malformed or compromised renderer cannot make the native process allocate
// an unbounded buffer. The frontend writes in 256–512 KiB chunks.
const MAX_DOWNLOAD_CHUNK_BYTES: usize = 2 * 1024 * 1024;

fn partial_path(file_path: &str) -> Result<std::path::PathBuf, String> {
    let path = Path::new(file_path);
    if file_path.trim().is_empty() || path.file_name().is_none() {
        return Err("Caminho de arquivo inválido.".to_string());
    }
    if path.is_dir() {
        return Err("O destino do download não pode ser uma pasta.".to_string());
    }
    Ok(std::path::PathBuf::from(format!("{file_path}.part")))
}

fn validate_chunk(data: &[u8]) -> Result<(), String> {
    if data.len() > MAX_DOWNLOAD_CHUNK_BYTES {
        return Err(format!(
            "Bloco de download excede o limite de {} MiB.",
            MAX_DOWNLOAD_CHUNK_BYTES / (1024 * 1024)
        ));
    }
    Ok(())
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeFileDownloadProgress {
    id: String,
    percent: f64,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
}

fn validate_download_url(url: &str) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|_| "URL de download inválida.".to_string())?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err("Downloads nativos aceitam apenas URLs HTTPS válidas.".to_string());
    }
    Ok(())
}

fn content_range_total(response: &reqwest::Response) -> Option<u64> {
    let value = response.headers().get(reqwest::header::CONTENT_RANGE)?;
    let value = value.to_str().ok()?;
    parse_content_range_total(value)
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.parse::<u64>().ok()
}

#[tauri::command]
pub fn load_twitter_cookies(app: AppHandle) -> Result<String, String> {
    let path = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join(TWITTER_COOKIES_FILE);
    match fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub fn save_twitter_cookies(app: AppHandle, cookies: String) -> Result<(), String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(TWITTER_COOKIES_FILE);
    if cookies.trim().is_empty() {
        return match fs::remove_file(path) {
            Ok(_) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.to_string()),
        };
    }
    fs::write(&path, cookies).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
pub fn ensure_dir(path: String) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_download_file(file_path: String, data: Vec<u8>) -> Result<(), String> {
    validate_chunk(&data)?;
    let path = Path::new(&file_path);
    let partial_path = partial_path(&file_path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let _ = fs::remove_file(&partial_path);
    fs::write(&partial_path, data).map_err(|e| e.to_string())?;
    fs::rename(&partial_path, path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn begin_download_file(file_path: String) -> Result<(), String> {
    let path = Path::new(&file_path);
    let partial_path = partial_path(&file_path)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let _ = fs::remove_file(&partial_path);
    fs::File::create(&partial_path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn append_download_file_chunk(file_path: String, data: Vec<u8>) -> Result<(), String> {
    validate_chunk(&data)?;
    let partial_path = partial_path(&file_path)?;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&partial_path)
        .map_err(|e| e.to_string())?;
    file.write_all(&data).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn finish_download_file(file_path: String) -> Result<(), String> {
    let partial_path = partial_path(&file_path)?;
    fs::rename(&partial_path, Path::new(&file_path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn abort_download_file(file_path: String) -> Result<(), String> {
    let partial_path = partial_path(&file_path)?;
    match fs::remove_file(&partial_path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Streams a remote HTTPS resource straight to disk. This keeps large media
/// out of the webview heap and avoids serializing file bytes over Tauri IPC.
#[tauri::command]
pub async fn download_url_to_file(
    app: AppHandle,
    id: String,
    url: String,
    file_path: String,
) -> Result<(), String> {
    validate_download_url(&url)?;
    let destination = Path::new(&file_path);
    let partial = partial_path(&file_path)?;
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let cancellation = DownloadCancellation::register(id.clone());
    let result = async {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .user_agent("Plasma Hub/0.1")
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() > 5 {
                    attempt.error("Redirecionamentos demais durante o download.")
                } else if attempt.url().scheme() != "https" {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(|error| error.to_string())?;
        let resume_from = fs::metadata(&partial)
            .map(|metadata| metadata.len())
            .unwrap_or(0);
        let request = if resume_from > 0 {
            client
                .get(&url)
                .header(reqwest::header::RANGE, format!("bytes={resume_from}-"))
        } else {
            client.get(&url)
        };
        let mut response = tokio::select! {
            result = request.send() => result.map_err(|error| error.to_string())?,
            _ = cancellation.wait() => return Err(DOWNLOAD_CANCELED_ERROR.to_string()),
        };
        if !response.status().is_success() {
            return Err(format!("Download HTTP {}", response.status()));
        }

        let resumed = resume_from > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        let total_bytes = if resumed {
            content_range_total(&response)
                .or_else(|| response.content_length().map(|length| length + resume_from))
        } else {
            response.content_length()
        };
        let mut downloaded_bytes = if resumed { resume_from } else { 0 };
        let mut file = if resumed {
            fs::OpenOptions::new().append(true).open(&partial)
        } else {
            fs::File::create(&partial)
        }
        .map_err(|error| error.to_string())?;

        while let Some(chunk) = tokio::select! {
            result = response.chunk() => result.map_err(|error| error.to_string())?,
            _ = cancellation.wait() => {
                drop(file);
                return Err(DOWNLOAD_CANCELED_ERROR.to_string());
            }
        } {
            file.write_all(&chunk).map_err(|error| error.to_string())?;
            downloaded_bytes += chunk.len() as u64;
            let progress = total_bytes
                .map(|total| ((downloaded_bytes as f64 / total as f64) * 100.0).min(99.0))
                .unwrap_or(50.0);
            let _ = app.emit(
                "native-file-download-progress",
                NativeFileDownloadProgress {
                    id: id.clone(),
                    percent: progress,
                    downloaded_bytes,
                    total_bytes,
                },
            );
        }

        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&partial, destination).map_err(|error| error.to_string())?;
        let _ = app.emit(
            "native-file-download-progress",
            NativeFileDownloadProgress {
                id,
                percent: 100.0,
                downloaded_bytes,
                total_bytes: total_bytes.or(Some(downloaded_bytes)),
            },
        );
        Ok(())
    }
    .await;

    if result
        .as_ref()
        .err()
        .is_some_and(|error| !is_cancellation_error(error))
    {
        let _ = fs::remove_file(&partial);
    }
    result
}

#[tauri::command]
pub fn generate_video_thumbnail(app: AppHandle, file_path: String) -> Result<String, String> {
    let source_path = Path::new(&file_path);
    if !source_path.is_file() {
        return Err("Arquivo de vídeo não encontrado.".to_string());
    }

    let metadata = fs::metadata(source_path).map_err(|error| error.to_string())?;
    let mut hasher = DefaultHasher::new();
    file_path.hash(&mut hasher);
    metadata.len().hash(&mut hasher);
    if let Ok(modified) = metadata.modified() {
        modified.hash(&mut hasher);
    }

    let directory = app
        .path()
        .app_cache_dir()
        .map_err(|error| error.to_string())?
        .join("drop-video-thumbnails");
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;

    let output_path = directory.join(format!("{:x}.jpg", hasher.finish()));
    if output_path.is_file() {
        return Ok(output_path.to_string_lossy().to_string());
    }

    let source = source_path.to_string_lossy().to_string();
    let output = output_path.to_string_lossy().to_string();
    let attempts = ["1", "0.25", "0"];
    let mut last_error = String::new();

    for seek in attempts {
        let status = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-ss",
                seek,
                "-i",
                &source,
                "-frames:v",
                "1",
                "-vf",
                "scale=420:-2",
                &output,
            ])
            .status();

        match status {
            Ok(exit_status) if exit_status.success() && output_path.is_file() => {
                return Ok(output);
            }
            Ok(exit_status) => {
                last_error = format!("ffmpeg saiu com status {exit_status}");
                let _ = fs::remove_file(&output_path);
            }
            Err(error) => {
                last_error = if error.kind() == std::io::ErrorKind::NotFound {
                    "ffmpeg não encontrado no sistema.".to_string()
                } else {
                    error.to_string()
                };
                break;
            }
        }
    }

    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_download_destination() {
        assert!(partial_path("").is_err());
    }

    #[test]
    fn rejects_directory_as_download_destination() {
        assert!(partial_path("/").is_err());
    }

    #[test]
    fn accepts_frontend_chunk_sizes_and_rejects_oversized_chunks() {
        assert!(validate_chunk(&vec![0; 512 * 1024]).is_ok());
        assert!(validate_chunk(&vec![0; MAX_DOWNLOAD_CHUNK_BYTES + 1]).is_err());
    }

    #[test]
    fn native_url_downloads_require_https() {
        assert!(validate_download_url("https://video.twimg.com/media/example.mp4").is_ok());
        assert!(validate_download_url("http://video.twimg.com/media/example.mp4").is_err());
        assert!(validate_download_url("not-a-url").is_err());
    }

    #[test]
    fn reads_total_size_from_a_partial_content_range() {
        assert_eq!(
            parse_content_range_total("bytes 1048576-2097151/8388608"),
            Some(8388608)
        );
        assert_eq!(parse_content_range_total("bytes */*"), None);
    }
}
