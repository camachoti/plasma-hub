//! Shared UI state and services.

use crate::context::ContextMenuState;
use crate::downloads::{DownloadHandle, DownloadRecord};
use crate::theme::{Density, Palette};
use crate::twitter::TwitterHandle;
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
    TdlibManager, TdlibMassDownloadRequest, TdlibMassDownloadResult, TdlibMessageInfo,
    TdlibMessagesResult, TdlibSearchMessagesResult, TdlibSendResult, TdlibSharedMediaResult,
    TdlibStatus, TdlibUserInfo,
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

    pub const fn rail_label(self) -> &'static str {
        match self {
            Self::Telegram => "TG",
            Self::Downloads => "DL",
            Self::Twitter => "X",
            Self::Settings => "CF",
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

/// Media opened in the lightbox.
#[derive(Clone, PartialEq)]
pub struct LightboxState {
    pub chat_id: i64,
    pub message_id: i64,
    pub is_video: bool,
    pub is_photo: bool,
    pub title: String,
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

    pub search_results: Signal<Vec<TdlibMessageInfo>>,
    pub search_loading: Signal<bool>,
    pub search_error: Signal<Option<String>>,

    pub telegram_tick: Signal<u64>,

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

    pub fn telegram_tick(self) -> u64 {
        (self.telegram_tick)()
    }
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

/// Cache key compatible with the React app (`chatId` or
/// `chatId::forum:topicId`).
pub fn message_cache_key(chat_id: &str, topic: Option<&TdlibForumTopicInfo>) -> String {
    match topic {
        Some(topic) => format!("{chat_id}::{}:{}", topic.kind, topic.id),
        None => chat_id.to_string(),
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
    let mut seen: std::collections::HashSet<i64> =
        target.iter().map(|message| message.id).collect();
    for message in incoming {
        if seen.insert(message.id) {
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
        }
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
    fn merge_messages_deduplicates_and_sorts() {
        let mut target = vec![message(2, 20), message(1, 10)];
        merge_messages(&mut target, [message(2, 20), message(3, 30)]);
        assert_eq!(
            target.iter().map(|message| message.id).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }
}
