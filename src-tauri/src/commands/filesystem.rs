use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tauri::{AppHandle, Manager};

const TWITTER_COOKIES_FILE: &str = "twitter-cookies.txt";

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
    let path = Path::new(&file_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let partial_path = format!("{}.part", file_path);
    let _ = fs::remove_file(&partial_path);
    fs::write(&partial_path, data).map_err(|e| e.to_string())?;
    fs::rename(&partial_path, path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn begin_download_file(file_path: String) -> Result<(), String> {
    let path = Path::new(&file_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let partial_path = format!("{}.part", file_path);
    let _ = fs::remove_file(&partial_path);
    fs::File::create(&partial_path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn append_download_file_chunk(file_path: String, data: Vec<u8>) -> Result<(), String> {
    let partial_path = format!("{}.part", file_path);
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&partial_path)
        .map_err(|e| e.to_string())?;
    file.write_all(&data).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn finish_download_file(file_path: String) -> Result<(), String> {
    let partial_path = format!("{}.part", file_path);
    fs::rename(&partial_path, Path::new(&file_path)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn abort_download_file(file_path: String) -> Result<(), String> {
    let partial_path = format!("{}.part", file_path);
    match fs::remove_file(&partial_path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
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
