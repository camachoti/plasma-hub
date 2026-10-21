//! Twitter/X cookie persistence.

use plasma_core::AppEnv;
use std::fs;

pub const TWITTER_COOKIES_FILE: &str = "twitter-cookies.txt";

/// Reads the stored cookie header/file contents. Missing files return an empty
/// string, matching the previous `load_twitter_cookies` command.
pub fn load_twitter_cookies(app: &AppEnv) -> Result<String, String> {
    let path = app.paths().data_dir().join(TWITTER_COOKIES_FILE);
    match fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.to_string()),
    }
}

/// Persists cookies with owner-only permissions on Unix. Empty input removes
/// the stored file.
pub fn save_twitter_cookies(app: &AppEnv, cookies: &str) -> Result<(), String> {
    let directory = app.paths().data_dir().to_path_buf();
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

#[cfg(test)]
mod tests {
    use super::*;
    use plasma_core::AppPaths;
    use std::path::PathBuf;

    fn env(name: &str) -> (AppEnv, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "plasma-twitter-cookies-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let paths = AppPaths::new(
            root.join("data"),
            root.join("local"),
            root.join("cache"),
            Some(root.join("downloads")),
        )
        .unwrap();
        (AppEnv::headless(paths), root)
    }

    #[test]
    fn roundtrips_and_removes_cookies() {
        let (app, root) = env("roundtrip");
        assert_eq!(load_twitter_cookies(&app).unwrap(), "");
        save_twitter_cookies(&app, "auth_token=abc").unwrap();
        assert_eq!(load_twitter_cookies(&app).unwrap(), "auth_token=abc");
        save_twitter_cookies(&app, "   ").unwrap();
        assert_eq!(load_twitter_cookies(&app).unwrap(), "");
        let _ = std::fs::remove_dir_all(root);
    }
}
