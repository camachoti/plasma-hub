//! Shared UI state and services.

use crate::context::ContextMenuState;
use crate::downloads::{DownloadHandle, DownloadRecord};
use crate::icons::IconName;
use crate::theme::{Density, Palette};
use crate::twitter::TwitterHandle;
use crate::views::select::SelectPortal;
use base64::Engine as _;
use dioxus::prelude::*;
use plasma_core::{AppEnv, EventBus};
use plasma_duplicates::{SearchMatch, SearchMode, SearchResult, SearchSource};
use plasma_extract::{DownloadSpec, MediaInfo, Platform};
use plasma_store::{MessageCacheGetResult, MessageCacheMeta, SettingsStore};
use plasma_telegram::{
    NativeMediaAssetMeta, NativeMediaCacheStats, NativePlaybackSource, NativeSavedMedia,
    TdlibChatCapabilitiesResult, TdlibChatInfo, TdlibChatsResult, TdlibCommandResult,
    TdlibDownloadItem, TdlibForumTopicInfo, TdlibForumTopicsResult, TdlibForwardResult,
    TdlibGlobalSearchResult, TdlibManager, TdlibMassDownloadRequest, TdlibMassDownloadResult,
    TdlibMessageInfo, TdlibMessagesResult, TdlibSearchMessagesResult, TdlibSendResult,
    TdlibSharedMediaResult, TdlibStatus, TdlibUserInfo,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Telegram API credentials embedded by `apps/plasma/build.rs`.
#[derive(Clone, Debug)]
pub struct TelegramCredentials {
    pub api_id: i32,
    pub api_hash: String,
}

/// Runtime services provided by the composition root.
#[derive(Clone)]
pub struct AppServices {
    pub env: AppEnv,
    pub settings: SettingsStore,
    pub events: EventBus,
    pub telegram: TelegramHandle,
    pub downloads: DownloadHandle,
    pub twitter: TwitterHandle,
    pub extract: ExtractorHandle,
    pub credentials: Option<TelegramCredentials>,
    /// URL received from an Android share intent before the UI mounted.
    pub initial_shared_url: Option<String>,
}

impl AppServices {
    pub fn new(
        env: AppEnv,
        settings: SettingsStore,
        events: EventBus,
        telegram: TelegramHandle,
        credentials: Option<TelegramCredentials>,
    ) -> Self {
        let downloads = DownloadHandle::new(env.clone(), telegram.runtime_handle());
        let twitter = TwitterHandle::new(env.clone(), telegram.runtime_handle());
        let extract = ExtractorHandle::new(env.clone(), telegram.runtime_handle(), twitter.clone());
        Self {
            env,
            settings,
            events,
            telegram,
            downloads,
            twitter,
            extract,
            credentials,
            initial_shared_url: None,
        }
    }

    /// Applies the cache limit saved in settings, if one is configured.
    ///
    /// The limit used to be applied only when the user changed the setting, so
    /// new media could grow past it. Call this after saves and at startup.
    pub async fn enforce_media_cache_limit(&self) -> Result<Option<NativeMediaCacheStats>, String> {
        let limit = self
            .settings
            .get(plasma_store::SettingKey::MEDIA_CACHE_LIMIT)
            .ok()
            .flatten()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0);
        if limit <= 0 {
            return Ok(None);
        }
        self.telegram.evict_media_cache(limit).await.map(Some)
    }
}

/// Async facade over the native platform extractors.
#[derive(Clone)]
pub struct ExtractorHandle {
    env: AppEnv,
    runtime: tokio::runtime::Handle,
    twitter: TwitterHandle,
}

impl ExtractorHandle {
    pub fn new(env: AppEnv, runtime: tokio::runtime::Handle, twitter: TwitterHandle) -> Self {
        Self {
            env,
            runtime,
            twitter,
        }
    }

    pub async fn analyze(&self, url: String) -> Result<MediaInfo, String> {
        let cookies = self.cookies_for(&url).await;
        let env = self.env.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_extract::analyze_url(&env, &url, cookies).await });
        self.join(handle).await?
    }

    pub async fn download(
        &self,
        id: String,
        original_url: String,
        format_id: String,
        direct_url: Option<String>,
        file_name: String,
        has_audio: bool,
    ) -> Result<(), String> {
        let cookies = self.cookies_for(&original_url).await;
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_extract::download_format(
                &env,
                DownloadSpec {
                    id,
                    original_url,
                    format_id,
                    direct_url,
                    file_name,
                    has_audio,
                    cookies,
                },
            )
            .await
        });
        self.join(handle).await?
    }

    async fn cookies_for(&self, url: &str) -> Option<String> {
        if Platform::detect(url) != Platform::Twitter {
            return None;
        }
        self.twitter
            .load_cookies()
            .await
            .ok()
            .filter(|cookies| !cookies.trim().is_empty())
    }

    async fn join<T>(&self, handle: tokio::task::JoinHandle<T>) -> Result<T, String> {
        handle
            .await
            .map_err(|error| format!("Tarefa do extrator falhou: {error}"))
    }
}

/// Async facade over `plasma-telegram`.
///
/// TDLib needs a Tokio runtime, while Dioxus runs its own executor. Every call
/// is dispatched to the Tokio runtime and awaited without blocking the UI.
#[derive(Clone)]
pub struct TelegramHandle {
    manager: Arc<TdlibManager>,
    env: AppEnv,
    runtime: tokio::runtime::Handle,
}

impl TelegramHandle {
    pub fn new(env: AppEnv, runtime: tokio::runtime::Handle) -> Self {
        Self {
            manager: Arc::new(TdlibManager::default()),
            env,
            runtime,
        }
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.runtime.clone()
    }

    async fn join<T>(&self, handle: tokio::task::JoinHandle<T>) -> Result<T, String> {
        handle
            .await
            .map_err(|error| format!("Tarefa do Telegram falhou: {error}"))
    }

    pub async fn init(&self, api_id: i32, api_hash: String) -> TdlibCommandResult {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_init(&env, &manager, api_id, api_hash).await
        });
        self.join(handle).await?
    }

    pub async fn status(&self) -> TdlibCommandResult {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_status(&manager).await });
        self.join(handle).await?
    }

    pub async fn set_phone(&self, phone_number: String) -> TdlibCommandResult {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_set_phone(&manager, phone_number).await });
        self.join(handle).await?
    }

    pub async fn check_code(&self, code: String) -> TdlibCommandResult {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_check_code(&manager, code).await });
        self.join(handle).await?
    }

    pub async fn check_password(&self, password: String) -> TdlibCommandResult {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_check_password(&manager, password).await });
        self.join(handle).await?
    }

    pub async fn get_me(&self) -> Result<TdlibUserInfo, String> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_get_me(&manager).await });
        self.join(handle).await?
    }

    pub async fn get_chats(&self, limit: i32) -> Result<TdlibChatsResult, String> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_get_chats(&manager, Some(limit)).await });
        self.join(handle).await?
    }

    /// Fetches one message, used to resolve replies whose original is outside
    /// the loaded timeline page.
    pub async fn get_message(
        &self,
        chat_id: i64,
        message_id: i64,
    ) -> Result<TdlibMessageInfo, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_get_message(&manager, chat_id, message_id).await
        });
        self.join(handle).await?
    }

    pub async fn get_messages(
        &self,
        chat_id: i64,
        limit: i32,
        offset_id: i64,
        offset: i32,
        topic_id: Option<i64>,
        topic_kind: Option<String>,
    ) -> Result<TdlibMessagesResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_get_messages(
                &manager,
                chat_id,
                Some(limit),
                Some(offset_id),
                Some(offset),
                topic_id,
                topic_kind,
            )
            .await
        });
        self.join(handle).await?
    }

    pub async fn search_chat_messages(
        &self,
        chat_id: i64,
        query: String,
        limit: i32,
        from_message_id: Option<i64>,
        topic_id: Option<i64>,
        topic_kind: Option<String>,
    ) -> Result<TdlibSearchMessagesResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_search_chat_messages(
                &manager,
                chat_id,
                query,
                Some(limit),
                from_message_id,
                topic_id,
                topic_kind,
            )
            .await
        });
        self.join(handle).await?
    }

    pub async fn search_messages(
        &self,
        query: String,
        limit: i32,
        offset: Option<String>,
    ) -> Result<TdlibGlobalSearchResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_search_messages(&manager, query, Some(limit), offset).await
        });
        self.join(handle).await?
    }

    pub async fn get_forum_topics(
        &self,
        chat_id: i64,
        limit: i32,
    ) -> Result<TdlibForumTopicsResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_get_forum_topics(&manager, chat_id, Some(limit)).await
        });
        self.join(handle).await?
    }

    pub async fn read_chat(&self, chat_id: i64) -> Result<(), String> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_read_chat(&manager, chat_id).await });
        self.join(handle).await?
    }

    pub async fn capabilities(&self, chat_id: i64) -> Result<TdlibChatCapabilitiesResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_get_chat_capabilities(&manager, chat_id).await
        });
        self.join(handle).await?
    }

    pub async fn send_message(
        &self,
        chat_id: i64,
        text: String,
        reply_to_id: Option<i64>,
        topic_id: Option<i64>,
        topic_kind: Option<String>,
    ) -> Result<TdlibSendResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_send_message(
                &manager,
                chat_id,
                text,
                reply_to_id,
                topic_id,
                topic_kind,
            )
            .await
        });
        self.join(handle).await?
    }

    pub async fn send_media(
        &self,
        chat_id: i64,
        file_path: String,
        caption: Option<String>,
        reply_to_id: Option<i64>,
        topic_id: Option<i64>,
        topic_kind: Option<String>,
    ) -> Result<TdlibSendResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_send_media(
                &manager,
                chat_id,
                file_path,
                caption,
                reply_to_id,
                topic_id,
                topic_kind,
            )
            .await
        });
        self.join(handle).await?
    }

    pub async fn forward_message(
        &self,
        chat_id: i64,
        message_id: i64,
        to_chat_id: i64,
        topic_id: Option<i64>,
        topic_kind: Option<String>,
    ) -> Result<TdlibForwardResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_forward_message(
                &manager, chat_id, message_id, to_chat_id, topic_id, topic_kind,
            )
            .await
        });
        self.join(handle).await?
    }

    pub async fn shared_media(
        &self,
        chat_id: i64,
        limit: i32,
    ) -> Result<TdlibSharedMediaResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_get_shared_media(&manager, chat_id, Some(limit)).await
        });
        self.join(handle).await?
    }

    /// Photos and videos sent by one user inside a conversation.
    pub async fn user_media(
        &self,
        chat_id: i64,
        sender_id: String,
        limit: i32,
    ) -> Result<TdlibSharedMediaResult, String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_search_user_media(&manager, chat_id, sender_id, Some(limit))
                .await
        });
        self.join(handle).await?
    }

    /// Adds or removes an emoji reaction. Returns an error message on failure.
    pub async fn toggle_reaction(
        &self,
        chat_id: i64,
        message_id: i64,
        emoji: String,
        add: bool,
    ) -> Result<(), String> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_toggle_message_reaction(
                &manager, chat_id, message_id, emoji, add,
            )
            .await
        });
        let result = self.join(handle).await??;
        if result.success {
            Ok(())
        } else {
            Err(result
                .error
                .unwrap_or_else(|| "Não foi possível reagir.".to_string()))
        }
    }

    pub async fn thumbnail_data_url(
        &self,
        chat_id: i64,
        message_id: i64,
    ) -> Result<Option<String>, String> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_download_message_thumbnail(&env, &manager, chat_id, message_id)
                .await
        });
        let result = self.join(handle).await??;
        if !result.success {
            return Ok(None);
        }
        let Some(path) = result.file_path else {
            return Ok(None);
        };
        read_path_data_url(PathBuf::from(path)).await.map(Some)
    }

    /// Builds a small data URL for a media file dragged into the composer.
    /// Videos use the cached poster frame; images are capped so a huge photo
    /// does not turn the preview modal into a multi-megabyte data URL.
    pub async fn media_preview_data_url(
        &self,
        file_path: String,
        is_video: bool,
    ) -> Result<Option<String>, String> {
        if is_video {
            let env = self.env.clone();
            let handle = self.runtime.spawn(async move {
                let poster = tokio::task::spawn_blocking(move || {
                    plasma_media::generate_video_thumbnail(&env, &file_path)
                })
                .await
                .map_err(|error| format!("Miniatura do vídeo falhou: {error}"))??;
                read_path_data_url(PathBuf::from(poster)).await.map(Some)
            });
            return self.join(handle).await?;
        }

        let path = PathBuf::from(file_path);
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        // WebKitGTK has no HEIC decoder; show the generic media tile instead of
        // a broken image in the drop preview.
        if matches!(extension.as_str(), "heic" | "heif") {
            return Ok(None);
        }
        let size_path = path.clone();
        let size = tokio::task::spawn_blocking(move || {
            std::fs::metadata(size_path).map(|metadata| metadata.len())
        })
        .await
        .map_err(|error| format!("Leitura do arquivo falhou: {error}"))?
        .map_err(|error| format!("Leitura do arquivo falhou: {error}"))?;
        if size > 12 * 1024 * 1024 {
            return Ok(None);
        }
        read_path_data_url(path).await.map(Some)
    }

    pub async fn prepare_playback(
        &self,
        chat_id: i64,
        message_id: i64,
    ) -> Result<NativePlaybackSource, String> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::telegram_media_prepare_playback(
                &env, &manager, chat_id, message_id, None,
            )
            .await
        });
        self.join(handle).await?
    }

    pub async fn save_media(
        &self,
        chat_id: i64,
        message_id: i64,
        destination_path: Option<String>,
    ) -> Result<NativeSavedMedia, String> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::telegram_media_save(
                &env,
                &manager,
                chat_id,
                message_id,
                destination_path,
            )
            .await
        });
        self.join(handle).await?
    }

    pub async fn start_mass_download(
        &self,
        request: TdlibMassDownloadRequest,
    ) -> Result<TdlibMassDownloadResult, String> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_start_mass_download(&env, &manager, request).await
        });
        self.join(handle).await?
    }

    pub async fn stop_mass_download(&self) -> Result<(), String> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_stop_download(&manager).await });
        self.join(handle).await?
    }

    /// Makes sure the avatar of a chat is available on disk.
    ///
    /// The file is cached under the app cache directory, so the second call
    /// for the same chat does not touch TDLib again.
    pub async fn ensure_chat_avatar(&self, chat_id: i64) -> Result<Option<String>, String> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_download_chat_avatar(&env, &manager, chat_id).await
        });
        let result = self.join(handle).await??;
        if !result.success {
            return Ok(None);
        }
        Ok(result.file_path)
    }

    /// URL of the avatar of a chat, served by the local media server.
    pub fn chat_avatar_url(&self, chat_id: i64) -> Option<String> {
        plasma_telegram::cached_chat_avatar(&self.env, chat_id)?;
        let url = plasma_telegram::chat_avatar_url(chat_id);
        // The local media server is the only transport that can serve the
        // file; without it the row keeps the letter placeholder.
        url.starts_with("http://").then_some(url)
    }

    /// Ensures the media of a message is cached and returns its metadata.
    pub async fn ensure_media_cached(
        &self,
        chat_id: i64,
        message_id: i64,
    ) -> Result<NativeMediaAssetMeta, String> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::telegram_media_ensure_cached(
                &env,
                &manager,
                chat_id,
                message_id,
                Some("user".to_string()),
            )
            .await
        });
        self.join(handle).await?
    }

    /// Searches copies of the given media inside a folder.
    pub async fn search_duplicate_media(
        &self,
        run_id: String,
        folder_path: String,
        sources: Vec<SearchSource>,
        mode: Option<SearchMode>,
    ) -> Result<SearchResult, String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_duplicates::search_duplicate_media(&env, run_id, folder_path, sources, mode)
                .await
        });
        self.join(handle).await?
    }

    /// Cancels a running duplicate search.
    pub fn cancel_duplicate_search(&self, run_id: &str) -> bool {
        plasma_download::cancel_download(&format!("duplicate-search-{run_id}"))
    }

    /// Size and item count of the native Telegram media cache.
    pub async fn media_cache_stats(&self) -> Result<NativeMediaCacheStats, String> {
        let env = self.env.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::telegram_media_cache_stats(&env).await });
        self.join(handle).await?
    }

    /// Removes every file of the native Telegram media cache.
    pub async fn clear_media_cache(&self) -> Result<NativeMediaCacheStats, String> {
        let env = self.env.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::telegram_media_clear_cache(&env).await });
        self.join(handle).await?
    }

    /// Evicts the oldest media until the cache fits the given size in bytes.
    pub async fn evict_media_cache(&self, max_size: i64) -> Result<NativeMediaCacheStats, String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::telegram_media_evict_cache(&env, max_size).await
        });
        self.join(handle).await?
    }

    pub async fn download_avatar(&self, chat_id: i64) -> Result<Option<String>, String> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_download_chat_avatar(&env, &manager, chat_id).await
        });
        let result = self.join(handle).await??;
        if !result.success {
            return Ok(None);
        }
        let Some(path) = result.file_path else {
            return Ok(None);
        };

        read_path_data_url(PathBuf::from(path)).await.map(Some)
    }

    pub async fn cached_messages(
        &self,
        cache_key: String,
    ) -> Result<MessageCacheGetResult, String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            tokio::task::spawn_blocking(move || {
                plasma_store::telegram_message_cache_get(&env, cache_key)
            })
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
        });
        self.join(handle).await?
    }

    pub async fn mark_messages_deleted(
        &self,
        chat_id: String,
        message_ids: Vec<i64>,
    ) -> Result<(), String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            tokio::task::spawn_blocking(move || {
                plasma_store::telegram_message_cache_mark_deleted(&env, &chat_id, &message_ids)
            })
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
        });
        self.join(handle).await??;
        Ok(())
    }

    pub async fn cache_messages(
        &self,
        cache_key: String,
        messages: Vec<TdlibMessageInfo>,
    ) -> Result<(), String> {
        let values = messages
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            tokio::task::spawn_blocking(move || {
                plasma_store::telegram_message_cache_save(
                    &env,
                    cache_key,
                    values,
                    Some(MessageCacheMeta {
                        last_fetched_at: Some(now_millis()),
                    }),
                )
            })
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
        });
        self.join(handle).await??;
        Ok(())
    }
}

async fn read_path_data_url(path: PathBuf) -> Result<String, String> {
    let mime = avatar_mime(&path);
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(path))
        .await
        .map_err(|error| format!("Leitura do arquivo falhou: {error}"))?
        .map_err(|error| format!("Leitura do arquivo falhou: {error}"))?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
    Ok(format!("data:{mime};base64,{encoded}"))
}

fn avatar_mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "avif" => "image/avif",
        "jpg" | "jpeg" => "image/jpeg",
        _ => "image/jpeg",
    }
}

pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

/// Top-level app sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Telegram,
    Downloads,
    Twitter,
    Settings,
}

impl Tab {
    pub const ALL: [Self; 4] = [
        Self::Telegram,
        Self::Downloads,
        Self::Twitter,
        Self::Settings,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Telegram => "Telegram",
            Self::Downloads => "Downloads",
            Self::Twitter => "Twitter/X",
            Self::Settings => "Ajustes",
        }
    }

    pub const fn rail_icon(self) -> IconName {
        match self {
            Self::Telegram => IconName::BrandTelegram,
            Self::Downloads => IconName::ProgressDown,
            Self::Twitter => IconName::BrandTwitter,
            Self::Settings => IconName::Settings,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

impl ToastKind {
    pub const fn class(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Error => "error",
        }
    }
}

static NEXT_TOAST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
}

impl Toast {
    pub fn new(message: impl Into<String>, kind: ToastKind) -> Self {
        Self {
            id: NEXT_TOAST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            message: message.into(),
            kind,
        }
    }
}

/// Progress of the duplicate media search.
#[derive(Clone, Debug, PartialEq)]
pub struct DuplicateSearchProgress {
    pub run_id: String,
    pub scanned: u64,
    pub analyzed: u64,
    pub matches: Vec<SearchMatch>,
    pub unreadable: u64,
}

/// Progress emitted by a Telegram mass download.
#[derive(Clone, Debug, PartialEq)]
pub struct MassDownloadProgress {
    pub chat_id: i64,
    pub total: usize,
    pub downloaded: f64,
    pub current_file: String,
    pub is_scanning: bool,
    pub items: Vec<TdlibDownloadItem>,
}

/// Message selected for forwarding.
#[derive(Clone, PartialEq)]
pub struct ForwardState {
    pub source_chat_id: i64,
    pub message: TdlibMessageInfo,
}

/// Emoji picker opened from a message reaction control.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReactionPickerState {
    pub chat_id: i64,
    pub message_id: i64,
    pub x: f64,
    pub y: f64,
}

/// One item in an album opened in the lightbox.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightboxMedia {
    pub message_id: i64,
    pub is_video: bool,
    pub is_photo: bool,
}

impl LightboxMedia {
    pub fn from_message(message: &TdlibMessageInfo) -> Self {
        Self {
            message_id: message.id,
            is_video: message.is_video,
            is_photo: message.is_photo,
        }
    }
}

/// Media opened in the lightbox.
#[derive(Clone, PartialEq)]
pub struct LightboxState {
    pub chat_id: i64,
    pub message_id: i64,
    pub is_video: bool,
    pub is_photo: bool,
    pub title: String,
    /// Every media item in the album, in timeline order. Empty for a single
    /// message. When non-empty, `message_id` identifies the active item.
    pub album: Vec<LightboxMedia>,
}

impl LightboxState {
    pub fn from_message(chat_id: i64, message: &TdlibMessageInfo) -> Self {
        Self {
            chat_id,
            message_id: message.id,
            is_video: message.is_video,
            is_photo: message.is_photo,
            title: message.sender_name.clone(),
            album: Vec::new(),
        }
    }

    pub fn from_album(chat_id: i64, message: &TdlibMessageInfo, album: Vec<LightboxMedia>) -> Self {
        Self {
            album,
            ..Self::from_message(chat_id, message)
        }
    }

    /// Position of the active media in the album, as `(current, total)`.
    pub fn album_position(&self) -> Option<(usize, usize)> {
        let index = self.album_index()?;
        Some((index + 1, self.album.len()))
    }

    pub fn with_album_index(&self, index: usize) -> Option<Self> {
        let item = self.album.get(index)?;
        Some(Self {
            chat_id: self.chat_id,
            message_id: item.message_id,
            is_video: item.is_video,
            is_photo: item.is_photo,
            title: self.title.clone(),
            album: self.album.clone(),
        })
    }

    pub fn next_album(&self) -> Option<Self> {
        let index = self.album_index()?;
        self.with_album_index((index + 1) % self.album.len())
    }

    pub fn previous_album(&self) -> Option<Self> {
        let index = self.album_index()?;
        self.with_album_index((index + self.album.len() - 1) % self.album.len())
    }

    fn album_index(&self) -> Option<usize> {
        if self.album.len() <= 1 {
            return None;
        }
        self.album
            .iter()
            .position(|item| item.message_id == self.message_id)
    }
}

/// User whose sent media is being browsed from the timeline context menu.
#[derive(Clone, Debug, PartialEq)]
pub struct UserMediaState {
    pub chat_id: i64,
    pub sender_id: String,
    pub sender_name: String,
}

impl UserMediaState {
    /// Name shown in the panel header, with a neutral fallback when TDLib
    /// could not resolve the sender.
    pub fn display_name(&self) -> &str {
        if self.sender_name.trim().is_empty() {
            "este usuário"
        } else {
            self.sender_name.as_str()
        }
    }
}

/// Signal bundle shared through context.
#[derive(Clone, Copy)]
pub struct UiState {
    pub tab: Signal<Tab>,
    pub palette: Signal<Palette>,
    pub density: Signal<Density>,
    pub logged_in: Signal<bool>,
    pub show_spike: Signal<bool>,
    pub show_palette: Signal<bool>,
    pub toasts: Signal<Vec<Toast>>,
    /// Option list of the select currently open, rendered at the app root.
    pub select_portal: Signal<Option<SelectPortal>>,

    pub auth_state: Signal<String>,
    pub auth_error: Signal<Option<String>>,
    pub auth_user: Signal<Option<String>>,
    pub booting: Signal<bool>,
    pub preview_mode: Signal<bool>,
    pub avatar: Signal<Option<String>>,

    pub chats: Signal<Vec<TdlibChatInfo>>,
    pub chats_loading: Signal<bool>,
    pub chats_error: Signal<Option<String>>,
    pub selected_chat: Signal<Option<TdlibChatInfo>>,

    pub messages: Signal<Vec<TdlibMessageInfo>>,
    pub messages_loading: Signal<bool>,
    pub messages_error: Signal<Option<String>>,
    pub messages_has_more: Signal<bool>,
    /// Cache key of the conversation currently rendered in the timeline.
    pub messages_key: Signal<Option<String>>,

    pub topics: Signal<Vec<TdlibForumTopicInfo>>,
    pub selected_topic: Signal<Option<TdlibForumTopicInfo>>,
    /// Hidden topic keys (`kind:id`) of the selected chat.
    pub hidden_topics: Signal<Vec<String>>,
    /// In-memory thumbnails (`(chat_id, message_id)` -> data URL) so rows that
    /// leave and re-enter the virtualized window do not flash the placeholder.
    pub thumbnails: Signal<std::collections::HashMap<(i64, i64), String>>,
    /// Whether the topic list dialog is open.
    pub topic_browser_open: Signal<bool>,

    pub search_results: Signal<Vec<TdlibMessageInfo>>,
    pub search_loading: Signal<bool>,
    pub search_error: Signal<Option<String>>,

    pub telegram_tick: Signal<u64>,
    /// Bumped only for messages/deletions in the open conversation. The chat
    /// list keeps using `telegram_tick`, so unrelated chats no longer force a
    /// TDLib reload of the visible timeline.
    pub message_tick: Signal<u64>,

    pub downloads: Signal<Vec<DownloadRecord>>,

    pub mass_download_running: Signal<bool>,
    pub mass_download_stopping: Signal<bool>,
    pub mass_download_progress: Signal<Option<MassDownloadProgress>>,
    pub mass_download_open: Signal<bool>,
    pub duplicate_search_progress: Signal<Option<DuplicateSearchProgress>>,

    /// Right-click menu opened at the pointer position.
    pub context_menu: Signal<Option<ContextMenuState>>,

    /// Message the composer replies to.
    pub reply_to: Signal<Option<TdlibMessageInfo>>,
    /// Message waiting to be forwarded.
    pub forward_state: Signal<Option<ForwardState>>,
    /// Media opened in the lightbox.
    pub lightbox: Signal<Option<LightboxState>>,
    /// User media panel opened from the sender context menu.
    pub user_media: Signal<Option<UserMediaState>>,
    /// Emoji picker for message reactions.
    pub reaction_picker: Signal<Option<ReactionPickerState>>,

    /// Pending URL delivered by an Android share intent.
    pub shared_url: Signal<Option<String>>,
}

impl UiState {
    pub fn tab(self) -> Tab {
        (self.tab)()
    }

    pub fn palette(self) -> Palette {
        (self.palette)()
    }

    pub fn density(self) -> Density {
        (self.density)()
    }

    pub fn logged_in(self) -> bool {
        (self.logged_in)()
    }

    pub fn show_spike(self) -> bool {
        (self.show_spike)()
    }

    pub fn show_palette(self) -> bool {
        (self.show_palette)()
    }

    pub fn auth_state(self) -> String {
        (self.auth_state)()
    }

    pub fn auth_error(self) -> Option<String> {
        (self.auth_error)()
    }

    pub fn auth_user(self) -> Option<String> {
        (self.auth_user)()
    }

    pub fn booting(self) -> bool {
        (self.booting)()
    }

    pub fn preview_mode(self) -> bool {
        (self.preview_mode)()
    }

    pub fn avatar(self) -> Option<String> {
        (self.avatar)()
    }

    pub fn chats(self) -> Vec<TdlibChatInfo> {
        (self.chats)()
    }

    pub fn chats_loading(self) -> bool {
        (self.chats_loading)()
    }

    pub fn chats_error(self) -> Option<String> {
        (self.chats_error)()
    }

    pub fn selected_chat(self) -> Option<TdlibChatInfo> {
        (self.selected_chat)()
    }

    pub fn messages(self) -> Vec<TdlibMessageInfo> {
        (self.messages)()
    }

    pub fn messages_loading(self) -> bool {
        (self.messages_loading)()
    }

    pub fn messages_error(self) -> Option<String> {
        (self.messages_error)()
    }

    pub fn messages_has_more(self) -> bool {
        (self.messages_has_more)()
    }

    pub fn topics(self) -> Vec<TdlibForumTopicInfo> {
        (self.topics)()
    }

    pub fn selected_topic(self) -> Option<TdlibForumTopicInfo> {
        (self.selected_topic)()
    }

    /// Keys (`kind:id`) of the topics hidden in the selected chat.
    pub fn hidden_topics(self) -> Vec<String> {
        (self.hidden_topics)()
    }

    pub fn topic_hidden(self, topic: &TdlibForumTopicInfo) -> bool {
        let key = topic_visibility_key(topic);
        self.hidden_topics().contains(&key)
    }

    pub fn topic_browser_open(self) -> bool {
        (self.topic_browser_open)()
    }

    pub fn select_portal(self) -> Option<SelectPortal> {
        (self.select_portal)()
    }

    pub fn messages_key(self) -> Option<String> {
        (self.messages_key)()
    }

    pub fn search_results(self) -> Vec<TdlibMessageInfo> {
        (self.search_results)()
    }

    pub fn search_loading(self) -> bool {
        (self.search_loading)()
    }

    pub fn search_error(self) -> Option<String> {
        (self.search_error)()
    }

    pub fn downloads(self) -> Vec<DownloadRecord> {
        (self.downloads)()
    }

    pub fn mass_download_running(self) -> bool {
        (self.mass_download_running)()
    }

    pub fn mass_download_stopping(self) -> bool {
        (self.mass_download_stopping)()
    }

    pub fn mass_download_progress(self) -> Option<MassDownloadProgress> {
        (self.mass_download_progress)()
    }

    pub fn mass_download_open(self) -> bool {
        (self.mass_download_open)()
    }

    pub fn duplicate_search_progress(self) -> Option<DuplicateSearchProgress> {
        (self.duplicate_search_progress)()
    }

    pub fn context_menu(self) -> Option<ContextMenuState> {
        (self.context_menu)()
    }

    pub fn reply_to(self) -> Option<TdlibMessageInfo> {
        (self.reply_to)()
    }

    pub fn forward_state(self) -> Option<ForwardState> {
        (self.forward_state)()
    }

    pub fn lightbox(self) -> Option<LightboxState> {
        (self.lightbox)()
    }

    pub fn user_media(self) -> Option<UserMediaState> {
        (self.user_media)()
    }

    pub fn reaction_picker(self) -> Option<ReactionPickerState> {
        (self.reaction_picker)()
    }

    pub fn telegram_tick(self) -> u64 {
        (self.telegram_tick)()
    }

    pub fn message_tick(self) -> u64 {
        (self.message_tick)()
    }
}

/// Stable key identifying a forum topic inside a chat.
pub fn topic_visibility_key(topic: &TdlibForumTopicInfo) -> String {
    let kind = if topic.kind.is_empty() {
        "forum"
    } else {
        topic.kind.as_str()
    };
    format!("{kind}:{}", topic.id)
}

/// SQLite key holding the hidden topic list of a chat.
pub fn topic_hidden_setting_key(chat_id: i64) -> String {
    format!("plasma_hidden_topics_{chat_id}")
}

/// Loads the hidden topics persisted for a chat.
pub fn load_hidden_topics(services: &AppServices, ui: UiState, chat_id: i64) {
    let keys = services
        .settings
        .get_raw(&topic_hidden_setting_key(chat_id))
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str::<Vec<String>>(&raw).ok())
        .unwrap_or_default();
    set_if_changed(ui.hidden_topics, keys);
}

/// Toggles a topic between hidden and visible, persisting the chat list.
///
/// Returns `true` when the topic is now hidden.
pub fn toggle_topic_hidden(
    services: &AppServices,
    mut ui: UiState,
    chat_id: i64,
    topic: &TdlibForumTopicInfo,
) -> bool {
    let key = topic_visibility_key(topic);
    let mut keys = ui.hidden_topics();
    let hidden = if let Some(position) = keys.iter().position(|entry| *entry == key) {
        keys.remove(position);
        false
    } else {
        keys.push(key);
        true
    };
    // Update the UI first and persist off the UI thread: the SQLite write used
    // to keep the chip visible until it finished.
    ui.hidden_topics.set(keys.clone());
    if let Ok(raw) = serde_json::to_string(&keys) {
        let services = services.clone();
        let setting = topic_hidden_setting_key(chat_id);
        spawn(async move {
            if let Err(error) = services.settings.set_raw(&setting, &raw) {
                push_toast(
                    ui.toasts,
                    format!("Falha ao salvar os tópicos ocultos: {error}"),
                    ToastKind::Error,
                );
            }
        });
    }
    hidden
}

/// Writes `next` only when it differs from the current value.
///
/// `Signal::set` always notifies its subscribers, so background refreshes that
/// return the same data would re-render the shell (and the timeline) on every
/// TDLib update. Returns `true` when the value changed.
pub fn set_if_changed<T: PartialEq + 'static>(mut signal: Signal<T>, next: T) -> bool {
    let changed = {
        let current = signal.read();
        *current != next
    };
    if changed {
        signal.set(next);
    }
    changed
}

pub fn push_toast(mut toasts: Signal<Vec<Toast>>, message: impl Into<String>, kind: ToastKind) {
    let mut next = toasts.read().clone();
    next.push(Toast::new(message, kind));
    if next.len() > 4 {
        next.remove(0);
    }
    toasts.set(next);
}

pub fn apply_palette(services: &AppServices, mut ui: UiState, palette: Palette) {
    ui.palette.set(palette);
    if let Err(error) = services.settings.set(
        plasma_store::SettingKey::APPEARANCE_PALETTE,
        palette.as_str(),
    ) {
        push_toast(
            ui.toasts,
            format!("Falha ao salvar a paleta: {error}"),
            ToastKind::Error,
        );
    }
}

pub fn apply_density(services: &AppServices, mut ui: UiState, density: Density) {
    ui.density.set(density);
    if let Err(error) = services.settings.set(
        plasma_store::SettingKey::APPEARANCE_DENSITY,
        density.as_str(),
    ) {
        push_toast(
            ui.toasts,
            format!("Falha ao salvar a densidade: {error}"),
            ToastKind::Error,
        );
    }
}

/// Copies a TDLib status into the shared auth signals.
pub fn apply_auth_status(mut ui: UiState, status: TdlibStatus) {
    let ready = status.ready;
    ui.auth_state.set(status.state);
    ui.auth_error.set(status.error);
    if ready {
        ui.logged_in.set(true);
        ui.preview_mode.set(false);
        bump_telegram_tick(ui);
        bump_message_tick(ui);
    }
}

pub fn apply_auth_error(mut ui: UiState, error: String) {
    ui.auth_state.set("error".to_string());
    ui.auth_error.set(Some(error));
}

pub fn bump_telegram_tick(mut ui: UiState) {
    let next = ui.telegram_tick().wrapping_add(1);
    (ui.telegram_tick).set(next);
}

pub fn bump_message_tick(mut ui: UiState) {
    let next = ui.message_tick().wrapping_add(1);
    (ui.message_tick).set(next);
}

/// Cache key compatible with the React app (`chatId` or
/// `chatId::forum:topicId`).
pub fn message_cache_key(chat_id: &str, topic: Option<&TdlibForumTopicInfo>) -> String {
    match topic {
        Some(topic) => format!("{chat_id}::{}:{}", topic.kind, topic.id),
        None => chat_id.to_string(),
    }
}

/// Toggles an emoji reaction on a message, updating the timeline immediately
/// and reverting the optimistic change if TDLib rejects it.
pub fn react_to_message(
    services: &AppServices,
    mut ui: UiState,
    chat_id: i64,
    message_id: i64,
    emoji: String,
) {
    let Some(mine) = ui
        .messages()
        .iter()
        .find(|message| message.id == message_id)
        .map(|message| {
            message
                .reactions
                .iter()
                .any(|reaction| reaction.emoji == emoji && reaction.mine)
        })
    else {
        return;
    };
    let add = !mine;
    let previous = ui
        .messages()
        .iter()
        .find(|message| message.id == message_id)
        .map(|message| message.reactions.clone())
        .unwrap_or_default();

    apply_reaction(&mut ui, message_id, &emoji, add);
    let services = services.clone();
    spawn(async move {
        if let Err(error) = services
            .telegram
            .toggle_reaction(chat_id, message_id, emoji, add)
            .await
        {
            let mut messages = ui.messages();
            if let Some(message) = messages.iter_mut().find(|message| message.id == message_id) {
                message.reactions = previous.clone();
            }
            set_if_changed(ui.messages, messages);
            let mut results = ui.search_results();
            if let Some(message) = results.iter_mut().find(|message| message.id == message_id) {
                message.reactions = previous;
            }
            set_if_changed(ui.search_results, results);
            push_toast(ui.toasts, error, ToastKind::Error);
        }
    });
}

/// Applies the optimistic reaction change to the conversation and search rows.
fn apply_reaction(ui: &mut UiState, message_id: i64, emoji: &str, add: bool) {
    let mut messages = ui.messages();
    if let Some(message) = messages.iter_mut().find(|message| message.id == message_id) {
        change_reactions(&mut message.reactions, emoji, add);
    }
    set_if_changed(ui.messages, messages);

    let mut results = ui.search_results();
    if let Some(message) = results.iter_mut().find(|message| message.id == message_id) {
        change_reactions(&mut message.reactions, emoji, add);
    }
    set_if_changed(ui.search_results, results);
}

fn change_reactions(
    reactions: &mut Vec<plasma_telegram::TdlibReactionInfo>,
    emoji: &str,
    add: bool,
) {
    if add {
        if let Some(reaction) = reactions
            .iter_mut()
            .find(|reaction| reaction.emoji == emoji)
        {
            if !reaction.mine {
                reaction.count += 1;
                reaction.mine = true;
            }
        } else {
            reactions.push(plasma_telegram::TdlibReactionInfo {
                emoji: emoji.to_string(),
                count: 1,
                mine: true,
            });
        }
        reactions.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.emoji.cmp(&b.emoji)));
    } else if let Some(position) = reactions
        .iter()
        .position(|reaction| reaction.emoji == emoji)
    {
        let reaction = &mut reactions[position];
        if reaction.mine {
            reaction.count -= 1;
            reaction.mine = false;
            if reaction.count <= 0 {
                reactions.remove(position);
            }
        }
    }
}

/// Marks the currently displayed messages as deleted immediately, before the
/// SQLite write finishes.
pub fn mark_deleted_messages(mut ui: UiState, message_ids: &[i64]) {
    if message_ids.is_empty() {
        return;
    }
    let ids: std::collections::HashSet<i64> = message_ids.iter().copied().collect();

    let mut messages = ui.messages();
    let mut changed = false;
    for message in &mut messages {
        if ids.contains(&message.id) && !message.is_deleted {
            message.is_deleted = true;
            changed = true;
        }
    }
    if changed {
        ui.messages.set(messages);
    }

    let mut results = ui.search_results();
    let mut changed = false;
    for message in &mut results {
        if ids.contains(&message.id) && !message.is_deleted {
            message.is_deleted = true;
            changed = true;
        }
    }
    if changed {
        ui.search_results.set(results);
    }
}

pub fn merge_messages(
    target: &mut Vec<TdlibMessageInfo>,
    incoming: impl IntoIterator<Item = TdlibMessageInfo>,
) {
    // Existing rows are replaced by the fresher copy so edits and reactions
    // show up after a refresh; the local deleted marker is never undone.
    let mut positions: std::collections::HashMap<i64, usize> = target
        .iter()
        .enumerate()
        .map(|(index, message)| (message.id, index))
        .collect();
    for mut message in incoming {
        if let Some(position) = positions.get(&message.id).copied() {
            message.is_deleted = message.is_deleted || target[position].is_deleted;
            target[position] = message;
        } else {
            positions.insert(message.id, target.len());
            target.push(message);
        }
    }
    target.sort_by_key(|message| (message.date, message_id_order(message.id)));
}

fn message_id_order(message_id: i64) -> (u8, u64) {
    if message_id < 0 {
        (1, message_id.unsigned_abs())
    } else {
        (0, message_id as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: i64, date: i32) -> TdlibMessageInfo {
        TdlibMessageInfo {
            id,
            message: String::new(),
            date,
            out: false,
            sender_id: String::new(),
            sender_name: String::new(),
            reply_to_msg_id: None,
            topic_id: None,
            topic_kind: None,
            media: false,
            text: String::new(),
            has_media: false,
            is_photo: false,
            is_video: false,
            grouped_id: None,
            video_duration: None,
            media_size: None,
            thumbnail_path: None,
            is_deleted: false,
            reactions: Vec::new(),
        }
    }

    #[test]
    fn lightbox_album_navigation_wraps() {
        let mut first = message(1, 10);
        first.sender_name = "Alice".to_string();
        first.is_photo = true;
        let mut second = message(2, 20);
        second.is_video = true;
        let mut third = message(3, 30);
        third.is_photo = true;
        let album = [&first, &second, &third]
            .into_iter()
            .map(LightboxMedia::from_message)
            .collect::<Vec<_>>();
        let state = LightboxState::from_album(-100, &first, album);
        assert_eq!(state.album_position(), Some((1, 3)));

        let next = state.next_album().expect("next album item");
        assert_eq!(next.message_id, 2);
        assert!(next.is_video);
        assert_eq!(next.album_position(), Some((2, 3)));

        let previous = state.previous_album().expect("previous album item");
        assert_eq!(previous.message_id, 3);
        assert_eq!(previous.album_position(), Some((3, 3)));

        let single = LightboxState::from_message(-100, &first);
        assert_eq!(single.album_position(), None);
        assert!(single.next_album().is_none());
    }

    #[test]
    fn cache_keys_match_the_react_app() {
        let topic = TdlibForumTopicInfo {
            id: 42,
            kind: "forum".to_string(),
            title: "Topic".to_string(),
            is_closed: false,
            is_pinned: false,
            unread_count: 0,
        };
        assert_eq!(message_cache_key("-100", None), "-100");
        assert_eq!(message_cache_key("-100", Some(&topic)), "-100::forum:42");
    }

    #[test]
    fn topic_visibility_keys_match_the_react_app() {
        let topic = TdlibForumTopicInfo {
            id: 42,
            kind: "forum".to_string(),
            title: "Topic".to_string(),
            is_closed: false,
            is_pinned: false,
            unread_count: 0,
        };
        assert_eq!(topic_visibility_key(&topic), "forum:42");
        let legacy = TdlibForumTopicInfo {
            kind: String::new(),
            ..topic
        };
        assert_eq!(topic_visibility_key(&legacy), "forum:42");
        assert_eq!(topic_hidden_setting_key(-100), "plasma_hidden_topics_-100");
    }

    #[test]
    fn merge_messages_deduplicates_and_sorts() {
        let mut target = vec![message(2, 20), message(1, 10)];
        merge_messages(&mut target, [message(2, 20), message(3, 30)]);
        assert_eq!(
            target.iter().map(|message| message.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn merge_messages_refreshes_existing_rows() {
        let mut target = vec![message(1, 10)];
        let mut updated = message(1, 10);
        updated.text = "editada".to_string();
        updated.reactions.push(plasma_telegram::TdlibReactionInfo {
            emoji: "👍".to_string(),
            count: 2,
            mine: true,
        });
        merge_messages(&mut target, [updated]);
        assert_eq!(target.len(), 1);
        assert_eq!(target[0].text, "editada");
        assert_eq!(target[0].reactions.len(), 1);
        assert_eq!(target[0].reactions[0].count, 2);
    }

    #[test]
    fn merge_messages_keeps_the_deleted_marker() {
        let mut target = vec![message(1, 10)];
        target[0].is_deleted = true;
        merge_messages(&mut target, [message(1, 10)]);
        assert!(target[0].is_deleted);
    }

    #[test]
    fn change_reactions_adds_and_removes() {
        let mut reactions = Vec::new();
        change_reactions(&mut reactions, "👍", true);
        assert_eq!(reactions.len(), 1);
        assert_eq!(reactions[0].count, 1);
        assert!(reactions[0].mine);
        change_reactions(&mut reactions, "👍", true);
        assert_eq!(reactions[0].count, 1, "adding twice must not inflate");
        change_reactions(&mut reactions, "❤️", true);
        change_reactions(&mut reactions, "👍", false);
        assert_eq!(reactions.len(), 1);
        assert_eq!(reactions[0].emoji, "❤️");
    }
}
