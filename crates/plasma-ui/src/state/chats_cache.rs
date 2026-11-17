//! Last known Telegram chat list for instant paint on cold start.

use plasma_store::SettingsStore;
use plasma_telegram::TdlibChatInfo;

pub const CHAT_LIST_CACHE_KEY: &str = "plasma_telegram_chat_list";

pub fn recall_cached_chat_list(settings: &SettingsStore) -> Vec<TdlibChatInfo> {
    settings
        .get_raw(CHAT_LIST_CACHE_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<Vec<TdlibChatInfo>>(&raw).ok())
        .unwrap_or_default()
}

pub fn remember_chat_list(
    settings: &SettingsStore,
    dialogs: &[TdlibChatInfo],
) -> plasma_core::Result<()> {
    let raw = serde_json::to_string(dialogs)?;
    settings.set_raw(CHAT_LIST_CACHE_KEY, &raw)
}

pub fn clear_cached_chat_list(settings: &SettingsStore) {
    let _ = settings.remove_raw(CHAT_LIST_CACHE_KEY);
}
