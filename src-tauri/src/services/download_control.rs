use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    time::Duration,
};

pub const DOWNLOAD_CANCELED_ERROR: &str = "Download cancelado pelo usuário.";

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

#[tauri::command]
pub fn cancel_native_download(id: String) -> bool {
    cancel_download(&id)
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
}
