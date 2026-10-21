//! Tauri command wrappers for the native TikTok extractor and downloader.

use crate::env::env;
use plasma_tiktok::TikTokMediaInfo;
use tauri::AppHandle;

#[tauri::command]
pub async fn analyze_tiktok_native(
    app_handle: AppHandle,
    url: String,
) -> Result<TikTokMediaInfo, String> {
    plasma_tiktok::analyze_tiktok_native(&env(&app_handle), url).await
}

#[tauri::command]
pub async fn download_tiktok_native(
    app_handle: AppHandle,
    id: String,
    url: String,
    format_id: String,
    filename: String,
    direct_url: Option<String>,
) -> Result<(), String> {
    plasma_tiktok::download_tiktok_native(
        &env(&app_handle),
        id,
        url,
        format_id,
        filename,
        direct_url,
    )
    .await
}
