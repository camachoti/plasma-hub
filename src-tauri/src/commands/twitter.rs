//! Tauri command wrappers for the native Twitter/X analyzer and downloader.

use plasma_twitter::{NativeMediaInfo, NativeProfileInfo};

#[tauri::command]
pub async fn analyze_twitter_profile_native(
    url: String,
    cookies: Option<String>,
) -> Result<NativeProfileInfo, String> {
    plasma_twitter::analyze_twitter_profile_native(url, cookies).await
}

#[tauri::command]
pub async fn analyze_twitter_tweet_native(
    url: String,
    cookies: Option<String>,
) -> Result<NativeMediaInfo, String> {
    plasma_twitter::analyze_twitter_tweet_native(url, cookies).await
}

#[tauri::command]
pub async fn download_twitter_native(
    app_handle: tauri::AppHandle,
    id: String,
    url: String,
    filename: String,
    cookies: Option<String>,
) -> Result<(), String> {
    let env = crate::env::env(&app_handle);
    plasma_twitter::download_twitter_native(&env, id, url, filename, cookies).await
}

#[tauri::command]
pub async fn download_twitter_profile_native(
    app_handle: tauri::AppHandle,
    id: String,
    username: String,
    media_urls: Vec<String>,
    cookies: Option<String>,
) -> Result<(), String> {
    let env = crate::env::env(&app_handle);
    plasma_twitter::download_twitter_profile_native(&env, id, username, media_urls, cookies).await
}
