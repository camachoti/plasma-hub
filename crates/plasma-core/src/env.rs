use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Filesystem locations owned by the application.
///
/// The values are resolved once at startup by the composition root
/// (`apps/plasma`) and shared by every service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    data_dir: PathBuf,
    local_data_dir: PathBuf,
    cache_dir: PathBuf,
    download_dir: Option<PathBuf>,
}

impl AppPaths {
    /// Creates the app-owned directories and returns the resolved paths.
    pub fn new(
        data_dir: impl Into<PathBuf>,
        local_data_dir: impl Into<PathBuf>,
        cache_dir: impl Into<PathBuf>,
        download_dir: Option<PathBuf>,
    ) -> std::io::Result<Self> {
        let paths = Self {
            data_dir: data_dir.into(),
            local_data_dir: local_data_dir.into(),
            cache_dir: cache_dir.into(),
            download_dir,
        };
        paths.ensure_dirs()?;
        Ok(paths)
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for dir in [&self.data_dir, &self.local_data_dir, &self.cache_dir] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn local_data_dir(&self) -> &Path {
        &self.local_data_dir
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// The platform downloads directory, when the OS exposes one.
    ///
    /// Services keep their current fallback behaviour by treating this as a
    /// `Result`, exactly like the previous `AppHandle::path()` calls.
    pub fn download_dir(&self) -> Result<PathBuf, String> {
        self.download_dir
            .clone()
            .ok_or_else(|| "Diretório de downloads indisponível neste sistema.".to_string())
    }

    pub fn has_download_dir(&self) -> bool {
        self.download_dir.is_some()
    }
}

/// Sink used by domain services to publish events to the UI.
///
/// The Tauri adapter serialises these into Tauri events; the future Dioxus app
/// will forward them into a `broadcast` channel. Keeping the sink generic is
/// what allows the domain crates to stay UI-agnostic.
pub trait EventEmitter: Send + Sync + 'static {
    fn emit_json(&self, event: &str, payload: Value);
}

/// Emitter used by tests, CLIs and headless tools.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoopEmitter;

impl EventEmitter for NoopEmitter {
    fn emit_json(&self, _event: &str, _payload: Value) {}
}

/// Runtime context handed to every service.
#[derive(Clone)]
pub struct AppEnv {
    paths: Arc<AppPaths>,
    events: Arc<dyn EventEmitter>,
}

impl AppEnv {
    pub fn new(paths: AppPaths, events: Arc<dyn EventEmitter>) -> Self {
        Self {
            paths: Arc::new(paths),
            events,
        }
    }

    pub fn headless(paths: AppPaths) -> Self {
        Self::new(paths, Arc::new(NoopEmitter))
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    /// Emits an event, serialising the payload to JSON.
    ///
    /// Serialisation failures are logged instead of panicking so a malformed
    /// payload can never take the app down.
    pub fn emit<S: Serialize>(&self, event: &str, payload: S) {
        match serde_json::to_value(payload) {
            Ok(value) => self.events.emit_json(event, value),
            Err(error) => {
                tracing::error!(event, %error, "failed to serialise event payload");
            }
        }
    }

    pub fn emit_value(&self, event: &str, payload: Value) {
        self.events.emit_json(event, payload);
    }
}

impl std::fmt::Debug for AppEnv {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AppEnv")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingEmitter {
        events: Mutex<Vec<(String, Value)>>,
    }

    impl EventEmitter for RecordingEmitter {
        fn emit_json(&self, event: &str, payload: Value) {
            self.events
                .lock()
                .unwrap()
                .push((event.to_string(), payload));
        }
    }

    fn temp_paths(name: &str) -> AppPaths {
        let root =
            std::env::temp_dir().join(format!("plasma-core-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        AppPaths::new(
            root.join("data"),
            root.join("local"),
            root.join("cache"),
            Some(root.join("downloads")),
        )
        .unwrap()
    }

    #[test]
    fn creates_owned_directories() {
        let paths = temp_paths("dirs");
        assert!(paths.data_dir().is_dir());
        assert!(paths.local_data_dir().is_dir());
        assert!(paths.cache_dir().is_dir());
        assert_eq!(
            paths.download_dir().unwrap(),
            std::env::temp_dir()
                .join(format!("plasma-core-test-dirs-{}", std::process::id()))
                .join("downloads")
        );
        let _ = std::fs::remove_dir_all(paths.data_dir().parent().unwrap());
    }

    #[test]
    fn emits_serialised_events() {
        let emitter = Arc::new(RecordingEmitter::default());
        let env = AppEnv::new(temp_paths("events"), emitter.clone());
        env.emit("demo", serde_json::json!({ "ok": true }));
        let events = emitter.events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0, "demo");
        assert_eq!(events[0].1["ok"], true);
        let _ = std::fs::remove_dir_all(env.paths().data_dir().parent().unwrap());
    }

    #[test]
    fn missing_download_dir_is_reported() {
        let root = std::env::temp_dir().join(format!(
            "plasma-core-test-nodownload-{}",
            std::process::id()
        ));
        let paths = AppPaths::new(
            root.join("data"),
            root.join("local"),
            root.join("cache"),
            None,
        )
        .unwrap();
        assert!(paths.download_dir().is_err());
        assert!(!paths.has_download_dir());
        let _ = std::fs::remove_dir_all(&root);
    }
}
