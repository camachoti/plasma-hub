//! Tauri command wrappers for the SQLite message cache and download history.

use crate::env::env;
use plasma_store::{
    download_history_get as download_history_get_impl,
    download_history_save as download_history_save_impl,
    telegram_message_cache_get as telegram_message_cache_get_impl,
    telegram_message_cache_meta as telegram_message_cache_meta_impl,
    telegram_message_cache_save as telegram_message_cache_save_impl,
    telegram_message_cache_shared_media as telegram_message_cache_shared_media_impl,
    topic_download_state_get as topic_download_state_get_impl, MessageCacheGetResult,
    MessageCacheMeta, MessageCacheSaveResult, SharedMediaCacheResult, TopicDownloadState,
};
use tauri::AppHandle;

#[tauri::command]
pub fn topic_download_state_get(
    app: AppHandle,
    chat_id: String,
) -> Result<Vec<TopicDownloadState>, String> {
    topic_download_state_get_impl(&env(&app), chat_id)
}

#[tauri::command]
pub fn download_history_get(app: AppHandle) -> Result<Option<String>, String> {
    download_history_get_impl(&env(&app))
}

#[tauri::command]
pub fn download_history_save(app: AppHandle, history_json: String) -> Result<(), String> {
    download_history_save_impl(&env(&app), history_json)
}

#[tauri::command]
pub fn telegram_message_cache_get(
    app: AppHandle,
    cache_key: String,
) -> Result<MessageCacheGetResult, String> {
    telegram_message_cache_get_impl(&env(&app), cache_key)
}

#[tauri::command]
pub fn telegram_message_cache_meta(
    app: AppHandle,
    cache_key: String,
) -> Result<MessageCacheMeta, String> {
    telegram_message_cache_meta_impl(&env(&app), cache_key)
}

#[tauri::command]
pub fn telegram_message_cache_save(
    app: AppHandle,
    cache_key: String,
    messages: Vec<serde_json::Value>,
    meta: Option<MessageCacheMeta>,
) -> Result<MessageCacheSaveResult, String> {
    telegram_message_cache_save_impl(&env(&app), cache_key, messages, meta)
}

#[tauri::command]
pub fn telegram_message_cache_shared_media(
    app: AppHandle,
    cache_key: String,
    limit: i64,
) -> Result<SharedMediaCacheResult, String> {
    telegram_message_cache_shared_media_impl(&env(&app), cache_key, limit)
}
