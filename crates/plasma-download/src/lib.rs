//! Download primitives: cancellation, streamed HTTPS downloads and filesystem
//! sinks used by the webview/native extractors.

use plasma_core::AppEnv;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::watch;

pub const DOWNLOAD_CANCELED_ERROR: &str = "Download cancelado pelo usuário.";

// The UI writes in 256–512 KiB chunks. Keep a single request bounded so a
// malformed or compromised renderer cannot allocate unbounded buffers.
const MAX_DOWNLOAD_CHUNK_BYTES: usize = 2 * 1024 * 1024;
const MAX_DOWNLOAD_ATTEMPTS: u32 = 3;
const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);

fn active_downloads() -> &'static Mutex<HashMap<String, watch::Sender<bool>>> {
    static ACTIVE_DOWNLOADS: OnceLock<Mutex<HashMap<String, watch::Sender<bool>>>> =
        OnceLock::new();
    ACTIVE_DOWNLOADS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct DownloadCancellation {
    id: String,
    canceled: watch::Sender<bool>,
}

impl DownloadCancellation {
    pub fn register(id: impl Into<String>) -> Self {
        let id = id.into();
        let (canceled, _receiver) = watch::channel(false);
        active_downloads()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id.clone(), canceled.clone());
        Self { id, canceled }
    }

    pub fn is_canceled(&self) -> bool {
        *self.canceled.borrow()
    }

    /// Resolves as soon as the download is cancelled. `watch` keeps the wait
    /// race-free without the 100 ms polling loop this used to run.
    pub async fn wait(&self) {
        let mut receiver = self.canceled.subscribe();
        if *receiver.borrow() {
            return;
        }
        let _ = receiver.changed().await;
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
        // `send` fails when no waiter is subscribed yet; `send_replace` always
        // updates the shared value, so a later `wait()` still observes it.
        canceled.send_replace(true);
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

/// Internal download failure. The public API still returns `String` so the
/// extractors and the webview bridge stay unchanged; classification is what
/// lets the retry loop decide whether a partial file is worth keeping.
#[derive(Debug, thiserror::Error)]
enum DownloadError {
    #[error("{DOWNLOAD_CANCELED_ERROR}")]
    Cancelled,
    #[error("URL de download inválida.")]
    InvalidUrl,
    #[error("Downloads nativos aceitam apenas URLs HTTPS válidas.")]
    InsecureUrl,
    #[error("Download HTTP {0}")]
    Http(reqwest::StatusCode),
    #[error("Servidor retornou um intervalo de download inesperado.")]
    UnexpectedRange,
    #[error("Header inválido: {0}")]
    InvalidHeader(String),
    #[error(transparent)]
    Request(#[from] reqwest::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl DownloadError {
    fn is_retryable(&self) -> bool {
        match self {
            Self::Http(status) => {
                status.is_server_error()
                    || *status == reqwest::StatusCode::REQUEST_TIMEOUT
                    || *status == reqwest::StatusCode::TOO_MANY_REQUESTS
            }
            // A range mismatch usually means the partial file is stale or the
            // server lost support for resume; starting over is safe.
            Self::UnexpectedRange => true,
            Self::Request(error) => {
                error.is_timeout()
                    || error.is_connect()
                    || error.is_body()
                    || error.is_request()
                    || error.is_decode()
            }
            Self::Io(error) => matches!(
                error.kind(),
                std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::Interrupted
                    | std::io::ErrorKind::WouldBlock
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::BrokenPipe
            ),
            _ => false,
        }
    }

    fn keeps_partial(&self) -> bool {
        self.is_retryable() || matches!(self, Self::Cancelled)
    }
}

fn validate_download_url(url: &str) -> Result<(), DownloadError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| DownloadError::InvalidUrl)?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err(DownloadError::InsecureUrl);
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
///
/// Transient failures are retried with exponential backoff and keep the
/// partial file, so a flaky connection resumes instead of starting over.
pub async fn download_url_to_file(
    app: &AppEnv,
    id: &str,
    url: &str,
    file_path: &str,
    headers: Option<HashMap<String, String>>,
) -> Result<(), String> {
    validate_download_url(url).map_err(|error| error.to_string())?;
    let destination = Path::new(file_path);
    let partial = partial_path(file_path)?;
    let partial_meta = partial_meta_path(&partial);
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let cancellation = DownloadCancellation::register(id);
    let result = download_with_retries(
        app,
        id,
        url,
        destination,
        &partial,
        &partial_meta,
        &headers,
        &cancellation,
    )
    .await;

    if let Err(error) = &result {
        if !error.keeps_partial() {
            let _ = std::fs::remove_file(&partial);
            let _ = std::fs::remove_file(&partial_meta);
        }
    }

    result.map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
async fn download_with_retries(
    app: &AppEnv,
    id: &str,
    url: &str,
    destination: &Path,
    partial: &Path,
    partial_meta: &Path,
    headers: &Option<HashMap<String, String>>,
    cancellation: &DownloadCancellation,
) -> Result<(), DownloadError> {
    let mut attempt = 0_u32;
    loop {
        attempt += 1;
        match download_once(
            app,
            id,
            url,
            destination,
            partial,
            partial_meta,
            headers,
            cancellation,
        )
        .await
        {
            Ok(()) => return Ok(()),
            Err(error) if matches!(error, DownloadError::Cancelled) => return Err(error),
            Err(error) if error.is_retryable() && attempt < MAX_DOWNLOAD_ATTEMPTS => {
                if matches!(error, DownloadError::UnexpectedRange) {
                    let _ = std::fs::remove_file(partial);
                    let _ = std::fs::remove_file(partial_meta);
                }
                let delay = RETRY_BASE_DELAY.saturating_mul(1_u32 << (attempt - 1));
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = cancellation.wait() => return Err(DownloadError::Cancelled),
                }
            }
            Err(error) => return Err(error),
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn download_once(
    app: &AppEnv,
    id: &str,
    url: &str,
    destination: &Path,
    partial: &Path,
    partial_meta: &Path,
    headers: &Option<HashMap<String, String>>,
    cancellation: &DownloadCancellation,
) -> Result<(), DownloadError> {
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
        .build()?;
    let stored_meta = std::fs::read(partial_meta)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<PartialDownloadMeta>(&bytes).ok())
        .filter(|meta| meta.source_key == source_key(url));
    let resume_from = if stored_meta.is_some() {
        std::fs::metadata(partial)
            .map(|metadata| metadata.len())
            .unwrap_or(0)
    } else {
        let _ = std::fs::remove_file(partial);
        let _ = std::fs::remove_file(partial_meta);
        0
    };
    let mut request = client.get(url);
    if let Some(headers) = headers {
        for (name, value) in headers {
            let name = reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .map_err(|error| DownloadError::InvalidHeader(error.to_string()))?;
            let value = reqwest::header::HeaderValue::from_bytes(value.as_bytes())
                .map_err(|error| DownloadError::InvalidHeader(error.to_string()))?;
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
        result = request.send() => result?,
        _ = cancellation.wait() => return Err(DownloadError::Cancelled),
    };
    if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        return Err(DownloadError::UnexpectedRange);
    }
    if !response.status().is_success() {
        return Err(DownloadError::Http(response.status()));
    }

    let resumed = resume_from > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT;
    if response.status() == reqwest::StatusCode::PARTIAL_CONTENT
        && content_range_start(&response) != Some(resume_from)
    {
        return Err(DownloadError::UnexpectedRange);
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
        std::fs::write(partial_meta, serde_json::to_vec(&meta)?)?;
    } else {
        let _ = std::fs::remove_file(partial_meta);
    }
    let total_bytes = if resumed {
        content_range_total(&response)
            .or_else(|| response.content_length().map(|length| length + resume_from))
    } else {
        response.content_length()
    };
    let mut downloaded_bytes = if resumed { resume_from } else { 0 };
    let mut file = if resumed {
        std::fs::OpenOptions::new().append(true).open(partial)?
    } else {
        std::fs::File::create(partial)?
    };

    while let Some(chunk) = tokio::select! {
        result = response.chunk() => result?,
        _ = cancellation.wait() => {
            drop(file);
            return Err(DownloadError::Cancelled);
        }
    } {
        file.write_all(&chunk)?;
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

    file.sync_all()?;
    drop(file);
    std::fs::rename(partial, destination)?;
    let _ = std::fs::remove_file(partial_meta);
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

    #[tokio::test]
    async fn cancellation_wakes_waiters_without_polling() {
        let cancellation = DownloadCancellation::register("download-wait");
        let waiter = tokio::spawn(async move {
            cancellation.wait().await;
            cancellation.is_canceled()
        });
        tokio::task::yield_now().await;
        assert!(cancel_download("download-wait"));
        let canceled = tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("wait timed out")
            .expect("join waiter");
        assert!(canceled);
    }

    #[test]
    fn classifies_transient_and_permanent_download_errors() {
        assert!(DownloadError::Http(reqwest::StatusCode::SERVICE_UNAVAILABLE).is_retryable());
        assert!(DownloadError::Http(reqwest::StatusCode::TOO_MANY_REQUESTS).is_retryable());
        assert!(DownloadError::Http(reqwest::StatusCode::REQUEST_TIMEOUT).is_retryable());
        assert!(!DownloadError::Http(reqwest::StatusCode::NOT_FOUND).is_retryable());
        assert!(DownloadError::UnexpectedRange.is_retryable());
        assert!(
            DownloadError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"))
                .is_retryable()
        );
        assert!(!DownloadError::InvalidUrl.is_retryable());
        assert!(DownloadError::Cancelled.keeps_partial());
        assert!(!DownloadError::InsecureUrl.keeps_partial());
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
