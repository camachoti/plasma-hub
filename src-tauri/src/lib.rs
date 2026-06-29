mod db;
mod downloader;
mod tdlib_native;
mod twitter_native;
mod youtube;

use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn path_exists(path: String) -> bool {
    Path::new(&path).exists()
}

#[tauri::command]
fn ensure_dir(path: String) -> Result<(), String> {
    fs::create_dir_all(path).map_err(|e| e.to_string())
}

#[tauri::command]
fn save_download_file(file_path: String, data: Vec<u8>) -> Result<(), String> {
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
fn begin_download_file(file_path: String) -> Result<(), String> {
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
fn append_download_file_chunk(file_path: String, data: Vec<u8>) -> Result<(), String> {
    let partial_path = format!("{}.part", file_path);
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&partial_path)
        .map_err(|e| e.to_string())?;
    file.write_all(&data).map_err(|e| e.to_string())
}

#[tauri::command]
fn finish_download_file(file_path: String) -> Result<(), String> {
    let partial_path = format!("{}.part", file_path);
    fs::rename(&partial_path, Path::new(&file_path)).map_err(|e| e.to_string())
}

#[tauri::command]
fn abort_download_file(file_path: String) -> Result<(), String> {
    let partial_path = format!("{}.part", file_path);
    match fs::remove_file(&partial_path) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .setup(|app| {
            let conn = db::init_db().expect("Failed to initialize database");
            app.manage(db::DbState {
                conn: std::sync::Mutex::new(conn),
            });

            app.manage(downloader::DownloaderState {
                active_downloads: Arc::new(Mutex::new(Vec::new())),
            });

            app.manage(tdlib_native::TdlibManager::default());

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            path_exists,
            ensure_dir,
            save_download_file,
            begin_download_file,
            append_download_file_chunk,
            finish_download_file,
            abort_download_file,
            tdlib_native::tdlib_init,
            tdlib_native::tdlib_status,
            tdlib_native::tdlib_set_phone,
            tdlib_native::tdlib_check_code,
            tdlib_native::tdlib_check_password,
            tdlib_native::tdlib_get_me,
            tdlib_native::tdlib_download_message_media,
            tdlib_native::tdlib_start_mass_download,
            tdlib_native::tdlib_stop_download,
            db::get_downloads,
            downloader::start_download,
            twitter_native::analyze_twitter_profile_native,
            twitter_native::analyze_twitter_tweet_native,
            twitter_native::download_twitter_profile_native,
            twitter_native::download_twitter_native,
            youtube::get_youtube_stream_url,
            youtube::download_youtube_native
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
