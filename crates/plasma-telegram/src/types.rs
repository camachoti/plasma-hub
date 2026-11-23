use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tdlib_rs::types::File;

#[derive(Clone, Serialize)]
pub struct TdlibStatus {
    pub success: bool,
    pub ready: bool,
    pub state: String,
    pub error: Option<String>,
}

pub type TdlibCommandResult = plasma_core::Result<TdlibStatus>;

#[derive(Serialize)]
pub struct TdlibUserInfo {
    pub success: bool,
    pub id: Option<i64>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone_number: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibDownloadResult {
    pub success: bool,
    pub skipped: bool,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
    pub size: i64,
    pub mime_type: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMediaAssetMeta {
    pub success: bool,
    pub state: String,
    pub chat_id: i64,
    pub message_id: i64,
    pub total_bytes: Option<i64>,
    pub downloaded_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub file_name: Option<String>,
    pub native_file_path: Option<String>,
    pub thumbnail_path: Option<String>,
    pub completed_at: Option<i64>,
    pub updated_at: i64,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePlaybackSource {
    pub success: bool,
    pub kind: String,
    pub url: Option<String>,
    pub file_path: Option<String>,
    pub mime_type: Option<String>,
    pub cache_state: String,
    pub total_bytes: Option<i64>,
    pub downloaded_bytes: Option<i64>,
    pub file_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeSavedMedia {
    pub success: bool,
    pub file_path: Option<String>,
    pub file_name: Option<String>,
    pub size: Option<i64>,
    pub skipped: bool,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMediaCacheStats {
    pub success: bool,
    pub total_size: i64,
    pub media_count: usize,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NativeMediaProgress {
    pub(crate) chat_id: i64,
    pub(crate) message_id: i64,
    pub(crate) stage: String,
    pub(crate) progress: u8,
    pub(crate) downloaded_bytes: Option<i64>,
    pub(crate) total_bytes: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibThumbnailResult {
    pub success: bool,
    pub file_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibAvatarResult {
    pub success: bool,
    pub file_path: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibChatInfo {
    pub id: String,
    pub title: String,
    pub date: i32,
    pub unread_count: i32,
    pub is_group: bool,
    pub is_channel: bool,
    pub is_member: bool,
    pub can_send_messages: bool,
    pub can_send_media: bool,
    pub has_topics: bool,
    /// TDLib id of the newest message, used to mark the chat read without a
    /// fresh `getChat` when the list already knows the tip of the history.
    #[serde(default)]
    pub last_message_id: Option<i64>,
    pub last_message_text: String,
    pub last_message_date: i32,
    pub last_message_has_media: bool,
    pub last_message_is_video: bool,
    pub last_message_is_photo: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibChatsResult {
    pub success: bool,
    pub dialogs: Vec<TdlibChatInfo>,
    pub has_more: bool,
    pub error: Option<String>,
}

/// Emoji reaction attached to a message.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibReactionInfo {
    pub emoji: String,
    pub count: i32,
    pub mine: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibMessageInfo {
    pub id: i64,
    pub message: String,
    pub date: i32,
    pub out: bool,
    pub sender_id: String,
    pub sender_name: String,
    pub reply_to_msg_id: Option<i64>,
    pub topic_id: Option<i64>,
    pub topic_kind: Option<String>,
    pub media: bool,
    pub text: String,
    pub has_media: bool,
    pub is_photo: bool,
    pub is_video: bool,
    pub grouped_id: Option<String>,
    pub video_duration: Option<i32>,
    pub media_size: Option<i64>,
    /// Pixel size reported by Telegram. Missing on older cached messages.
    #[serde(default)]
    pub media_width: Option<i32>,
    #[serde(default)]
    pub media_height: Option<i32>,
    pub thumbnail_path: Option<String>,
    pub is_deleted: bool,
    /// Emoji reactions; `#[serde(default)]` keeps old cached rows readable.
    #[serde(default)]
    pub reactions: Vec<TdlibReactionInfo>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibMessagesResult {
    pub success: bool,
    pub messages: Vec<TdlibMessageInfo>,
    pub has_more: bool,
    pub oldest_message_id: Option<i64>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSearchMessagesResult {
    pub success: bool,
    pub messages: Vec<TdlibMessageInfo>,
    pub total_count: i32,
    pub next_from_message_id: Option<i64>,
    pub error: Option<String>,
}

/// One message returned by a cross-chat search.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibGlobalSearchMessage {
    pub chat_id: String,
    pub chat_title: String,
    pub message: TdlibMessageInfo,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibGlobalSearchResult {
    pub success: bool,
    pub messages: Vec<TdlibGlobalSearchMessage>,
    pub total_count: i32,
    pub next_offset: String,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSharedMediaItem {
    pub id: i64,
    pub date: i32,
    pub sender_id: String,
    pub sender_name: String,
    pub has_media: bool,
    pub is_photo: bool,
    pub is_video: bool,
    pub grouped_id: Option<String>,
    pub media_size: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSharedMediaResult {
    pub success: bool,
    pub media: Vec<TdlibSharedMediaItem>,
    /// Cursor for the next page, expressed in app message ids. Zero when there
    /// is nothing left to fetch.
    pub next_from_message_id: i64,
    pub has_more: bool,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibSendResult {
    pub success: bool,
    pub message: Option<TdlibMessageInfo>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibForwardResult {
    pub success: bool,
    pub messages: Vec<TdlibMessageInfo>,
    pub error: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TdlibForumTopicInfo {
    pub id: i64,
    pub kind: String,
    pub title: String,
    pub is_closed: bool,
    pub is_pinned: bool,
    pub unread_count: i32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibForumTopicsResult {
    pub success: bool,
    pub is_forum: bool,
    pub topics: Vec<TdlibForumTopicInfo>,
    pub error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibChatCapabilitiesResult {
    pub success: bool,
    pub chat_id: i64,
    pub is_member: bool,
    pub can_send_messages: bool,
    pub can_send_media: bool,
    pub error: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibMassDownloadRequest {
    pub chat_id: i64,
    pub folder_path: String,
    pub topic_id: Option<i64>,
    pub topic_kind: Option<String>,
    pub split_by_user: bool,
    pub split_by_album: bool,
    pub album_split_mode: Option<String>,
}

#[derive(Serialize)]
pub struct TdlibMassDownloadResult {
    pub success: bool,
    pub downloaded_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub total: usize,
    pub aborted: bool,
    pub error: Option<String>,
    pub tracking_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibDownloadItem {
    pub name: String,
    pub message_id: i64,
    pub status: String,
    pub progress: u8,
    pub size: i64,
    pub file_path: Option<String>,
    pub thumbnail_path: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TdlibDownloadProgress {
    pub chat_id: i64,
    pub total: usize,
    pub downloaded: f64,
    pub current_file: String,
    pub is_scanning: bool,
    pub items: Vec<TdlibDownloadItem>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TdlibNewMessageEvent {
    pub(crate) chat_id: i64,
    pub(crate) topic_id: Option<i64>,
    pub(crate) topic_kind: Option<String>,
    pub(crate) message: TdlibMessageInfo,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TdlibDeletedMessagesEvent {
    pub(crate) chat_id: i64,
    pub(crate) message_ids: Vec<i64>,
}

pub(crate) struct PendingMassDownload {
    pub(crate) file: File,
    pub(crate) thumbnail: Option<File>,
    pub(crate) destination: PathBuf,
    pub(crate) item_index: usize,
}
