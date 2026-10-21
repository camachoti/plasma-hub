//! Download primitives: cancellation, streamed HTTPS downloads and filesystem
//! sinks used by the webview/native extractors.

use plasma_core::AppEnv;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, OnceLock,
};
use std::time::Duration;

pub const DOWNLOAD_CANCELED_ERROR: &str = "Download cancelado pelo usuário.";

// The UI writes in 256–512 KiB chunks. Keep a single request bounded so a
// malformed or compromised renderer cannot allocate unbounded buffers.
const MAX_DOWNLOAD_CHUNK_BYTES: usize = 2 * 1024 * 1024;

fn active_downloads() -> &'static Mutex<HashMap<String, Arc<AtomicBool>>> {
    static ACTIVE_DOWNLOADS: OnceLock<Mutex<HashMap<String, Arc<AtomicBool>>>> = OnceLock::new();
    ACTIVE_DOWNLOADS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct DownloadCancellation {
    id: String,
    canceled: Arc<AtomicBool>,
}

impl DownloadCancellation {
    pub fn register(id: impl Into<String>) -> Self {
        let id = id.into();
        let canceled = Arc::new(AtomicBool::new(false));
        active_downloads()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id.clone(), canceled.clone());
        Self { id, canceled }
    }

    pub fn is_canceled(&self) -> bool {
        self.canceled.load(Ordering::Relaxed)
    }

    pub async fn wait(&self) {
        while !self.is_canceled() {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

impl Drop for DownloadCancellation {
    fn drop(&mut self) {
        active_downloads()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&self.id);
    }
}

pub fn cancel_download(id: &str) -> bool {
    let canceled = active_downloads()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(id)
        .cloned();
    if let Some(canceled) = canceled {
        canceled.store(true, Ordering::Relaxed);
        true
    } else {
        false
    }
}

pub fn is_cancellation_error(error: &str) -> bool {
    error == DOWNLOAD_CANCELED_ERROR
}

fn partial_path(file_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(file_path);
    if file_path.trim().is_empty() || path.file_name().is_none() {
        return Err("Caminho de arquivo inválido.".to_string());
    }
    if path.is_dir() {
        return Err("O destino do download não pode ser uma pasta.".to_string());
    }
    Ok(PathBuf::from(format!("{file_path}.part")))
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

#[derive(serde::Serialize, serde::Deserialize)]
struct PartialDownloadMeta {
    source_key: u64,
    etag: String,
}

fn source_key(url: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    hasher.finish()
}

fn partial_meta_path(partial: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta", partial.display()))
}

fn content_range_start(response: &reqwest::Response) -> Option<u64> {
    let value = response
        .headers()
        .get(reqwest::header::CONTENT_RANGE)?
        .to_str()
        .ok()?;
    parse_content_range_start(value)
}

fn parse_content_range_start(value: &str) -> Option<u64> {
    value
        .strip_prefix("bytes ")?
        .split('-')
        .next()?
        .parse()
        .ok()
}

fn parse_content_range_total(value: &str) -> Option<u64> {
    value.rsplit('/').next()?.parse::<u64>().ok()
}

/// Writes a complete payload, replacing any previous partial file.
pub fn save_download_file(file_path: &str, data: &[u8]) -> Result<(), String> {
    validate_chunk(data)?;
    let path = Path::new(file_path);
    let partial = partial_path(file_path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let _ = std::fs::remove_file(&partial);
    std::fs::write(&partial, data).map_err(|error| error.to_string())?;
    std::fs::rename(&partial, path).map_err(|error| error.to_string())
}

pub fn begin_download_file(file_path: &str) -> Result<(), String> {
    let path = Path::new(file_path);
    let partial = partial_path(file_path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let _ = std::fs::remove_file(&partial);
    std::fs::File::create(&partial).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn append_download_file_chunk(file_path: &str, data: &[u8]) -> Result<(), String> {
    validate_chunk(data)?;
    let partial = partial_path(file_path)?;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&partial)
        .map_err(|error| error.to_string())?;
    file.write_all(data).map_err(|error| error.to_string())
}

pub fn finish_download_file(file_path: &str) -> Result<(), String> {
    let partial = partial_path(file_path)?;
    std::fs::rename(&partial, Path::new(file_path)).map_err(|error| error.to_string())
}

pub fn abort_download_file(file_path: &str) -> Result<(), String> {
    let partial = partial_path(file_path)?;
    match std::fs::remove_file(&partial) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// Streams a remote HTTPS resource straight to disk, resuming from a previous
/// partial file when the server supports `Range` and the ETag still matches.
pub async fn download_url_to_file(
    app: &AppEnv,
    id: &str,
    url: &str,
    file_path: &str,
    headers: Option<HashMap<String, String>>,
) -> Result<(), String> {
    validate_download_url(url)?;
    let destination = Path::new(file_path);
    let partial = partial_path(file_path)?;
    let partial_meta = partial_meta_path(&partial);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let cancellation = DownloadCancellation::register(id);
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
        let stored_meta = std::fs::read(&partial_meta)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<PartialDownloadMeta>(&bytes).ok())
            .filter(|meta| meta.source_key == source_key(url));
        let resume_from = if stored_meta.is_some() {
            std::fs::metadata(&partial)
                .map(|metadata| metadata.len())
                .unwrap_or(0)
        } else {
            let _ = std::fs::remove_file(&partial);
            let _ = std::fs::remove_file(&partial_meta);
            0
        };
        let mut request = client.get(url);
        if let Some(headers) = headers {
            for (name, value) in headers {
                let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|error| format!("Header inválido: {error}"))?;
                let value = reqwest::header::HeaderValue::from_bytes(value.as_bytes())
                    .map_err(|error| format!("Valor de header inválido: {error}"))?;
                request = request.header(name, value);
            }
        }
        if resume_from > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={resume_from}-"));
            if let Some(meta) = &stored_meta {
                request = request.header(reqwest::header::IF_RANGE, meta.etag.as_str());
            }
        }
        let mut response = tokio::select! {
            result = request.send() => result.map_err(|error| error.to_string())?,
            _ = cancellation.wait() => return Err(DOWNLOAD_CANCELED_ERROR.to_string()),
        };
        if !response.status().is_success() {
            return Err(format!("Download HTTP {}", response.status()));
        }

        let resumed = resume_from > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if response.status() == reqwest::StatusCode::PARTIAL_CONTENT
            && content_range_start(&response) != Some(resume_from)
        {
            return Err("Servidor retornou um intervalo de download inesperado.".to_string());
        }
        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .filter(|value| !value.starts_with("W/"));
        if let Some(etag) = etag {
            let meta = PartialDownloadMeta {
                source_key: source_key(url),
                etag: etag.to_string(),
            };
            std::fs::write(
                &partial_meta,
                serde_json::to_vec(&meta).map_err(|error| error.to_string())?,
            )
            .map_err(|error| error.to_string())?;
        } else {
            let _ = std::fs::remove_file(&partial_meta);
        }
        let total_bytes = if resumed {
            content_range_total(&response)
                .or_else(|| response.content_length().map(|length| length + resume_from))
        } else {
            response.content_length()
        };
        let mut downloaded_bytes = if resumed { resume_from } else { 0 };
        let mut file = if resumed {
            std::fs::OpenOptions::new().append(true).open(&partial)
        } else {
            std::fs::File::create(&partial)
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
            app.emit(
                "native-file-download-progress",
                NativeFileDownloadProgress {
                    id: id.to_string(),
                    percent: progress,
                    downloaded_bytes,
                    total_bytes,
                },
            );
        }

        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        std::fs::rename(&partial, destination).map_err(|error| error.to_string())?;
        let _ = std::fs::remove_file(&partial_meta);
        app.emit(
            "native-file-download-progress",
            NativeFileDownloadProgress {
                id: id.to_string(),
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
        let _ = std::fs::remove_file(&partial);
        let _ = std::fs::remove_file(&partial_meta);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancels_registered_download() {
        let cancellation = DownloadCancellation::register("download-1");
        assert!(!cancellation.is_canceled());
        assert!(cancel_download("download-1"));
        assert!(cancellation.is_canceled());
    }

    #[test]
    fn removes_download_when_registration_drops() {
        let cancellation = DownloadCancellation::register("download-2");
        drop(cancellation);
        assert!(!cancel_download("download-2"));
    }

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

    #[test]
    fn rejects_unexpected_resume_offset() {
        assert_eq!(
            parse_content_range_start("bytes 1048576-2097151/8388608"),
            Some(1048576)
        );
        assert_eq!(parse_content_range_start("bytes */8388608"), None);
        assert_eq!(parse_content_range_start("bytes 0-99/1000"), Some(0));
    }

    #[test]
    fn sink_roundtrip_keeps_destination_clean() {
        let root =
            std::env::temp_dir().join(format!("plasma-download-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let destination = root.join("nested").join("file.bin");
        let path = destination.to_string_lossy().to_string();

        begin_download_file(&path).unwrap();
        append_download_file_chunk(&path, b"hello ").unwrap();
        append_download_file_chunk(&path, b"world").unwrap();
        finish_download_file(&path).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"hello world");

        begin_download_file(&path).unwrap();
        abort_download_file(&path).unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"hello world");
        let _ = std::fs::remove_dir_all(&root);
    }
}
