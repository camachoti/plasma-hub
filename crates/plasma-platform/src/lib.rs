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

/// Opens an http(s) URL with the platform handler.
pub fn open_url(url: &str) -> Result<(), String> {
    let url = url.trim();
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("Link inválido.".to_string());
    }

    #[cfg(target_os = "android")]
    {
        android::open_url(url)
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
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

/// Shares a local file through the platform share sheet when available.
///
/// On desktop this falls back to revealing the file in the file manager.
pub fn share_path(path: &str) -> Result<(), String> {
    let target = Path::new(path);
    if !target.exists() {
        return Err("Arquivo não encontrado.".to_string());
    }

    #[cfg(target_os = "android")]
    {
        return android::share_path(path);
    }

    #[cfg(not(target_os = "android"))]
    {
        reveal_path(path)
    }
}

/// Pulls the first `http(s)` URL out of free-form share / intent text.
pub fn extract_http_url(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    let start = lower.find("https://").or_else(|| lower.find("http://"))?;
    let candidate = &value[start..];
    let end = candidate
        .find(|ch: char| ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '\'' | ')' | ']'))
        .unwrap_or(candidate.len());
    let trimmed =
        candidate[..end].trim_end_matches([',', '.', ')', ']', '}', ';', '!', '?', '"', '\'']);
    (!trimmed.is_empty()).then(|| trimmed.to_string())
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

    #[test]
    fn extracts_urls_from_share_text() {
        assert_eq!(
            extract_http_url("https://x.com/foo/status/1"),
            Some("https://x.com/foo/status/1".into())
        );
        assert_eq!(
            extract_http_url("Olha isso https://youtu.be/abc123 via YouTube"),
            Some("https://youtu.be/abc123".into())
        );
        assert_eq!(
            extract_http_url("<a href=\"https://www.reddit.com/r/x/comments/1\">post</a>"),
            Some("https://www.reddit.com/r/x/comments/1".into())
        );
        assert_eq!(extract_http_url("sem link aqui"), None);
    }
}
