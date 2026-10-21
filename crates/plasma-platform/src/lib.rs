//! Filesystem and OS helpers that do not belong to a specific product domain.

#[cfg(target_os = "android")]
pub mod android;

#[cfg(target_os = "android")]
pub use android::{
    android_app_dirs, android_video_thumbnail, pick_media_file, take_initial_shared_url,
    take_pending_shared_url, AndroidAppDirs,
};

use std::path::Path;

/// Callback invoked when Android delivers a shared URL while the app is running.
pub type ShareHandler = Box<dyn Fn(String) + Send + Sync + 'static>;

/// Registers the process-wide handler for Android share intents.
///
/// Desktop builds have no share intents, so the handler is simply dropped.
pub fn set_share_handler(handler: ShareHandler) {
    #[cfg(target_os = "android")]
    android::set_share_handler(handler);

    #[cfg(not(target_os = "android"))]
    drop(handler);
}

/// Desktop builds never receive Android share intents.
#[cfg(not(target_os = "android"))]
pub fn take_initial_shared_url() -> Option<String> {
    None
}

/// Desktop builds never queue Android share intents.
#[cfg(not(target_os = "android"))]
pub fn take_pending_shared_url() -> Option<String> {
    None
}

/// Returns whether a path exists, mirroring the old `path_exists` command.
pub fn path_exists(path: &str) -> bool {
    Path::new(path).exists()
}

/// Creates a directory and every missing parent.
pub fn ensure_dir(path: &str) -> Result<(), String> {
    std::fs::create_dir_all(path).map_err(|error| error.to_string())
}

/// Opens a file with the platform handler.
pub fn open_path(path: &str) -> Result<(), String> {
    let target = Path::new(path);
    if !target.exists() {
        return Err("Arquivo não encontrado.".to_string());
    }

    #[cfg(target_os = "android")]
    {
        return android::open_path(path);
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(target)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// Reveals a file in the platform file manager.
///
/// Android has no equivalent "reveal" action, so it falls back to opening the
/// file itself.
pub fn reveal_path(path: &str) -> Result<(), String> {
    let target = Path::new(path);
    if !target.exists() {
        return Err("Arquivo não encontrado.".to_string());
    }

    #[cfg(target_os = "android")]
    {
        return android::open_path(path);
    }

    #[cfg(not(target_os = "android"))]
    {
        let directory = target.parent().unwrap_or(target);
        open_path(directory.to_string_lossy().as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_nested_directories() {
        let root =
            std::env::temp_dir().join(format!("plasma-platform-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let target = root.join("a").join("b");
        ensure_dir(target.to_string_lossy().as_ref()).unwrap();
        assert!(path_exists(target.to_string_lossy().as_ref()));
        assert!(!path_exists(
            root.join("missing").to_string_lossy().as_ref()
        ));
        let _ = std::fs::remove_dir_all(&root);
    }
}
