//! Download queue state, native URL downloads and persisted history.
//!
//! The React app keeps its queue in memory and mirrors the history JSON into
//! SQLite. The Rust shell uses the same table and JSON shape so switching
//! between the two apps does not lose completed downloads.

use crate::state::{now_millis, push_toast, AppServices, ToastKind, UiState};
use dioxus::prelude::WritableExt;
use plasma_core::AppEnv;
use plasma_store::download_history_get;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A single download item. Unknown fields from the React history are preserved
/// through `extra` when the shell rewrites the history.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadRecord {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub chat_id: Option<String>,
    #[serde(default)]
    pub message_id: Option<i64>,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub progress: f64,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub file_path: Option<String>,
    #[serde(default)]
    pub file_size: Option<i64>,
    #[serde(default)]
    pub source_url: Option<String>,
    #[serde(default)]
    pub source_page_url: Option<String>,
    #[serde(default)]
    pub source_mode: Option<String>,
    #[serde(default)]
    pub source_format_id: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub source_label: Option<String>,
    #[serde(default)]
    pub batch_id: Option<String>,
    #[serde(default)]
    pub batch_title: Option<String>,
    #[serde(default)]
    pub batch_kind: Option<String>,
    #[serde(default)]
    pub batch_total: Option<i64>,
    #[serde(default)]
    pub batch_downloaded: Option<i64>,
    #[serde(default)]
    pub created_at: Option<i64>,
    #[serde(default)]
    pub can_cancel: Option<bool>,
    #[serde(default)]
    pub can_retry: Option<bool>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl DownloadRecord {
    pub fn new_url(id: String, file_name: String, file_path: String, source_url: String) -> Self {
        Self {
            id,
            file_name,
            progress: 0.0,
            status: "downloading".to_string(),
            file_path: Some(file_path),
            source_url: Some(source_url),
            platform: Some("web".to_string()),
            created_at: Some(now_millis()),
            can_cancel: Some(true),
            can_retry: Some(false),
            ..Self::default()
        }
    }

    pub fn is_active(&self) -> bool {
        self.status == "downloading"
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.status.as_str(), "completed" | "failed" | "canceled")
    }

    pub fn percent(&self) -> f64 {
        self.progress.clamp(0.0, 100.0)
    }

    pub fn title(&self) -> String {
        if !self.file_name.trim().is_empty() {
            self.file_name.clone()
        } else if let Some(url) = self.source_url.as_ref() {
            url.clone()
        } else {
            self.id.clone()
        }
    }
}

/// Applies a native progress event to the in-memory queue.
pub fn apply_download_progress(
    mut ui: UiState,
    id: &str,
    downloaded: u64,
    total: u64,
    percent: Option<f64>,
) {
    let mut items = ui.downloads();
    let Some(item) = items.iter_mut().find(|item| item.id == id) else {
        return;
    };
    if item.status == "canceled" {
        return;
    }
    item.status = "downloading".to_string();
    item.progress = if let Some(percent) = percent {
        percent.clamp(0.0, 99.5)
    } else if total > 0 {
        ((downloaded as f64 / total as f64) * 100.0).clamp(0.0, 99.5)
    } else {
        item.progress.max(1.0)
    };
    if total > 0 {
        item.file_size = Some(total.min(i64::MAX as u64) as i64);
    }
    if let Some(total) = item.batch_total {
        let fraction = item.progress / 100.0;
        item.batch_downloaded = Some((total as f64 * fraction).round() as i64);
    }
    item.can_cancel = Some(true);
    item.can_retry = Some(false);
    ui.downloads.set(items);
}

pub fn apply_download_done(mut ui: UiState, id: &str) {
    let mut items = ui.downloads();
    let Some(item) = items.iter_mut().find(|item| item.id == id) else {
        return;
    };
    item.progress = 100.0;
    item.status = "completed".to_string();
    item.error = None;
    item.can_cancel = Some(false);
    item.can_retry = Some(false);
    ui.downloads.set(items);
}

pub fn apply_download_error(mut ui: UiState, id: &str, error: String) {
    let mut items = ui.downloads();
    let Some(item) = items.iter_mut().find(|item| item.id == id) else {
        return;
    };
    if item.status == "canceled" {
        return;
    }
    item.status = "failed".to_string();
    item.error = Some(error);
    item.can_cancel = Some(false);
    item.can_retry = Some(true);
    ui.downloads.set(items);
}

pub fn persist_downloads(services: AppServices, ui: UiState) {
    let items = ui.downloads();
    spawn_persist(services, items);
}

pub fn push_download(services: AppServices, mut ui: UiState, record: DownloadRecord) {
    let mut items = ui.downloads();
    items.retain(|item| item.id != record.id);
    items.push(record);
    if items.len() > 300 {
        let drain = items.len() - 300;
        items.drain(0..drain);
    }
    ui.downloads.set(items.clone());
    spawn_persist(services, items);
}

pub fn update_download(
    services: AppServices,
    mut ui: UiState,
    id: &str,
    update: impl FnOnce(&mut DownloadRecord),
    persist: bool,
) {
    let mut items = ui.downloads();
    let Some(item) = items.iter_mut().find(|item| item.id == id) else {
        return;
    };
    update(item);
    let items_snapshot = items.clone();
    ui.downloads.set(items);
    if persist {
        spawn_persist(services, items_snapshot);
    }
}

fn spawn_persist(services: AppServices, items: Vec<DownloadRecord>) {
    dioxus::prelude::spawn(async move {
        let _ = services.downloads.save_history(items).await;
    });
}

/// Native handle used by the Rust UI for queue operations.
#[derive(Clone)]
pub struct DownloadHandle {
    env: AppEnv,
    runtime: tokio::runtime::Handle,
    save_lock: std::sync::Arc<tokio::sync::Mutex<()>>,
}

impl DownloadHandle {
    pub fn new(env: AppEnv, runtime: tokio::runtime::Handle) -> Self {
        Self {
            env,
            runtime,
            save_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn download_dir(&self) -> PathBuf {
        self.env
            .paths()
            .download_dir()
            .unwrap_or_else(|_| self.env.paths().data_dir().join("downloads"))
    }

    pub fn destination_for(&self, file_name: &str) -> String {
        let safe = sanitize_filename(file_name);
        let directory = self.download_dir();
        let mut path = directory.join(&safe);
        if path.exists() {
            let stem = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("download")
                .to_string();
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .map(|value| format!(".{value}"))
                .unwrap_or_default();
            for index in 1..10_000 {
                path = directory.join(format!("{stem} ({index}){extension}"));
                if !path.exists() {
                    break;
                }
            }
        }
        path.to_string_lossy().to_string()
    }

    pub async fn download_url(
        &self,
        id: String,
        url: String,
        file_path: String,
        headers: Option<HashMap<String, String>>,
    ) -> Result<(), String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_download::download_url_to_file(&env, &id, &url, &file_path, headers).await
        });
        self.join(handle).await?
    }

    pub fn cancel(&self, id: &str) -> bool {
        plasma_download::cancel_download(id)
    }

    pub async fn load_history(&self) -> Result<Vec<DownloadRecord>, String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            tokio::task::spawn_blocking(move || download_history_get(&env)).await
        });
        let raw = self
            .join(handle)
            .await?
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
        let Some(raw) = raw else {
            return Ok(Vec::new());
        };
        let values: Vec<Value> =
            serde_json::from_str(&raw).map_err(|error| format!("Histórico inválido: {error}"))?;
        Ok(values
            .into_iter()
            .filter_map(|value| serde_json::from_value(value).ok())
            .map(|mut item: DownloadRecord| {
                if item.status == "downloading" {
                    item.status = "failed".to_string();
                    item.error = Some("Download interrompido ao fechar o aplicativo.".to_string());
                    item.can_cancel = Some(false);
                    item.can_retry = Some(item.source_url.is_some() && item.file_path.is_some());
                }
                item
            })
            .collect())
    }

    pub async fn save_history(&self, items: Vec<DownloadRecord>) -> Result<(), String> {
        let env = self.env.clone();
        let save_lock = self.save_lock.clone();
        let handle = self.runtime.spawn(async move {
            let _guard = save_lock.lock().await;
            let values: Vec<Value> = items
                .into_iter()
                .rev()
                .take(300)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|item| serde_json::to_value(item).unwrap_or(Value::Null))
                .collect();
            let raw = serde_json::to_string(&values).map_err(|error| error.to_string())?;
            tokio::task::spawn_blocking(move || plasma_store::download_history_save(&env, raw))
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())
        });
        self.join(handle).await?
    }

    async fn join<T>(&self, handle: tokio::task::JoinHandle<T>) -> Result<T, String> {
        handle
            .await
            .map_err(|error| format!("Tarefa de download falhou: {error}"))
    }
}

pub fn sanitize_filename(file_name: &str) -> String {
    let name = Path::new(file_name)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("download");
    let safe: String = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ' ' | ')' | '(') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = safe.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        "download".to_string()
    } else {
        trimmed
    }
}

pub fn new_download_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let counter = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("web-{}-{counter}", now_millis())
}

pub fn toast_download_error(ui: UiState, message: impl Into<String>) {
    push_toast(ui.toasts, message, ToastKind::Error);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_paths_and_keeps_extensions() {
        assert_eq!(sanitize_filename("../../clip.mp4"), "clip.mp4");
        assert_eq!(sanitize_filename("foto final.jpg"), "foto final.jpg");
        assert_eq!(sanitize_filename(""), "download");
    }

    #[test]
    fn clamps_progress_and_formats_title() {
        let mut item = DownloadRecord::new_url(
            "1".into(),
            "video.mp4".into(),
            "/tmp/video.mp4".into(),
            "https://example.com/video.mp4".into(),
        );
        item.progress = 140.0;
        assert_eq!(item.percent(), 100.0);
        assert!(item.is_active());
        item.status = "completed".into();
        assert!(item.is_terminal());
        assert_eq!(item.title(), "video.mp4");
    }

    #[test]
    fn preserves_unknown_history_fields() {
        let raw = r#"{"id":"x","fileName":"a","customField":42}"#;
        let item: DownloadRecord = serde_json::from_str(raw).unwrap();
        assert_eq!(item.extra["customField"], 42);
        let encoded = serde_json::to_value(item).unwrap();
        assert_eq!(encoded["customField"], 42);
    }
}
