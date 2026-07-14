mod commands;
mod services;

use tauri::Manager;

use commands::filesystem;

use services::telegram as telegram_service;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_http::init())
        .setup(|app| {
            app.manage(telegram_service::TdlibManager::default());

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
            telegram_service::tdlib_init,
            telegram_service::tdlib_status,
            telegram_service::tdlib_set_phone,
            telegram_service::tdlib_check_code,
            telegram_service::tdlib_check_password,
            telegram_service::tdlib_get_me,
            telegram_service::tdlib_download_message_media,
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
