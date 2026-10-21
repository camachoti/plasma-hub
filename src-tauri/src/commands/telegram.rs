//! Tauri command wrappers for `plasma-telegram`.
//!
//! This module is the only place that knows about Tauri's `State` and
//! `AppHandle`. All behaviour lives in the `plasma-telegram` crate.

use crate::env::env;
use plasma_telegram::*;
use tauri::{AppHandle, State};

#[tauri::command]
pub async fn tdlib_init(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    api_id: i32,
    api_hash: String,
) -> TdlibCommandResult {
    plasma_telegram::tdlib_init(&env(&app), state.inner(), api_id, api_hash).await
}

#[tauri::command]
pub async fn tdlib_status(state: State<'_, TdlibManager>) -> TdlibCommandResult {
    plasma_telegram::tdlib_status(state.inner()).await
}

#[tauri::command]
pub async fn tdlib_set_phone(
    state: State<'_, TdlibManager>,
    phone_number: String,
) -> TdlibCommandResult {
    plasma_telegram::tdlib_set_phone(state.inner(), phone_number).await
}

#[tauri::command]
pub async fn tdlib_check_code(state: State<'_, TdlibManager>, code: String) -> TdlibCommandResult {
    plasma_telegram::tdlib_check_code(state.inner(), code).await
}

#[tauri::command]
pub async fn tdlib_check_password(
    state: State<'_, TdlibManager>,
    password: String,
) -> TdlibCommandResult {
    plasma_telegram::tdlib_check_password(state.inner(), password).await
}

#[tauri::command]
pub async fn tdlib_get_me(state: State<'_, TdlibManager>) -> Result<TdlibUserInfo, String> {
    plasma_telegram::tdlib_get_me(state.inner()).await
}

#[tauri::command]
pub async fn tdlib_get_chats(
    state: State<'_, TdlibManager>,
    limit: Option<i32>,
) -> Result<TdlibChatsResult, String> {
    plasma_telegram::tdlib_get_chats(state.inner(), limit).await
}

#[tauri::command]
pub async fn tdlib_read_chat(state: State<'_, TdlibManager>, chat_id: i64) -> Result<(), String> {
    plasma_telegram::tdlib_read_chat(state.inner(), chat_id).await
}

#[tauri::command]
pub async fn tdlib_read_all_chats(state: State<'_, TdlibManager>) -> Result<(), String> {
    plasma_telegram::tdlib_read_all_chats(state.inner()).await
}

#[tauri::command]
pub async fn tdlib_get_messages(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    limit: Option<i32>,
    offset_id: Option<i64>,
    offset: Option<i32>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibMessagesResult, String> {
    plasma_telegram::tdlib_get_messages(
        state.inner(),
        chat_id,
        limit,
        offset_id,
        offset,
        topic_id,
        topic_kind,
    )
    .await
}

#[tauri::command]
pub async fn tdlib_search_chat_messages(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    query: String,
    limit: Option<i32>,
    from_message_id: Option<i64>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibSearchMessagesResult, String> {
    plasma_telegram::tdlib_search_chat_messages(
        state.inner(),
        chat_id,
        query,
        limit,
        from_message_id,
        topic_id,
        topic_kind,
    )
    .await
}

#[tauri::command]
pub async fn tdlib_get_chat_capabilities(
    state: State<'_, TdlibManager>,
    chat_id: i64,
) -> Result<TdlibChatCapabilitiesResult, String> {
    plasma_telegram::tdlib_get_chat_capabilities(state.inner(), chat_id).await
}

#[tauri::command]
pub async fn tdlib_get_shared_media(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    limit: Option<i32>,
) -> Result<TdlibSharedMediaResult, String> {
    plasma_telegram::tdlib_get_shared_media(state.inner(), chat_id, limit).await
}

#[tauri::command]
pub async fn tdlib_search_user_media(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    user_id: String,
    limit: Option<i32>,
) -> Result<TdlibSharedMediaResult, String> {
    plasma_telegram::tdlib_search_user_media(state.inner(), chat_id, user_id, limit).await
}

#[tauri::command]
pub async fn tdlib_send_message(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    text: String,
    reply_to_id: Option<i64>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibSendResult, String> {
    plasma_telegram::tdlib_send_message(
        state.inner(),
        chat_id,
        text,
        reply_to_id,
        topic_id,
        topic_kind,
    )
    .await
}

#[tauri::command]
pub async fn tdlib_send_media(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    file_path: String,
    caption: Option<String>,
    reply_to_id: Option<i64>,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibSendResult, String> {
    plasma_telegram::tdlib_send_media(
        state.inner(),
        chat_id,
        file_path,
        caption,
        reply_to_id,
        topic_id,
        topic_kind,
    )
    .await
}

#[tauri::command]
pub async fn tdlib_forward_message(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    to_chat_id: i64,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
) -> Result<TdlibForwardResult, String> {
    plasma_telegram::tdlib_forward_message(
        state.inner(),
        chat_id,
        message_id,
        to_chat_id,
        topic_id,
        topic_kind,
    )
    .await
}

#[tauri::command]
pub async fn tdlib_get_forum_topics(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    limit: Option<i32>,
) -> Result<TdlibForumTopicsResult, String> {
    plasma_telegram::tdlib_get_forum_topics(state.inner(), chat_id, limit).await
}

#[tauri::command]
pub async fn tdlib_topic_has_new_media(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    topic_id: i64,
    topic_kind: Option<String>,
    after_message_id: i64,
) -> Result<bool, String> {
    plasma_telegram::tdlib_topic_has_new_media(
        state.inner(),
        chat_id,
        topic_id,
        topic_kind,
        after_message_id,
    )
    .await
}

#[tauri::command]
pub async fn telegram_media_get_meta(
    app: AppHandle,
    chat_id: i64,
    message_id: i64,
) -> Result<NativeMediaAssetMeta, String> {
    plasma_telegram::telegram_media_get_meta(&env(&app), chat_id, message_id).await
}

#[tauri::command]
pub async fn telegram_media_ensure_cached(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    priority: Option<String>,
) -> Result<NativeMediaAssetMeta, String> {
    plasma_telegram::telegram_media_ensure_cached(
        &env(&app),
        state.inner(),
        chat_id,
        message_id,
        priority,
    )
    .await
}

#[tauri::command]
pub async fn telegram_media_prepare_playback(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    mode: Option<String>,
) -> Result<NativePlaybackSource, String> {
    plasma_telegram::telegram_media_prepare_playback(
        &env(&app),
        state.inner(),
        chat_id,
        message_id,
        mode,
    )
    .await
}

#[tauri::command]
pub async fn telegram_media_save(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    destination_path: Option<String>,
) -> Result<NativeSavedMedia, String> {
    plasma_telegram::telegram_media_save(
        &env(&app),
        state.inner(),
        chat_id,
        message_id,
        destination_path,
    )
    .await
}

#[tauri::command]
pub async fn telegram_media_cancel(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
) -> Result<bool, String> {
    plasma_telegram::telegram_media_cancel(state.inner(), chat_id, message_id).await
}

#[tauri::command]
pub async fn telegram_media_cache_stats(app: AppHandle) -> Result<NativeMediaCacheStats, String> {
    plasma_telegram::telegram_media_cache_stats(&env(&app)).await
}

#[tauri::command]
pub async fn telegram_media_clear_cache(app: AppHandle) -> Result<NativeMediaCacheStats, String> {
    plasma_telegram::telegram_media_clear_cache(&env(&app)).await
}

#[tauri::command]
pub async fn telegram_media_evict_cache(
    app: AppHandle,
    max_cache_size: i64,
) -> Result<NativeMediaCacheStats, String> {
    plasma_telegram::telegram_media_evict_cache(&env(&app), max_cache_size).await
}

#[tauri::command]
pub async fn tdlib_download_message_media(
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
    folder_path: String,
) -> Result<TdlibDownloadResult, String> {
    plasma_telegram::tdlib_download_message_media(state.inner(), chat_id, message_id, folder_path)
        .await
}

#[tauri::command]
pub async fn tdlib_cache_message_media(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
) -> Result<TdlibDownloadResult, String> {
    plasma_telegram::tdlib_cache_message_media(&env(&app), state.inner(), chat_id, message_id).await
}

#[tauri::command]
pub async fn tdlib_download_message_thumbnail(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
    message_id: i64,
) -> Result<TdlibThumbnailResult, String> {
    plasma_telegram::tdlib_download_message_thumbnail(
        &env(&app),
        state.inner(),
        chat_id,
        message_id,
    )
    .await
}

#[tauri::command]
pub async fn tdlib_download_chat_avatar(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    chat_id: i64,
) -> Result<TdlibAvatarResult, String> {
    plasma_telegram::tdlib_download_chat_avatar(&env(&app), state.inner(), chat_id).await
}

#[tauri::command]
pub async fn tdlib_stop_download(state: State<'_, TdlibManager>) -> Result<(), String> {
    plasma_telegram::tdlib_stop_download(state.inner()).await
}

#[tauri::command]
pub async fn tdlib_start_mass_download(
    app: AppHandle,
    state: State<'_, TdlibManager>,
    request: TdlibMassDownloadRequest,
) -> Result<TdlibMassDownloadResult, String> {
    plasma_telegram::tdlib_start_mass_download(&env(&app), state.inner(), request).await
}
