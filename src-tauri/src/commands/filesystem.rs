//! Tauri command wrappers for filesystem, download and media helpers.

use crate::env::env;
use std::collections::HashMap;
use tauri::AppHandle;

#[tauri::command]
pub fn path_exists(path: String) -> bool {
    plasma_platform::path_exists(&path)
}

#[tauri::command]
pub fn ensure_dir(path: String) -> Result<(), String> {
    plasma_platform::ensure_dir(&path)
}

#[tauri::command]
pub fn save_download_file(file_path: String, data: Vec<u8>) -> Result<(), String> {
    plasma_download::save_download_file(&file_path, &data)
}

#[tauri::command]
pub fn begin_download_file(file_path: String) -> Result<(), String> {
    plasma_download::begin_download_file(&file_path)
}

#[tauri::command]
pub fn append_download_file_chunk(file_path: String, data: Vec<u8>) -> Result<(), String> {
    plasma_download::append_download_file_chunk(&file_path, &data)
}

#[tauri::command]
pub fn finish_download_file(file_path: String) -> Result<(), String> {
    plasma_download::finish_download_file(&file_path)
}

#[tauri::command]
pub fn abort_download_file(file_path: String) -> Result<(), String> {
    plasma_download::abort_download_file(&file_path)
}

#[tauri::command]
pub async fn download_url_to_file(
    app: AppHandle,
    id: String,
    url: String,
    file_path: String,
    headers: Option<HashMap<String, String>>,
) -> Result<(), String> {
    plasma_download::download_url_to_file(&env(&app), &id, &url, &file_path, headers).await
}

#[tauri::command]
pub fn generate_video_thumbnail(app: AppHandle, file_path: String) -> Result<String, String> {
    plasma_media::generate_video_thumbnail(&env(&app), &file_path)
}

#[tauri::command]
pub fn load_twitter_cookies(app: AppHandle) -> Result<String, String> {
    plasma_twitter::load_twitter_cookies(&env(&app))
}

#[tauri::command]
pub fn save_twitter_cookies(app: AppHandle, cookies: String) -> Result<(), String> {
    plasma_twitter::save_twitter_cookies(&env(&app), &cookies)
}
