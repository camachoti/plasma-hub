mod commands;
mod env;
#[cfg(target_os = "linux")]
mod fontconfig_guard;

use plasma_core::AppEnv;
use plasma_telegram::TdlibManager;
use tauri::Manager;

/// Adapts the existing `plasma-media://` URI scheme to the UI-agnostic media
/// responder implemented by `plasma-telegram`.
fn plasma_media_protocol<R: tauri::Runtime>(
    ctx: tauri::UriSchemeContext<'_, R>,
    request: http::Request<Vec<u8>>,
    responder: tauri::UriSchemeResponder,
) {
    let env = ctx.app_handle().state::<AppEnv>().inner().clone();
    std::thread::spawn(move || {
        let response =
            plasma_telegram::get_plasma_media_response(&env, request).unwrap_or_else(|error| {
                println!("[plasma-media] internal error={error}");
                plasma_telegram::media_protocol_error(
                    http::StatusCode::INTERNAL_SERVER_ERROR,
                    &error,
                )
            });
        responder.respond(response);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    let _fontconfig_guard = fontconfig_guard::install_if_needed();

    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("plasma-media", plasma_media_protocol)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .setup(|app| {
            let env = env::build_env(app.handle())?;
            app.manage(env.clone());
            app.manage(plasma_store::SettingsStore::open(&env)?);
            app.manage(TdlibManager::default());
            plasma_telegram::start_plasma_media_http_server(env)?;

            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::filesystem::path_exists,
            commands::filesystem::ensure_dir,
            commands::filesystem::save_download_file,
            commands::filesystem::begin_download_file,
            commands::filesystem::append_download_file_chunk,
            commands::filesystem::finish_download_file,
            commands::filesystem::abort_download_file,
            commands::filesystem::download_url_to_file,
            commands::filesystem::generate_video_thumbnail,
            commands::filesystem::load_twitter_cookies,
            commands::filesystem::save_twitter_cookies,
            commands::duplicate_search::search_duplicate_media,
            commands::cache::telegram_message_cache_get,
            commands::cache::telegram_message_cache_meta,
            commands::cache::telegram_message_cache_save,
            commands::cache::telegram_message_cache_shared_media,
            commands::cache::download_history_get,
            commands::cache::download_history_save,
            commands::cache::topic_download_state_get,
            commands::settings::settings_get,
            commands::settings::settings_set,
            commands::settings::settings_remove,
            commands::settings::settings_snapshot,
            commands::settings::settings_import_legacy,
            commands::telegram::tdlib_init,
            commands::telegram::tdlib_status,
            commands::telegram::tdlib_set_phone,
            commands::telegram::tdlib_check_code,
            commands::telegram::tdlib_check_password,
            commands::telegram::tdlib_get_me,
            commands::telegram::tdlib_get_chats,
            commands::telegram::tdlib_read_chat,
            commands::telegram::tdlib_read_all_chats,
            commands::telegram::tdlib_get_messages,
            commands::telegram::tdlib_search_chat_messages,
            commands::telegram::tdlib_get_chat_capabilities,
            commands::telegram::tdlib_get_shared_media,
            commands::telegram::tdlib_search_user_media,
            commands::telegram::tdlib_send_message,
            commands::telegram::tdlib_send_media,
            commands::telegram::tdlib_forward_message,
            commands::telegram::tdlib_get_forum_topics,
            commands::telegram::tdlib_topic_has_new_media,
            commands::telegram::telegram_media_prepare_playback,
            commands::telegram::telegram_media_ensure_cached,
            commands::telegram::telegram_media_save,
            commands::telegram::telegram_media_get_meta,
            commands::telegram::telegram_media_cancel,
            commands::telegram::telegram_media_cache_stats,
            commands::telegram::telegram_media_clear_cache,
            commands::telegram::telegram_media_evict_cache,
            commands::telegram::tdlib_download_message_media,
            commands::telegram::tdlib_cache_message_media,
            commands::telegram::tdlib_download_message_thumbnail,
            commands::telegram::tdlib_download_chat_avatar,
            commands::telegram::tdlib_start_mass_download,
            commands::telegram::tdlib_stop_download,
            commands::downloads::cancel_native_download,
            commands::tiktok::analyze_tiktok_native,
            commands::tiktok::download_tiktok_native,
            commands::twitter::analyze_twitter_profile_native,
            commands::twitter::analyze_twitter_tweet_native,
            commands::twitter::download_twitter_profile_native,
            commands::twitter::download_twitter_native,
            commands::youtube::get_youtube_stream_url,
            commands::youtube::download_youtube_native
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
