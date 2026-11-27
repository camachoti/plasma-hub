//! Instagram cookie persistence for private posts and reels.

use plasma_core::AppEnv;
use std::fs;

pub const INSTAGRAM_COOKIES_FILE: &str = "instagram-cookies.txt";

pub fn load_instagram_cookies(app: &AppEnv) -> Result<String, String> {
    let path = app.paths().data_dir().join(INSTAGRAM_COOKIES_FILE);
    match fs::read_to_string(path) {
        Ok(value) => Ok(value),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.to_string()),
    }
}

pub fn save_instagram_cookies(app: &AppEnv, cookies: &str) -> Result<(), String> {
    let directory = app.paths().data_dir().to_path_buf();
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let path = directory.join(INSTAGRAM_COOKIES_FILE);
    if cookies.trim().is_empty() {
        return match fs::remove_file(path) {
            Ok(()) => Ok(()),
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
