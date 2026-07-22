mod commands;
mod services;

use tauri::Manager;

use commands::filesystem;

use services::telegram as telegram_service;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol(
            "plasma-media",
            telegram_service::plasma_media_protocol,
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .setup(|app| {
            app.manage(telegram_service::TdlibManager::default());
            telegram_service::start_plasma_media_http_server(app.handle().clone())?;

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            filesystem::path_exists,
            filesystem::ensure_dir,
            filesystem::save_download_file,
            filesystem::begin_download_file,
            filesystem::append_download_file_chunk,
            filesystem::finish_download_file,
            filesystem::abort_download_file,
            services::message_cache::telegram_message_cache_get,
            services::message_cache::telegram_message_cache_meta,
            services::message_cache::telegram_message_cache_save,
            services::message_cache::telegram_message_cache_shared_media,
            telegram_service::tdlib_init,
            telegram_service::tdlib_status,
            telegram_service::tdlib_set_phone,
            telegram_service::tdlib_check_code,
            telegram_service::tdlib_check_password,
            telegram_service::tdlib_get_me,
            telegram_service::tdlib_get_chats,
            telegram_service::tdlib_get_messages,
            telegram_service::tdlib_get_shared_media,
            telegram_service::tdlib_search_user_media,
            telegram_service::tdlib_send_message,
            telegram_service::tdlib_send_media,
            telegram_service::tdlib_forward_message,
            telegram_service::tdlib_get_forum_topics,
            telegram_service::telegram_media_prepare_playback,
            telegram_service::telegram_media_ensure_cached,
            telegram_service::telegram_media_save,
            telegram_service::telegram_media_get_meta,
            telegram_service::telegram_media_cancel,
            telegram_service::telegram_media_cache_stats,
            telegram_service::telegram_media_clear_cache,
            telegram_service::telegram_media_evict_cache,
            telegram_service::tdlib_download_message_media,
            telegram_service::tdlib_cache_message_media,
            telegram_service::tdlib_download_message_thumbnail,
            telegram_service::tdlib_download_chat_avatar,
            telegram_service::tdlib_start_mass_download,
            telegram_service::tdlib_stop_download,
            services::twitter::analyze_twitter_profile_native,
            services::twitter::analyze_twitter_tweet_native,
            services::twitter::download_twitter_profile_native,
            services::twitter::download_twitter_native,
            services::youtube::get_youtube_stream_url,
            services::youtube::download_youtube_native
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
