mod db;
mod downloader;
mod twitter_native;
mod youtube;

use std::fs;
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

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            path_exists,
            ensure_dir,
            save_download_file,
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
