use super::{now_millis, service_error};
use base64::Engine as _;
use plasma_core::{AppEnv, Error};
use plasma_duplicates::{SearchMode, SearchResult, SearchSource};
use plasma_store::{MessageCacheGetResult, MessageCacheMeta};
use plasma_telegram::{
    ConversationPriorityGuard, NativeMediaAssetMeta, NativeMediaCacheStats, NativePlaybackSource,
    NativeSavedMedia, TdlibChatCapabilitiesResult, TdlibChatInfo, TdlibChatsResult,
    TdlibForumTopicsResult, TdlibForwardResult, TdlibGlobalSearchResult, TdlibJoinResult,
    TdlibLinkTarget, TdlibManager, TdlibMassDownloadRequest, TdlibMassDownloadResult,
    TdlibMessageInfo, TdlibMessagesResult, TdlibSearchMessagesResult, TdlibSendResult,
    TdlibSharedMediaResult, TdlibStatus, TdlibUserInfo,
};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Semaphore;

/// Async facade over `plasma-telegram`.
///
/// TDLib needs a Tokio runtime, while Dioxus runs its own executor. Every call
/// is dispatched to the Tokio runtime and awaited without blocking the UI.
#[derive(Clone)]
pub struct TelegramHandle {
    manager: Arc<TdlibManager>,
    env: AppEnv,
    runtime: tokio::runtime::Handle,
    /// Limits parallel avatar downloads so restoring a long chat list cannot
    /// queue hundreds of TDLib requests at once on the single client.
    avatar_fetch_slots: Arc<Semaphore>,
    /// Each timeline thumbnail calls `get_message` plus a file fetch on miss;
    /// uncapped concurrency can stall the whole app when opening a media-heavy chat.
    thumbnail_fetch_slots: Arc<Semaphore>,
}

impl TelegramHandle {
    pub fn new(env: AppEnv, runtime: tokio::runtime::Handle) -> Self {
        Self {
            manager: Arc::new(TdlibManager::default()),
            env,
            runtime,
            // Keep open-chat history ahead of list avatars / timeline thumbs.
            avatar_fetch_slots: Arc::new(Semaphore::new(2)),
            thumbnail_fetch_slots: Arc::new(Semaphore::new(2)),
        }
    }

    pub fn runtime_handle(&self) -> tokio::runtime::Handle {
        self.runtime.clone()
    }

    async fn join<T>(&self, handle: tokio::task::JoinHandle<T>) -> Result<T, Error> {
        handle
            .await
            .map_err(|error| service_error(format!("Tarefa do Telegram falhou: {error}")))
    }

    pub async fn init(&self, api_id: i32, api_hash: String) -> plasma_core::Result<TdlibStatus> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_init(&env, &manager, api_id, api_hash).await
        });
        self.join(handle).await?
    }

    pub async fn status(&self) -> plasma_core::Result<TdlibStatus> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_status(&manager).await });
        self.join(handle).await?
    }

    pub async fn set_phone(&self, phone_number: String) -> plasma_core::Result<TdlibStatus> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_set_phone(&manager, phone_number).await });
        self.join(handle).await?
    }

    pub async fn check_code(&self, code: String) -> plasma_core::Result<TdlibStatus> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_check_code(&manager, code).await });
        self.join(handle).await?
    }

    pub async fn check_password(&self, password: String) -> plasma_core::Result<TdlibStatus> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_check_password(&manager, password).await });
        self.join(handle).await?
    }

    pub async fn get_me(&self) -> plasma_core::Result<TdlibUserInfo> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_get_me(&manager).await });
        self.join(handle).await?
    }

    pub async fn get_chats(&self, limit: i32) -> plasma_core::Result<TdlibChatsResult> {
        self.get_chats_with_mode(limit, false).await
    }

    /// Soft reconcile: Main list only, no Archive/contacts fill.
    pub async fn get_chats_soft(&self, limit: i32) -> plasma_core::Result<TdlibChatsResult> {
        self.get_chats_with_mode(limit, true).await
    }

    async fn get_chats_with_mode(
        &self,
        limit: i32,
        soft: bool,
    ) -> plasma_core::Result<TdlibChatsResult> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_get_chats(&manager, Some(limit), soft).await
        });
        self.join(handle).await?
    }

    /// Resolves a `t.me` or `tg://` link without joining.
    ///
    /// `needs_join` means the viewer can still open a preview. `join_by_request`
    /// means an administrator has to approve before the user becomes a member.
    pub async fn resolve_link(&self, url: String) -> plasma_core::Result<TdlibLinkTarget> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_resolve_link(&manager, url).await });
        self.join(handle).await?
    }

    pub async fn join_chat(&self, chat_id: i64) -> plasma_core::Result<TdlibJoinResult> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_join_chat(&manager, chat_id).await });
        self.join(handle).await?
    }

    pub async fn join_invite(&self, invite_link: String) -> plasma_core::Result<TdlibJoinResult> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_join_invite(&manager, invite_link).await });
        self.join(handle).await?
    }

    /// Fetches one chat, used to open a global search result whose
    /// conversation is not in the loaded chat list.
    pub async fn get_chat(&self, chat_id: i64) -> plasma_core::Result<TdlibChatInfo> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_get_chat(&manager, chat_id).await });
        self.join(handle).await?
    }

    /// Fetches one message, used to resolve replies whose original is outside
    /// the loaded timeline page.
    pub async fn get_message(
        &self,
        chat_id: i64,
        message_id: i64,
    ) -> plasma_core::Result<TdlibMessageInfo> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_get_message(&manager, chat_id, message_id).await
        });
        self.join(handle).await?
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn get_messages(
        &self,
        chat_id: i64,
        limit: i32,
        offset_id: i64,
        offset: i32,
        topic_id: Option<i64>,
        topic_kind: Option<String>,
        max_rounds: Option<usize>,
    ) -> plasma_core::Result<TdlibMessagesResult> {
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
                max_rounds,
            )
            .await
        });
        self.join(handle).await?
    }

    pub fn begin_conversation_priority(&self) -> ConversationPriorityGuard {
        ConversationPriorityGuard::enter(&self.manager)
    }

    pub fn conversation_priority_active(&self) -> bool {
        plasma_telegram::conversation_priority_active(&self.manager)
    }

    pub async fn wait_conversation_priority_clear(&self) {
        plasma_telegram::wait_conversation_priority_clear(&self.manager).await;
    }

    /// Resolves sender names after the history page is already visible.
    pub async fn fill_sender_names(
        &self,
        messages: Vec<TdlibMessageInfo>,
    ) -> plasma_core::Result<Vec<TdlibMessageInfo>> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_fill_sender_names(&manager, messages).await
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
    ) -> plasma_core::Result<TdlibSearchMessagesResult> {
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
    ) -> plasma_core::Result<TdlibGlobalSearchResult> {
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
        assume_forum: bool,
    ) -> plasma_core::Result<TdlibForumTopicsResult> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_get_forum_topics(&manager, chat_id, Some(limit), assume_forum)
                .await
        });
        self.join(handle).await?
    }

    /// Resolves whether a chat is a forum without listing its topics.
    pub async fn chat_is_forum(&self, chat_id: i64) -> plasma_core::Result<bool> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_chat_is_forum(&manager, chat_id).await });
        self.join(handle).await?
    }

    pub async fn read_chat(
        &self,
        chat_id: i64,
        last_message_id: Option<i64>,
        unread_hint: Option<i32>,
    ) -> plasma_core::Result<()> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_read_chat(&manager, chat_id, last_message_id, unread_hint).await
        });
        self.join(handle).await?
    }

    pub async fn capabilities(
        &self,
        chat_id: i64,
    ) -> plasma_core::Result<TdlibChatCapabilitiesResult> {
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
    ) -> plasma_core::Result<TdlibSendResult> {
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
    ) -> plasma_core::Result<TdlibSendResult> {
        let env = self.env.clone();
        let source = file_path;
        let prepared = tokio::task::spawn_blocking(move || {
            let source = PathBuf::from(source);
            plasma_media::prepare_quicktime_for_send(env.paths().cache_dir(), &source)
        })
        .await
        .map_err(|error| service_error(format!("Conversão do vídeo falhou: {error}")))?
        .map_err(service_error)?;
        let file_path = prepared.to_string_lossy().to_string();
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
    ) -> plasma_core::Result<TdlibForwardResult> {
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
    ) -> plasma_core::Result<TdlibSharedMediaResult> {
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
        from_message_id: Option<i64>,
    ) -> plasma_core::Result<TdlibSharedMediaResult> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_search_user_media(
                &manager,
                chat_id,
                sender_id,
                Some(limit),
                from_message_id,
            )
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
    ) -> plasma_core::Result<()> {
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
            Err(service_error(
                result
                    .error
                    .unwrap_or_else(|| "Não foi possível reagir.".to_string()),
            ))
        }
    }

    pub async fn thumbnail_data_url(
        &self,
        chat_id: i64,
        message_id: i64,
    ) -> plasma_core::Result<Option<String>> {
        // Queue already defers Background work while Conversation leases are held.
        let _permit = self
            .thumbnail_fetch_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| service_error(format!("semáforo de miniatura: {error}")))?;
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

    /// Small data URL for a preview file already on disk. Large files are skipped
    /// so a video path cannot be inlined into the download list.
    pub async fn file_data_url(&self, path: String) -> plasma_core::Result<Option<String>> {
        let handle = self.runtime.spawn(async move {
            let path = PathBuf::from(path);
            let length = tokio::task::spawn_blocking({
                let path = path.clone();
                move || std::fs::metadata(path).map(|metadata| metadata.len())
            })
            .await
            .map_err(|error| service_error(format!("Leitura da prévia falhou: {error}")))?
            .unwrap_or(0);
            if length == 0 || length > 400_000 {
                return Ok(None);
            }
            read_path_data_url(path).await.map(Some)
        });
        self.join(handle).await?
    }

    /// Builds a small data URL for a media file dragged into the composer.
    /// Videos use the cached poster frame; images are capped so a huge photo
    /// does not turn the preview modal into a multi-megabyte data URL.
    pub async fn media_preview_data_url(
        &self,
        file_path: String,
        is_video: bool,
    ) -> plasma_core::Result<Option<String>> {
        if is_video {
            let env = self.env.clone();
            let handle = self.runtime.spawn(async move {
                let poster = tokio::task::spawn_blocking(move || {
                    plasma_media::generate_video_thumbnail(&env, &file_path)
                })
                .await
                .map_err(|error| format!("Miniatura do vídeo falhou: {error}"))??;
                read_path_data_url(PathBuf::from(poster))
                    .await
                    .map(Some)
                    .map_err(|error| error.to_string())
            });
            return self.join(handle).await?.map_err(service_error);
        }

        let path = PathBuf::from(file_path);
        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        // WebKitGTK has no HEIC decoder. A JPEG next to the file is what the
        // preview can paint; without heif-convert or ffmpeg the tile stays empty.
        if matches!(extension.as_str(), "heic" | "heif") {
            let output = path.with_extension("preview.jpg");
            let source = path.clone();
            let preview = tokio::task::spawn_blocking(move || {
                plasma_media::jpeg_preview_for_still(&source, &output).map(|()| output)
            })
            .await;
            return match preview {
                Ok(Ok(preview)) => read_path_data_url(preview).await.map(Some),
                _ => Ok(None),
            };
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
    ) -> plasma_core::Result<NativePlaybackSource> {
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
        destination_dir: Option<String>,
    ) -> plasma_core::Result<NativeSavedMedia> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::telegram_media_save(
                &env,
                &manager,
                chat_id,
                message_id,
                destination_dir,
            )
            .await
        });
        self.join(handle).await?
    }

    pub async fn start_mass_download(
        &self,
        request: TdlibMassDownloadRequest,
    ) -> plasma_core::Result<TdlibMassDownloadResult> {
        let env = self.env.clone();
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_start_mass_download(&env, &manager, request).await
        });
        self.join(handle).await?
    }

    pub async fn stop_mass_download(&self) -> plasma_core::Result<()> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_stop_download(&manager).await });
        self.join(handle).await?
    }

    pub async fn topic_download_states(
        &self,
        chat_id: String,
    ) -> plasma_core::Result<Vec<plasma_store::TopicDownloadState>> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            tokio::task::spawn_blocking(move || {
                plasma_store::topic_download_state_get(&env, chat_id)
            })
            .await
        });
        self.join(handle)
            .await?
            .map_err(|error| service_error(error.to_string()))?
            .map_err(|error| service_error(error.to_string()))
    }

    pub async fn topic_has_new_media(
        &self,
        chat_id: i64,
        topic_id: i64,
        topic_kind: Option<String>,
        after_message_id: i64,
    ) -> plasma_core::Result<bool> {
        let manager = self.manager.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::tdlib_topic_has_new_media(
                &manager,
                chat_id,
                topic_id,
                topic_kind,
                after_message_id,
            )
            .await
        });
        self.join(handle).await?
    }

    pub async fn log_out(&self) -> plasma_core::Result<TdlibStatus> {
        let manager = self.manager.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::tdlib_log_out(&manager).await });
        self.join(handle).await?
    }

    /// Makes sure the avatar of a chat is available on disk.
    ///
    /// The file is cached under the app cache directory, so the second call
    /// for the same chat does not touch TDLib again.
    pub async fn ensure_chat_avatar(&self, chat_id: i64) -> plasma_core::Result<Option<String>> {
        // Avatar download is Background on the TDLib queue; the slot cap only
        // limits how many UI rows race into that queue at once.
        let _permit = self
            .avatar_fetch_slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|error| service_error(format!("semáforo de avatar: {error}")))?;
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
    ) -> plasma_core::Result<NativeMediaAssetMeta> {
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
    ) -> plasma_core::Result<SearchResult> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_duplicates::search_duplicate_media(&env, run_id, folder_path, sources, mode)
                .await
        });
        self.join(handle).await?.map_err(service_error)
    }

    /// Cancels a running duplicate search.
    pub fn cancel_duplicate_search(&self, run_id: &str) -> bool {
        plasma_download::cancel_download(&format!("duplicate-search-{run_id}"))
    }

    /// Size and item count of the native Telegram media cache.
    pub async fn media_cache_stats(&self) -> plasma_core::Result<NativeMediaCacheStats> {
        let env = self.env.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::telegram_media_cache_stats(&env).await });
        self.join(handle).await?
    }

    /// Removes every file of the native Telegram media cache.
    pub async fn clear_media_cache(&self) -> plasma_core::Result<NativeMediaCacheStats> {
        let env = self.env.clone();
        let handle = self
            .runtime
            .spawn(async move { plasma_telegram::telegram_media_clear_cache(&env).await });
        self.join(handle).await?
    }

    /// Evicts the oldest media until the cache fits the given size in bytes.
    pub async fn evict_media_cache(
        &self,
        max_size: i64,
    ) -> plasma_core::Result<NativeMediaCacheStats> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            plasma_telegram::telegram_media_evict_cache(&env, max_size).await
        });
        self.join(handle).await?
    }

    pub async fn download_avatar(&self, chat_id: i64) -> plasma_core::Result<Option<String>> {
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
    ) -> plasma_core::Result<MessageCacheGetResult> {
        let env = self.env.clone();
        let handle = self.runtime.spawn(async move {
            tokio::task::spawn_blocking(move || {
                plasma_store::telegram_message_cache_get(&env, cache_key)
            })
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
        });
        self.join(handle).await?.map_err(service_error)
    }

    pub async fn mark_messages_deleted(
        &self,
        chat_id: String,
        message_ids: Vec<i64>,
    ) -> plasma_core::Result<()> {
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
    ) -> plasma_core::Result<()> {
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

async fn read_path_data_url(path: PathBuf) -> plasma_core::Result<String> {
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
