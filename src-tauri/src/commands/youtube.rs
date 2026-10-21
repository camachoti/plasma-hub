//! Tauri command wrappers for the native YouTube extractor and downloader.

use crate::env::env;
use tauri::AppHandle;

#[tauri::command]
pub async fn get_youtube_stream_url(
    app_handle: AppHandle,
    url: String,
    format_id: String,
) -> Result<String, String> {
    plasma_youtube::get_youtube_stream_url(&env(&app_handle), url, format_id).await
}

#[tauri::command]
pub async fn download_youtube_native(
    app_handle: AppHandle,
    id: String,
    url: String,
    format_id: String,
    filename: String,
) -> Result<(), String> {
    plasma_youtube::download_youtube_native(&env(&app_handle), id, url, format_id, filename).await
}
