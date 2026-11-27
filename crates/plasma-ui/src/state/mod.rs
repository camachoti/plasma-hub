//! Shared UI state and services.

use crate::context::ContextMenuState;
use crate::downloads::{DownloadHandle, DownloadRecord};
use crate::icons::IconName;
use crate::theme::{ColorMode, Density, UiStyle, COLOR_MODE_SETTING_KEY, STYLE_SETTING_KEY};
use crate::twitter::TwitterHandle;
use crate::views::select::SelectPortal;
use dioxus::prelude::*;
use plasma_core::{AppEnv, Error, EventBus};
use plasma_duplicates::SearchMatch;
use plasma_extract::{DownloadSpec, MediaInfo, Platform, ReelClip, ReelPage};
use plasma_store::SettingsStore;
use plasma_telegram::{
    NativeMediaCacheStats, TdlibChatInfo, TdlibDownloadItem, TdlibForumTopicInfo, TdlibMessageInfo,
    TdlibSharedMediaItem, TdlibStatus,
};
use std::time::{SystemTime, UNIX_EPOCH};

mod chats_cache;
mod forum;
mod telegram;

pub use chats_cache::*;
pub use forum::*;
pub use telegram::TelegramHandle;

/// Maps a service failure into the shared error type.
///
/// Download cancellation keeps its own variant so the UI can stop a job
/// without showing it as a failure.
pub(crate) fn service_error(message: impl std::fmt::Display) -> Error {
    let message = message.to_string();
    if message == plasma_download::DOWNLOAD_CANCELED_ERROR {
        Error::Cancelled
    } else {
        Error::msg(message)
    }
}

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
    pub async fn enforce_media_cache_limit(
        &self,
    ) -> plasma_core::Result<Option<NativeMediaCacheStats>> {
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

    pub async fn analyze(&self, url: String) -> plasma_core::Result<MediaInfo> {
        let cookies = self.cookies_for(&url).await;
        let env = self.env.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_extract::analyze_url(&env, &url, cookies).await });
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn download(
        &self,
        id: String,
        original_url: String,
        format_id: String,
        direct_url: Option<String>,
        file_name: String,
        has_audio: bool,
    ) -> plasma_core::Result<()> {
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
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn save_instagram_cookies(&self, cookies: String) -> plasma_core::Result<()> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_extract::save_instagram_cookies(&env, &cookies).map_err(service_error)
        });
        self.join(handle).await?
    }

    pub async fn list_reddit_reels(
        &self,
        subreddits: Vec<String>,
        after: Option<String>,
    ) -> plasma_core::Result<ReelPage> {
        let handle = self.runtime.spawn(async move {
            plasma_extract::list_reddit_reels(&subreddits, after.as_deref()).await
        });
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn list_instagram_reels(
        &self,
        username: String,
    ) -> plasma_core::Result<Vec<ReelClip>> {
        let cookies = self
            .load_instagram_cookies()
            .await
            .ok()
            .filter(|cookies| !cookies.trim().is_empty());
        let handle = self
            .runtime
            .spawn(async move { plasma_extract::list_instagram_reels(&username, cookies).await });
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn list_twitter_reels(&self, username: String) -> plasma_core::Result<Vec<ReelClip>> {
        let cookies = self
            .twitter
            .load_cookies()
            .await
            .ok()
            .filter(|cookies| !cookies.trim().is_empty());
        let handle = self
            .runtime
            .spawn(async move { plasma_extract::list_twitter_reels(&username, cookies).await });
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn list_youtube_reels(
        &self,
        sources: Vec<String>,
        interests: Vec<String>,
    ) -> plasma_core::Result<Vec<ReelClip>> {
        let handle = self
            .runtime
            .spawn(async move { plasma_extract::list_youtube_reels(&sources, &interests).await });
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn list_tiktok_reels(
        &self,
        sources: Vec<String>,
        interests: Vec<String>,
    ) -> plasma_core::Result<Vec<ReelClip>> {
        let handle = self
            .runtime
            .spawn(async move { plasma_extract::list_tiktok_reels(&sources, &interests).await });
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn resolve_clip(&self, url: String) -> plasma_core::Result<ReelClip> {
        let cookies = self.cookies_for(&url).await;
        let env = self.env.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_extract::resolve_clip_from_url(&env, &url, cookies).await });
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn resolve_reel_stream(
        &self,
        clip: ReelClip,
    ) -> plasma_core::Result<plasma_extract::ResolvedReelStream> {
        let cookies = self.cookies_for(&clip.page_url).await;
        let env = self.env.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_extract::resolve_reel_stream(&env, &clip, cookies).await });
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn load_instagram_cookies(&self) -> plasma_core::Result<String> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_extract::load_instagram_cookies(&env).map_err(service_error)
        });
        self.join(handle).await?
    }

    async fn cookies_for(&self, url: &str) -> Option<String> {
        match Platform::detect(url) {
            Platform::Twitter => self
                .twitter
                .load_cookies()
                .await
                .ok()
                .filter(|cookies| !cookies.trim().is_empty()),
            Platform::Instagram => self
                .load_instagram_cookies()
                .await
                .ok()
                .filter(|cookies| !cookies.trim().is_empty()),
            _ => None,
        }
    }

    async fn join<T>(&self, handle: tokio::task::JoinHandle<T>) -> Result<T, Error> {
        handle
            .await
            .map_err(|error| service_error(format!("Tarefa do extrator falhou: {error}")))
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
    Reels,
    Downloads,
    Twitter,
    Settings,
}

impl Tab {
    pub const ALL: [Self; 5] = [
        Self::Telegram,
        Self::Reels,
        Self::Downloads,
        Self::Twitter,
        Self::Settings,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Telegram => "Telegram",
            Self::Reels => "Reels",
            Self::Downloads => "Downloads",
            Self::Twitter => "Twitter/X",
            Self::Settings => "Ajustes",
        }
    }

    pub const fn rail_icon(self) -> IconName {
        match self {
            Self::Telegram => IconName::BrandTelegram,
            Self::Reels => IconName::Play,
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
pub enum ToastAction {
    OpenTab(Tab),
    RetryDownload(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
    pub action_label: Option<String>,
    pub action: Option<ToastAction>,
}

impl Toast {
    pub fn new(message: impl Into<String>, kind: ToastKind) -> Self {
        Self {
            id: NEXT_TOAST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            message: message.into(),
            kind,
            action_label: None,
            action: None,
        }
    }

    pub fn with_action(mut self, label: impl Into<String>, action: ToastAction) -> Self {
        self.action_label = Some(label.into());
        self.action = Some(action);
        self
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

/// One media item to look up inside a local folder.
#[derive(Clone, Debug, PartialEq)]
pub struct DuplicateSearchMedia {
    pub chat_id: i64,
    pub message_id: i64,
    pub name: String,
    pub is_video: bool,
}

impl DuplicateSearchMedia {
    pub fn from_message(chat_id: i64, message: &TdlibMessageInfo) -> Self {
        let kind = if message.is_video {
            "Vídeo"
        } else if message.is_photo {
            "Foto"
        } else {
            "Mídia"
        };
        Self {
            chat_id,
            message_id: message.id,
            name: format!("{kind} · {}", message.id),
            is_video: message.is_video,
        }
    }
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

/// Last Twitter/X gallery sync outcome shown in the conversation header.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TwitterGallerySyncStatus {
    pub chat_id: String,
    pub last_at_ms: Option<i64>,
    pub last_error: Option<String>,
    pub checking: bool,
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
    pub date: i32,
    pub media_size: Option<i64>,
    pub video_duration: Option<i32>,
}

impl LightboxMedia {
    pub fn from_message(message: &TdlibMessageInfo) -> Self {
        Self {
            message_id: message.id,
            is_video: message.is_video,
            is_photo: message.is_photo,
            date: message.date,
            media_size: message.media_size,
            video_duration: message.video_duration,
        }
    }

    pub fn from_shared(item: &TdlibSharedMediaItem) -> Self {
        Self {
            message_id: item.id,
            is_video: item.is_video,
            is_photo: item.is_photo,
            date: item.date,
            media_size: item.media_size,
            video_duration: None,
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
    pub date: i32,
    pub media_size: Option<i64>,
    pub video_duration: Option<i32>,
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
            date: message.date,
            media_size: message.media_size,
            video_duration: message.video_duration,
            album: Vec::new(),
        }
    }

    pub fn from_album(chat_id: i64, message: &TdlibMessageInfo, album: Vec<LightboxMedia>) -> Self {
        Self {
            album,
            ..Self::from_message(chat_id, message)
        }
    }

    pub fn from_shared(
        chat_id: i64,
        item: &TdlibSharedMediaItem,
        title: String,
        album: Vec<LightboxMedia>,
    ) -> Self {
        Self {
            chat_id,
            message_id: item.id,
            is_video: item.is_video,
            is_photo: item.is_photo,
            title,
            date: item.date,
            media_size: item.media_size,
            video_duration: None,
            album,
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
            date: item.date,
            media_size: item.media_size,
            video_duration: item.video_duration,
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

/// Dockable right-hand drawer sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawerTab {
    Queue,
    Media,
    Topics,
    MassDownload,
}

impl DrawerTab {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Queue => "Fila",
            Self::Media => "Mídias",
            Self::Topics => "Tópicos",
            Self::MassDownload => "Download",
        }
    }
}

/// Quick filter pills in the unified conversation sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatScope {
    All,
    Unread,
    Forums,
    Twitter,
}

impl ChatScope {
    pub const ALL: [Self; 4] = [Self::All, Self::Unread, Self::Forums, Self::Twitter];

    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "Todas",
            Self::Unread => "Não lidas",
            Self::Forums => "Fóruns",
            Self::Twitter => "Galerias",
        }
    }
}

/// Chrome shared by every screen: navigation, multi-style theme, panels and toasts.
#[derive(Clone, Copy)]
pub struct ShellState {
    pub tab: Signal<Tab>,
    pub style: Signal<UiStyle>,
    pub color_mode: Signal<ColorMode>,
    pub density: Signal<Density>,
    pub sidebar_open: Signal<bool>,
    pub drawer_open: Signal<bool>,
    pub drawer_tab: Signal<DrawerTab>,
    pub logged_in: Signal<bool>,
    pub show_spike: Signal<bool>,
    pub show_palette: Signal<bool>,
    pub toasts: Signal<Vec<Toast>>,
    /// Option list of the select currently open, rendered at the app root.
    pub select_portal: Signal<Option<SelectPortal>>,
    /// Pending URL delivered by an Android share intent.
    pub shared_url: Signal<Option<String>>,
}

/// Login and startup, including the preview shown before an account is ready.
#[derive(Clone, Copy)]
pub struct SessionState {
    pub auth_state: Signal<String>,
    pub auth_error: Signal<Option<String>>,
    pub auth_user: Signal<Option<String>>,
    pub booting: Signal<bool>,
    pub preview_mode: Signal<bool>,
    pub avatar: Signal<Option<String>>,
}

/// Open Telegram conversation: chat list, timeline, topics and search.
#[derive(Clone, Copy)]
pub struct ConversationState {
    pub chats: Signal<Vec<TdlibChatInfo>>,
    pub chats_loading: Signal<bool>,
    pub chats_error: Signal<Option<String>>,
    pub selected_chat: Signal<Option<TdlibChatInfo>>,
    /// Narrow/mobile pane: when true the chat list fills the viewport and the
    /// open conversation is hidden. Ignored on wide layouts (CSS).
    pub mobile_show_list: Signal<bool>,

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
    /// Cached topic listings by chat id, so a known forum opens with its topics
    /// already on screen while a fresh copy loads in the background.
    pub topics_cache: Signal<std::collections::HashMap<String, Vec<TdlibForumTopicInfo>>>,
    /// Topic keys (`kind:id`) known to have media newer than the last mass download.
    pub topics_with_new_media: Signal<std::collections::HashSet<String>>,
    /// Pending new-media counts for Twitter/X gallery chats (`twitter_profile_*`).
    pub twitter_gallery_badges: Signal<std::collections::HashMap<String, usize>>,
    /// Last gallery poller / manual sync status for the open fake chat header.
    pub twitter_sync_status: Signal<Option<TwitterGallerySyncStatus>>,

    pub search_results: Signal<Vec<TdlibMessageInfo>>,
    pub search_loading: Signal<bool>,
    pub search_error: Signal<Option<String>>,

    pub telegram_tick: Signal<u64>,
    /// Bumped only for messages/deletions in the open conversation. The chat
    /// list keeps using `telegram_tick`, so unrelated chats no longer force a
    /// TDLib reload of the visible timeline.
    pub message_tick: Signal<u64>,
}

/// File transfers started from the app: history, mass download and duplicate scan.
#[derive(Clone, Copy)]
pub struct TransferState {
    pub downloads: Signal<Vec<DownloadRecord>>,
    pub mass_download_running: Signal<bool>,
    pub mass_download_stopping: Signal<bool>,
    pub mass_download_progress: Signal<Option<MassDownloadProgress>>,
    pub mass_download_open: Signal<bool>,
    /// Topic pre-selected from a context menu; used as the panel default.
    pub mass_download_topic: Signal<Option<TdlibForumTopicInfo>>,
    pub duplicate_search_progress: Signal<Option<DuplicateSearchProgress>>,
    /// When set, the "Procurar arquivo no sistema" dialog is open with these items.
    pub duplicate_search: Signal<Option<Vec<DuplicateSearchMedia>>>,
}

/// Transient surfaces drawn over the timeline.
#[derive(Clone, Copy)]
pub struct PanelState {
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
}

/// Signal bundle shared through context.
#[derive(Clone, Copy)]
pub struct UiState {
    pub shell: ShellState,
    pub session: SessionState,
    pub conversation: ConversationState,
    pub transfers: TransferState,
    pub panels: PanelState,
}

impl ShellState {
    pub fn tab(self) -> Tab {
        (self.tab)()
    }

    pub fn style(self) -> UiStyle {
        (self.style)()
    }

    pub fn color_mode(self) -> ColorMode {
        (self.color_mode)()
    }

    pub fn density(self) -> Density {
        (self.density)()
    }

    pub fn sidebar_open(self) -> bool {
        (self.sidebar_open)()
    }

    pub fn drawer_open(self) -> bool {
        (self.drawer_open)()
    }

    pub fn drawer_tab(self) -> DrawerTab {
        (self.drawer_tab)()
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

    pub fn select_portal(self) -> Option<SelectPortal> {
        (self.select_portal)()
    }
}

impl SessionState {
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
}

impl ConversationState {
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

    /// Narrow pane: chat list fills the viewport (ignored on wide layouts).
    pub fn mobile_show_list(self) -> bool {
        (self.mobile_show_list)()
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

    pub fn messages_key(self) -> Option<String> {
        (self.messages_key)()
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

    pub fn topics_with_new_media(self) -> std::collections::HashSet<String> {
        (self.topics_with_new_media)()
    }

    pub fn twitter_gallery_badges(self) -> std::collections::HashMap<String, usize> {
        (self.twitter_gallery_badges)()
    }

    pub fn twitter_sync_status(self) -> Option<TwitterGallerySyncStatus> {
        (self.twitter_sync_status)()
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

    pub fn telegram_tick(self) -> u64 {
        (self.telegram_tick)()
    }

    pub fn message_tick(self) -> u64 {
        (self.message_tick)()
    }
}

impl TransferState {
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

    pub fn mass_download_topic(self) -> Option<TdlibForumTopicInfo> {
        (self.mass_download_topic)()
    }

    pub fn duplicate_search_progress(self) -> Option<DuplicateSearchProgress> {
        (self.duplicate_search_progress)()
    }

    pub fn duplicate_search(self) -> Option<Vec<DuplicateSearchMedia>> {
        (self.duplicate_search)()
    }
}

impl PanelState {
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

pub fn push_toast(
    mut toasts: Signal<Vec<Toast>>,
    message: impl std::fmt::Display,
    kind: ToastKind,
) {
    let mut next = toasts.read().clone();
    next.push(Toast::new(message.to_string(), kind));
    if next.len() > 4 {
        next.remove(0);
    }
    toasts.set(next);
}

pub fn push_toast_with_action(
    mut toasts: Signal<Vec<Toast>>,
    message: impl std::fmt::Display,
    kind: ToastKind,
    action_label: impl Into<String>,
    action: ToastAction,
) {
    let mut next = toasts.read().clone();
    next.push(Toast::new(message.to_string(), kind).with_action(action_label, action));
    if next.len() > 4 {
        next.remove(0);
    }
    toasts.set(next);
}

pub fn apply_style(services: &AppServices, mut ui: UiState, style: UiStyle) {
    ui.shell.style.set(style);
    if let Err(error) = services.settings.set(STYLE_SETTING_KEY, style.as_str()) {
        push_toast(
            ui.shell.toasts,
            format!("Falha ao salvar o estilo: {error}"),
            ToastKind::Error,
        );
    }
}

pub fn apply_color_mode(services: &AppServices, mut ui: UiState, mode: ColorMode) {
    ui.shell.color_mode.set(mode);
    if let Err(error) = services.settings.set(COLOR_MODE_SETTING_KEY, mode.as_str()) {
        push_toast(
            ui.shell.toasts,
            format!("Falha ao salvar o modo de cor: {error}"),
            ToastKind::Error,
        );
    }
    let _ = services
        .settings
        .set(plasma_store::SettingKey::APPEARANCE_PALETTE, mode.as_str());
}

pub fn set_drawer_open(services: &AppServices, mut ui: UiState, open: bool) {
    ui.shell.drawer_open.set(open);
    let _ = services
        .settings
        .set_bool(plasma_store::SettingKey::INFO_PANEL_OPEN, open);
}

pub fn open_drawer_tab(services: &AppServices, mut ui: UiState, tab: DrawerTab) {
    ui.shell.drawer_tab.set(tab);
    set_drawer_open(services, ui, true);
}

pub fn apply_density(services: &AppServices, mut ui: UiState, density: Density) {
    ui.shell.density.set(density);
    if let Err(error) = services.settings.set(
        plasma_store::SettingKey::APPEARANCE_DENSITY,
        density.as_str(),
    ) {
        push_toast(
            ui.shell.toasts,
            format!("Falha ao salvar a densidade: {error}"),
            ToastKind::Error,
        );
    }
}

/// Copies a TDLib status into the shared auth signals.
pub fn apply_auth_status(mut ui: UiState, status: TdlibStatus) {
    let ready = status.ready;
    ui.session.auth_state.set(status.state);
    ui.session.auth_error.set(status.error);
    if ready {
        ui.shell.logged_in.set(true);
        ui.session.preview_mode.set(false);
        // List only. Reloading the open conversation here raced get_chats and
        // doubled the history work after every reconnect.
        bump_telegram_tick(ui);
    }
}

pub fn apply_auth_error(mut ui: UiState, error: impl std::fmt::Display) {
    ui.session.auth_state.set("error".to_string());
    ui.session.auth_error.set(Some(error.to_string()));
}

pub fn bump_telegram_tick(mut ui: UiState) {
    let next = ui.conversation.telegram_tick().wrapping_add(1);
    (ui.conversation.telegram_tick).set(next);
}

pub fn bump_message_tick(mut ui: UiState) {
    let next = ui.conversation.message_tick().wrapping_add(1);
    (ui.conversation.message_tick).set(next);
}

fn live_preview(message: &TdlibMessageInfo) -> String {
    if message.text.trim().is_empty() {
        message.message.clone()
    } else {
        message.text.clone()
    }
}

fn missing_chat_claims() -> &'static std::sync::Mutex<std::collections::HashSet<i64>> {
    static CLAIMS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<i64>>> =
        std::sync::OnceLock::new();
    CLAIMS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

fn claim_missing_chat(chat_id: i64) -> bool {
    missing_chat_claims()
        .lock()
        .map(|mut claims| claims.insert(chat_id))
        .unwrap_or(false)
}

fn release_missing_chat(chat_id: i64) {
    if let Ok(mut claims) = missing_chat_claims().lock() {
        claims.remove(&chat_id);
    }
}

/// Updates one chat row from a message that just arrived.
///
/// Returns whether the row was already in the list. The caller fetches a
/// missing chat on its own instead of scanning every conversation.
pub fn apply_live_chat_row(chat: &mut TdlibChatInfo, message: &TdlibMessageInfo, is_open: bool) {
    chat.last_message_id = Some(message.id);
    chat.last_message_text = live_preview(message);
    chat.last_message_date = message.date;
    chat.date = message.date;
    chat.last_message_has_media = message.has_media;
    chat.last_message_is_video = message.is_video;
    chat.last_message_is_photo = message.is_photo;
    if is_open {
        chat.unread_count = 0;
    } else if !message.out {
        chat.unread_count = chat.unread_count.saturating_add(1);
    }
}

/// Applies a live message to the chat list and, when it belongs there, the
/// open timeline. This replaces a full TDLib reload of both.
pub fn apply_live_message(
    services: &AppServices,
    mut ui: UiState,
    chat_id: i64,
    message: TdlibMessageInfo,
) {
    let chat_key = chat_id.to_string();
    let open_id = ui
        .conversation
        .selected_chat
        .peek()
        .as_ref()
        .map(|chat| chat.id.clone());
    let is_open = open_id.as_deref() == Some(chat_key.as_str());
    let mut chats = ui.conversation.chats.peek().clone();
    if let Some(index) = chats.iter().position(|chat| chat.id == chat_key) {
        apply_live_chat_row(&mut chats[index], &message, is_open);
        let updated = chats.remove(index);
        if is_open {
            let mut selected = ui.conversation.selected_chat.peek().clone();
            if let Some(selected) = selected.as_mut() {
                selected.last_message_id = updated.last_message_id;
                selected.last_message_text = updated.last_message_text.clone();
                selected.last_message_date = updated.last_message_date;
                selected.date = updated.date;
                selected.last_message_has_media = updated.last_message_has_media;
                selected.last_message_is_video = updated.last_message_is_video;
                selected.last_message_is_photo = updated.last_message_is_photo;
                selected.unread_count = 0;
            }
            set_if_changed(ui.conversation.selected_chat, selected);
        }
        chats.insert(0, updated);
        ui.conversation.chats.set(chats);
    } else if !chats.is_empty() && claim_missing_chat(chat_id) {
        // An empty list means the startup scan is still running. Fetching
        // every live chat on top of it piles getChat calls onto the same
        // client and keeps the window on the loading placeholder.
        let services = services.clone();
        spawn(async move {
            if let Ok(chat) = services.telegram.get_chat(chat_id).await {
                let mut chats = ui.conversation.chats.peek().clone();
                if !chats.iter().any(|existing| existing.id == chat.id) {
                    chats.insert(0, chat);
                    ui.conversation.chats.set(chats);
                }
            }
            release_missing_chat(chat_id);
        });
    }

    if !is_open {
        return;
    }
    let topic = ui.conversation.selected_topic.peek().clone();
    let visible_here = match topic.as_ref() {
        Some(topic) => {
            message.topic_id == Some(topic.id)
                && (topic.kind.is_empty()
                    || message
                        .topic_kind
                        .as_deref()
                        .is_none_or(|kind| kind == topic.kind))
        }
        None => true,
    };
    let cache_key = message_cache_key(&chat_key, topic.as_ref());
    if visible_here && ui.conversation.messages_key.peek().as_deref() == Some(cache_key.as_str()) {
        let mut messages = ui.conversation.messages.peek().clone();
        merge_messages(&mut messages, [message.clone()]);
        set_if_changed(ui.conversation.messages, messages);
    }
    if let Some(topic_id) = message.topic_id {
        let reading_it = topic.as_ref().is_some_and(|topic| topic.id == topic_id);
        if !reading_it {
            let mut topics = ui.conversation.topics.peek().clone();
            if let Some(topic) = topics.iter_mut().find(|topic| topic.id == topic_id) {
                topic.unread_count = topic.unread_count.saturating_add(1);
                ui.conversation.topics.set(topics.clone());
                ui.conversation
                    .topics_cache
                    .write()
                    .insert(chat_key, topics);
            }
        }
    }
}

/// Patches a message already rendered in the open timeline.
pub fn apply_message_content(
    mut ui: UiState,
    chat_id: i64,
    message_id: i64,
    text: String,
    has_media: bool,
    is_photo: bool,
    is_video: bool,
) {
    let chat_key = chat_id.to_string();
    if ui
        .conversation
        .selected_chat
        .peek()
        .as_ref()
        .map(|chat| chat.id.as_str())
        != Some(chat_key.as_str())
    {
        return;
    }
    let mut messages = ui.conversation.messages.peek().clone();
    let Some(message) = messages.iter_mut().find(|message| message.id == message_id) else {
        return;
    };
    message.text = text.clone();
    message.message = text;
    message.has_media = has_media;
    message.media = has_media;
    message.is_photo = is_photo;
    message.is_video = is_video;
    ui.conversation.messages.set(messages);
}

/// Applies a title or unread change without reloading the chat list.
pub fn apply_chat_patch(
    mut ui: UiState,
    chat_id: i64,
    title: Option<String>,
    unread_count: Option<i32>,
) {
    let chat_key = chat_id.to_string();
    let mut chats = ui.conversation.chats.peek().clone();
    let Some(chat) = chats.iter_mut().find(|chat| chat.id == chat_key) else {
        return;
    };
    if let Some(title) = title.clone() {
        chat.title = title;
    }
    if let Some(unread_count) = unread_count {
        chat.unread_count = unread_count;
    }
    let updated_title = chat.title.clone();
    let updated_unread = chat.unread_count;
    ui.conversation.chats.set(chats);
    if ui
        .conversation
        .selected_chat
        .peek()
        .as_ref()
        .map(|chat| chat.id.as_str())
        == Some(chat_key.as_str())
    {
        let mut selected = ui.conversation.selected_chat.peek().clone();
        if let Some(selected) = selected.as_mut() {
            selected.title = updated_title;
            selected.unread_count = updated_unread;
        }
        set_if_changed(ui.conversation.selected_chat, selected);
    }
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
        .conversation
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
        .conversation
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
            let still_here = ui
                .conversation
                .selected_chat()
                .and_then(|chat| chat.id.parse::<i64>().ok())
                == Some(chat_id);
            if !still_here {
                push_toast(ui.shell.toasts, error, ToastKind::Error);
                return;
            }
            let mut messages = ui.conversation.messages();
            if let Some(message) = messages.iter_mut().find(|message| message.id == message_id) {
                message.reactions = previous.clone();
            }
            set_if_changed(ui.conversation.messages, messages);
            let mut results = ui.conversation.search_results();
            if let Some(message) = results.iter_mut().find(|message| message.id == message_id) {
                message.reactions = previous;
            }
            set_if_changed(ui.conversation.search_results, results);
            push_toast(ui.shell.toasts, error, ToastKind::Error);
        }
    });
}

/// Applies the optimistic reaction change to the conversation and search rows.
fn apply_reaction(ui: &mut UiState, message_id: i64, emoji: &str, add: bool) {
    let mut messages = ui.conversation.messages();
    if let Some(message) = messages.iter_mut().find(|message| message.id == message_id) {
        change_reactions(&mut message.reactions, emoji, add);
    }
    set_if_changed(ui.conversation.messages, messages);

    let mut results = ui.conversation.search_results();
    if let Some(message) = results.iter_mut().find(|message| message.id == message_id) {
        change_reactions(&mut message.reactions, emoji, add);
    }
    set_if_changed(ui.conversation.search_results, results);
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

    let mut messages = ui.conversation.messages();
    let mut changed = false;
    for message in &mut messages {
        if ids.contains(&message.id) && !message.is_deleted {
            message.is_deleted = true;
            changed = true;
        }
    }
    if changed {
        ui.conversation.messages.set(messages);
    }

    let mut results = ui.conversation.search_results();
    let mut changed = false;
    for message in &mut results {
        if ids.contains(&message.id) && !message.is_deleted {
            message.is_deleted = true;
            changed = true;
        }
    }
    if changed {
        ui.conversation.search_results.set(results);
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
            if message.sender_name.trim().is_empty() {
                message.sender_name = target[position].sender_name.clone();
            }
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
            media_width: None,
            media_height: None,
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
        assert_eq!(topic_list_setting_key(-100), "plasma_topics_-100");
        assert_eq!(forum_flag_setting_key(-100), "plasma_forum_-100");
    }

    #[test]
    fn forum_decision_uses_the_cache_before_asking_tdlib() {
        assert_eq!(
            forum_decision(true, true, true, None),
            ForumDecision::ShowCached
        );
        assert_eq!(
            forum_decision(true, true, false, None),
            ForumDecision::ShowLoading
        );
        assert_eq!(
            forum_decision(false, true, true, Some(false)),
            ForumDecision::ShowCached
        );
        assert_eq!(
            forum_decision(false, true, false, Some(true)),
            ForumDecision::ShowLoading
        );
        assert_eq!(
            forum_decision(false, true, false, Some(false)),
            ForumDecision::Plain
        );
        assert_eq!(
            forum_decision(false, false, false, None),
            ForumDecision::Plain
        );
        assert_eq!(
            forum_decision(false, true, false, None),
            ForumDecision::Probe
        );
        assert_eq!(
            forum_decision(true, false, false, Some(false)),
            ForumDecision::ShowLoading
        );
    }

    #[test]
    fn live_message_updates_the_row_without_marking_the_open_chat_unread() {
        let mut chat = TdlibChatInfo {
            id: "7".to_string(),
            title: "Grupo".to_string(),
            date: 1,
            unread_count: 2,
            is_group: true,
            is_channel: false,
            is_member: true,
            can_send_messages: true,
            can_send_media: true,
            has_topics: false,
            last_message_id: Some(1),
            last_message_text: "antes".to_string(),
            last_message_date: 1,
            last_message_has_media: false,
            last_message_is_video: false,
            last_message_is_photo: false,
        };
        let mut incoming = message(9, 20);
        incoming.text = "agora".to_string();
        apply_live_chat_row(&mut chat, &incoming, true);
        assert_eq!(chat.last_message_text, "agora");
        assert_eq!(chat.last_message_date, 20);
        assert_eq!(chat.unread_count, 0);

        apply_live_chat_row(&mut chat, &incoming, false);
        assert_eq!(chat.unread_count, 1);
        incoming.out = true;
        apply_live_chat_row(&mut chat, &incoming, false);
        assert_eq!(chat.unread_count, 1);
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
