//! Synthetic Twitter/X media galleries shown inside the Telegram chat list.
//!
//! These are not real Telegram chats and not Twitter DMs — only profile media
//! timelines persisted locally and merged into the UI. Sync is append-only:
//! new media is added, media that disappears from X stays on disk.

use plasma_store::{SettingKey, SettingsStore};
use plasma_telegram::{TdlibChatInfo, TdlibMessageInfo};
use plasma_twitter::{NativeProfileInfo, NativeProfileMediaItem};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

pub const TWITTER_FAKE_CHAT_PREFIX: &str = "twitter_profile_";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TwitterFakeMessage {
    pub id: i64,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    pub is_video: bool,
    pub date: i32,
    pub text: String,
    #[serde(default)]
    pub media_size: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TwitterFakeChat {
    pub id: String,
    pub username: String,
    pub title: String,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub thumbnail_url: Option<String>,
    pub original_url: String,
    pub created_at: i32,
    pub messages: Vec<TwitterFakeMessage>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TwitterFakeUpsertResult {
    pub chat_id: String,
    pub added_count: usize,
    pub media_count: usize,
    pub created: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TwitterFakeUpsertError {
    pub message: String,
}

impl TwitterFakeUpsertError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

pub fn is_twitter_fake_chat_id(chat_id: &str) -> bool {
    chat_id.starts_with(TWITTER_FAKE_CHAT_PREFIX)
}

/// Stable negative i64 used by timeline/thumbnail/lightbox paths that expect
/// a numeric chat id. Never overlaps with real TDLib ids (which are positive
/// for users and negative for groups/channels, but we only route through this
/// when the string id already identifies a fake chat).
pub fn fake_chat_numeric_id(chat_id: &str) -> i64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    chat_id.hash(&mut hasher);
    let hashed = hasher.finish() as i64;
    let value = if hashed == 0 {
        -1
    } else if hashed > 0 {
        -hashed
    } else {
        hashed
    };
    // Keep out of the common TDLib supergroup range by forcing the high bit.
    value | i64::MIN
}

pub fn chat_id_for_username(username: &str) -> String {
    format!(
        "{TWITTER_FAKE_CHAT_PREFIX}{}",
        username.trim_start_matches('@').to_ascii_lowercase()
    )
}

pub fn read_twitter_fake_chats(settings: &SettingsStore) -> Vec<TwitterFakeChat> {
    settings
        .get_json::<Vec<TwitterFakeChat>>(SettingKey::TWITTER_FAKE_CHATS)
        .ok()
        .flatten()
        .unwrap_or_default()
}

pub fn write_twitter_fake_chats(
    settings: &SettingsStore,
    chats: &Vec<TwitterFakeChat>,
) -> plasma_core::Result<()> {
    settings.set_json(SettingKey::TWITTER_FAKE_CHATS, chats)
}

pub fn clear_twitter_fake_chats(settings: &SettingsStore) -> plasma_core::Result<()> {
    settings.remove(SettingKey::TWITTER_FAKE_CHATS)?;
    let _ = settings.remove(SettingKey::TWITTER_FAKE_PENDING_CHAT);
    Ok(())
}

pub fn get_twitter_fake_chat(settings: &SettingsStore, chat_id: &str) -> Option<TwitterFakeChat> {
    if !is_twitter_fake_chat_id(chat_id) {
        return None;
    }
    read_twitter_fake_chats(settings)
        .into_iter()
        .find(|chat| chat.id == chat_id)
}

pub fn find_twitter_fake_message(
    chat: &TwitterFakeChat,
    message_id: i64,
) -> Option<&TwitterFakeMessage> {
    chat.messages
        .iter()
        .find(|message| message.id == message_id)
}

pub fn find_twitter_fake_message_by_numeric(
    settings: &SettingsStore,
    numeric_chat_id: i64,
    message_id: i64,
) -> Option<(TwitterFakeChat, TwitterFakeMessage)> {
    for chat in read_twitter_fake_chats(settings) {
        if fake_chat_numeric_id(&chat.id) != numeric_chat_id {
            continue;
        }
        if let Some(message) = find_twitter_fake_message(&chat, message_id).cloned() {
            return Some((chat, message));
        }
    }
    None
}

pub fn twitter_fake_dialogs(settings: &SettingsStore) -> Vec<TdlibChatInfo> {
    read_twitter_fake_chats(settings)
        .into_iter()
        .map(|chat| chat.to_dialog())
        .collect()
}

pub fn merge_fake_dialogs_into(dialogs: &mut Vec<TdlibChatInfo>, settings: &SettingsStore) {
    let fake = twitter_fake_dialogs(settings);
    if fake.is_empty() {
        dialogs.retain(|chat| !is_twitter_fake_chat_id(&chat.id));
        return;
    }
    dialogs.retain(|chat| !is_twitter_fake_chat_id(&chat.id));
    let mut merged = fake;
    merged.append(dialogs);
    *dialogs = merged;
}

pub fn create_from_profile(
    settings: &SettingsStore,
    profile: &NativeProfileInfo,
) -> Result<TwitterFakeUpsertResult, TwitterFakeUpsertError> {
    let username = normalize_username(&profile.username)?;
    let media_items = profile_media_items(profile);
    if media_items.is_empty() {
        return Err(TwitterFakeUpsertError::new(
            "Nenhuma mídia encontrada para criar o chat.",
        ));
    }

    let now = unix_now();
    let chat_id = chat_id_for_username(&username);
    let original_url = if profile.original_url.trim().is_empty() {
        format!("https://x.com/{username}")
    } else {
        profile.original_url.clone()
    };
    let mut intro_lines = vec![format!("Perfil importado do Twitter/X: @{username}")];
    if let Some(name) = profile
        .display_name
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        intro_lines.push(format!("Nome: {name}"));
    }
    intro_lines.push(format!("{} mídias coletadas", media_items.len()));
    intro_lines.push(original_url.clone());
    let intro = TwitterFakeMessage {
        id: 1,
        url: None,
        thumbnail_url: None,
        is_video: false,
        date: now - (media_items.len() as i32) * 60,
        text: intro_lines.join("\n"),
        media_size: None,
    };
    let media_messages = media_items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            media_message(&username, item, (index + 2) as i64, now, media_items.len())
        })
        .collect::<Vec<_>>();

    let chat = TwitterFakeChat {
        id: chat_id.clone(),
        username: username.clone(),
        title: profile
            .display_name
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("@{username}")),
        avatar_url: profile.avatar_url.clone(),
        thumbnail_url: profile.thumbnail_url.clone(),
        original_url,
        created_at: now,
        messages: {
            let mut messages = vec![intro];
            messages.extend(media_messages);
            messages
        },
    };
    let media_count = chat
        .messages
        .iter()
        .filter(|message| message.url.is_some())
        .count();

    let mut chats = read_twitter_fake_chats(settings);
    chats.retain(|existing| existing.id != chat_id);
    chats.insert(0, chat);
    write_twitter_fake_chats(settings, &chats)
        .map_err(|error| TwitterFakeUpsertError::new(error.to_string()))?;
    let _ = settings.set(SettingKey::TWITTER_FAKE_PENDING_CHAT, &chat_id);

    Ok(TwitterFakeUpsertResult {
        chat_id,
        added_count: media_count,
        media_count,
        created: true,
    })
}

pub fn update_from_profile(
    settings: &SettingsStore,
    profile: &NativeProfileInfo,
) -> Result<TwitterFakeUpsertResult, TwitterFakeUpsertError> {
    let username = normalize_username(&profile.username)?;
    let chat_id = chat_id_for_username(&username);
    let mut chats = read_twitter_fake_chats(settings);
    let Some(index) = chats.iter().position(|chat| chat.id == chat_id) else {
        return Err(TwitterFakeUpsertError::new(
            "Chat importado do Twitter/X não encontrado.",
        ));
    };

    let media_items = profile_media_items(profile);
    let existing_urls: HashSet<String> = chats[index]
        .messages
        .iter()
        .filter_map(|message| message.url.clone())
        .collect();
    let new_items: Vec<_> = media_items
        .into_iter()
        .filter(|item| !item.url.is_empty() && !existing_urls.contains(&item.url))
        .collect();

    let chat = &mut chats[index];
    if let Some(name) = profile
        .display_name
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        chat.title = name.clone();
    }
    if profile.avatar_url.is_some() {
        chat.avatar_url = profile.avatar_url.clone();
    }
    if profile.thumbnail_url.is_some() {
        chat.thumbnail_url = profile.thumbnail_url.clone();
    }
    if !profile.original_url.trim().is_empty() {
        chat.original_url = profile.original_url.clone();
    }

    let added_count = new_items.len();
    if added_count > 0 {
        let now = unix_now();
        let next_id = chat
            .messages
            .iter()
            .map(|message| message.id)
            .max()
            .unwrap_or(0)
            + 1;
        for (offset, item) in new_items.into_iter().enumerate() {
            chat.messages.push(TwitterFakeMessage {
                id: next_id + offset as i64,
                url: Some(item.url.clone()),
                thumbnail_url: item
                    .thumbnail_url
                    .clone()
                    .or_else(|| (!item.is_video).then(|| item.url.clone())),
                is_video: item.is_video || looks_like_video_url(&item.url),
                date: now + offset as i32,
                text: format!(
                    "{} nova · https://x.com/{username}",
                    if item.is_video || looks_like_video_url(&item.url) {
                        "Vídeo"
                    } else {
                        "Imagem"
                    }
                ),
                media_size: None,
            });
        }
    }

    let media_count = chat
        .messages
        .iter()
        .filter(|message| message.url.is_some())
        .count();
    write_twitter_fake_chats(settings, &chats)
        .map_err(|error| TwitterFakeUpsertError::new(error.to_string()))?;

    Ok(TwitterFakeUpsertResult {
        chat_id,
        added_count,
        media_count,
        created: false,
    })
}

pub fn remove_twitter_fake_chat(
    settings: &SettingsStore,
    chat_id: &str,
) -> plasma_core::Result<bool> {
    if !is_twitter_fake_chat_id(chat_id) {
        return Ok(false);
    }
    let mut chats = read_twitter_fake_chats(settings);
    let before = chats.len();
    chats.retain(|chat| chat.id != chat_id);
    if chats.len() == before {
        return Ok(false);
    }
    write_twitter_fake_chats(settings, &chats)?;
    if settings
        .get(SettingKey::TWITTER_FAKE_PENDING_CHAT)
        .ok()
        .flatten()
        .as_deref()
        == Some(chat_id)
    {
        let _ = settings.remove(SettingKey::TWITTER_FAKE_PENDING_CHAT);
    }
    Ok(true)
}

/// Merge chats loaded from legacy localStorage into the SQLite store without
/// dropping any local media messages.
pub fn merge_legacy_chats(
    settings: &SettingsStore,
    legacy: Vec<TwitterFakeChat>,
) -> plasma_core::Result<usize> {
    if legacy.is_empty() {
        return Ok(0);
    }
    let mut current = read_twitter_fake_chats(settings);
    let mut by_id: HashMap<String, usize> = current
        .iter()
        .enumerate()
        .map(|(index, chat)| (chat.id.clone(), index))
        .collect();
    let mut added_chats = 0usize;
    let mut added_media = 0usize;

    for incoming in legacy {
        if !is_twitter_fake_chat_id(&incoming.id) {
            continue;
        }
        if let Some(&index) = by_id.get(&incoming.id) {
            let chat = &mut current[index];
            if chat.title.trim().is_empty() {
                chat.title = incoming.title.clone();
            }
            if chat.avatar_url.is_none() {
                chat.avatar_url = incoming.avatar_url.clone();
            }
            if chat.thumbnail_url.is_none() {
                chat.thumbnail_url = incoming.thumbnail_url.clone();
            }
            if chat.original_url.trim().is_empty() {
                chat.original_url = incoming.original_url.clone();
            }
            let existing_urls: HashSet<String> = chat
                .messages
                .iter()
                .filter_map(|message| message.url.clone())
                .collect();
            let mut next_id = chat
                .messages
                .iter()
                .map(|message| message.id)
                .max()
                .unwrap_or(0)
                + 1;
            for message in incoming.messages {
                let Some(url) = message.url.as_ref() else {
                    // Keep intro-like text messages only when the local chat
                    // has no messages yet.
                    if chat.messages.is_empty() {
                        chat.messages.push(message);
                    }
                    continue;
                };
                if existing_urls.contains(url) {
                    continue;
                }
                let mut message = message;
                if chat.messages.iter().any(|item| item.id == message.id) {
                    message.id = next_id;
                    next_id += 1;
                }
                chat.messages.push(message);
                added_media += 1;
            }
        } else {
            added_media += incoming
                .messages
                .iter()
                .filter(|message| message.url.is_some())
                .count();
            by_id.insert(incoming.id.clone(), current.len());
            current.push(incoming);
            added_chats += 1;
        }
    }

    if added_chats > 0
        || added_media > 0
        || current.len() != read_twitter_fake_chats(settings).len()
    {
        write_twitter_fake_chats(settings, &current)?;
    }
    Ok(added_chats + added_media)
}

/// Reads legacy localStorage keys from the webview and merges them into SQLite.
pub async fn migrate_twitter_fake_chats_from_local_storage(
    settings: &SettingsStore,
) -> plasma_core::Result<usize> {
    let script = r#"
        (() => {
            const keys = ["plasma_twitter_fake_chats", "plasma_twitter_pending_fake_chat"];
            const entries = {};
            for (const key of keys) {
                try {
                    const value = window.localStorage.getItem(key);
                    if (value !== null) entries[key] = value;
                } catch (_) {}
            }
            dioxus.send(entries);
        })()
    "#;
    let mut eval = dioxus::document::eval(script);
    let entries = eval
        .recv::<std::collections::HashMap<String, String>>()
        .await
        .map_err(|error| plasma_core::Error::msg(format!("Falha ao ler localStorage: {error}")))?;
    let mut added = 0usize;
    if let Some(raw) = entries.get(SettingKey::TWITTER_FAKE_CHATS.as_str()) {
        let legacy: Vec<TwitterFakeChat> = serde_json::from_str(raw).map_err(|error| {
            plasma_core::Error::msg(format!(
                "JSON inválido em plasma_twitter_fake_chats: {error}"
            ))
        })?;
        added += merge_legacy_chats(settings, legacy)?;
    }
    if let Some(pending) = entries.get(SettingKey::TWITTER_FAKE_PENDING_CHAT.as_str()) {
        if settings
            .get(SettingKey::TWITTER_FAKE_PENDING_CHAT)?
            .is_none()
            && is_twitter_fake_chat_id(pending)
        {
            settings.set(SettingKey::TWITTER_FAKE_PENDING_CHAT, pending)?;
            added += 1;
        }
    }
    if added > 0 {
        // Best-effort cleanup so the next boot does not re-merge forever.
        let cleanup = r#"
            (() => {
                try {
                    window.localStorage.removeItem("plasma_twitter_fake_chats");
                    window.localStorage.removeItem("plasma_twitter_pending_fake_chat");
                } catch (_) {}
            })()
        "#;
        let _ = dioxus::document::eval(cleanup);
    }
    Ok(added)
}

pub fn take_pending_fake_chat(settings: &SettingsStore) -> Option<String> {
    let pending = settings
        .get(SettingKey::TWITTER_FAKE_PENDING_CHAT)
        .ok()
        .flatten()
        .filter(|value| is_twitter_fake_chat_id(value))?;
    let _ = settings.remove(SettingKey::TWITTER_FAKE_PENDING_CHAT);
    Some(pending)
}

impl TwitterFakeChat {
    pub fn to_dialog(&self) -> TdlibChatInfo {
        let last = self.messages.last();
        TdlibChatInfo {
            id: self.id.clone(),
            title: self.title.clone(),
            date: last.map(|message| message.date).unwrap_or(self.created_at),
            unread_count: 0,
            is_group: false,
            is_channel: false,
            is_member: true,
            can_send_messages: false,
            can_send_media: false,
            has_topics: false,
            last_message_id: last.map(|message| message.id),
            last_message_text: last
                .map(|message| message.text.clone())
                .unwrap_or_else(|| format!("{} mídias importadas do X", self.media_count())),
            last_message_date: last.map(|message| message.date).unwrap_or(self.created_at),
            last_message_has_media: last.and_then(|message| message.url.as_ref()).is_some(),
            last_message_is_video: last.is_some_and(|message| message.is_video),
            last_message_is_photo: last
                .is_some_and(|message| message.url.is_some() && !message.is_video),
        }
    }

    pub fn media_count(&self) -> usize {
        self.messages
            .iter()
            .filter(|message| message.url.is_some())
            .count()
    }

    pub fn media_messages(&self) -> Vec<&TwitterFakeMessage> {
        self.messages
            .iter()
            .filter(|message| message.url.as_ref().is_some_and(|url| !url.is_empty()))
            .collect()
    }

    pub fn messages_as_tdlib(&self) -> Vec<TdlibMessageInfo> {
        self.messages
            .iter()
            .map(|message| message.to_tdlib(&self.id, &self.title))
            .collect()
    }

    pub fn thumbnail_seed(&self) -> HashMap<(i64, i64), String> {
        let numeric = fake_chat_numeric_id(&self.id);
        let mut map = HashMap::new();
        for message in &self.messages {
            let Some(url) = message
                .thumbnail_url
                .as_ref()
                .or(message.url.as_ref())
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if message.is_video && message.thumbnail_url.is_none() {
                continue;
            }
            map.insert((numeric, message.id), url.clone());
        }
        map
    }
}

impl TwitterFakeMessage {
    pub fn to_tdlib(&self, chat_id: &str, sender_name: &str) -> TdlibMessageInfo {
        let has_media = self.url.as_ref().is_some_and(|url| !url.is_empty());
        TdlibMessageInfo {
            id: self.id,
            message: self.text.clone(),
            date: self.date,
            out: false,
            sender_id: chat_id.to_string(),
            sender_name: sender_name.to_string(),
            reply_to_msg_id: None,
            topic_id: None,
            topic_kind: None,
            media: has_media,
            text: self.text.clone(),
            has_media,
            is_photo: has_media && !self.is_video,
            is_video: has_media && self.is_video,
            grouped_id: None,
            video_duration: None,
            media_size: self.media_size,
            thumbnail_path: self.thumbnail_url.clone().or_else(|| {
                if has_media && !self.is_video {
                    self.url.clone()
                } else {
                    None
                }
            }),
            is_deleted: false,
            reactions: Vec::new(),
        }
    }
}

fn profile_media_items(profile: &NativeProfileInfo) -> Vec<NativeProfileMediaItem> {
    if !profile.media_items.is_empty() {
        return profile
            .media_items
            .iter()
            .filter(|item| !item.url.trim().is_empty())
            .cloned()
            .collect();
    }
    profile
        .media_urls
        .iter()
        .filter(|url| !url.trim().is_empty())
        .map(|url| NativeProfileMediaItem {
            url: url.clone(),
            thumbnail_url: if looks_like_video_url(url) {
                profile.thumbnail_url.clone()
            } else {
                Some(url.clone())
            },
            is_video: looks_like_video_url(url),
        })
        .collect()
}

fn media_message(
    username: &str,
    item: &NativeProfileMediaItem,
    id: i64,
    now: i32,
    total: usize,
) -> TwitterFakeMessage {
    let is_video = item.is_video || looks_like_video_url(&item.url);
    let index = (id - 1).max(1) as usize;
    TwitterFakeMessage {
        id,
        url: Some(item.url.clone()),
        thumbnail_url: item
            .thumbnail_url
            .clone()
            .or_else(|| (!is_video).then(|| item.url.clone())),
        is_video,
        date: now - (total.saturating_sub(index) as i32) * 60,
        text: format!(
            "{} {index} de {total} · https://x.com/{username}",
            if is_video { "Vídeo" } else { "Imagem" }
        ),
        media_size: None,
    }
}

fn normalize_username(username: &str) -> Result<String, TwitterFakeUpsertError> {
    let username = username.trim().trim_start_matches('@');
    if username.is_empty() {
        return Err(TwitterFakeUpsertError::new("Perfil do Twitter/X inválido."));
    }
    Ok(username.to_string())
}

fn looks_like_video_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.contains("video.twimg.com/") || lower.contains(".mp4?") || lower.ends_with(".mp4")
}

fn unix_now() -> i32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i32)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plasma_store::SettingsStore;

    fn store() -> (tempfile::TempDir, SettingsStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.sqlite3");
        let store = SettingsStore::open_at(&path).expect("open settings");
        (dir, store)
    }

    fn profile(username: &str, urls: &[&str]) -> NativeProfileInfo {
        NativeProfileInfo {
            platform: "twitter".into(),
            username: username.into(),
            display_name: Some(format!("Name {username}")),
            avatar_url: Some("https://example.com/avatar.jpg".into()),
            thumbnail_url: None,
            media_count: urls.len(),
            media_urls: urls.iter().map(|url| (*url).to_string()).collect(),
            media_items: urls
                .iter()
                .map(|url| NativeProfileMediaItem {
                    url: (*url).to_string(),
                    thumbnail_url: Some((*url).to_string()),
                    is_video: looks_like_video_url(url),
                })
                .collect(),
            original_url: format!("https://x.com/{username}"),
        }
    }

    #[test]
    fn create_and_update_are_append_only() {
        let (_dir, settings) = store();
        let created = create_from_profile(
            &settings,
            &profile(
                "alice",
                &["https://pbs.twimg.com/a.jpg", "https://pbs.twimg.com/b.jpg"],
            ),
        )
        .expect("create");
        assert!(created.created);
        assert_eq!(created.media_count, 2);

        let updated = update_from_profile(
            &settings,
            &profile(
                "alice",
                &["https://pbs.twimg.com/b.jpg", "https://pbs.twimg.com/c.jpg"],
            ),
        )
        .expect("update");
        assert_eq!(updated.added_count, 1);
        assert_eq!(updated.media_count, 3);

        let chat = get_twitter_fake_chat(&settings, &created.chat_id).expect("chat");
        let urls: HashSet<_> = chat
            .messages
            .iter()
            .filter_map(|message| message.url.clone())
            .collect();
        assert!(urls.contains("https://pbs.twimg.com/a.jpg"));
        assert!(urls.contains("https://pbs.twimg.com/b.jpg"));
        assert!(urls.contains("https://pbs.twimg.com/c.jpg"));
    }

    #[test]
    fn merge_legacy_keeps_local_media() {
        let (_dir, settings) = store();
        create_from_profile(
            &settings,
            &profile("bob", &["https://pbs.twimg.com/local.jpg"]),
        )
        .expect("create");

        let legacy = vec![TwitterFakeChat {
            id: chat_id_for_username("bob"),
            username: "bob".into(),
            title: "Bob".into(),
            avatar_url: None,
            thumbnail_url: None,
            original_url: "https://x.com/bob".into(),
            created_at: 1,
            messages: vec![
                TwitterFakeMessage {
                    id: 2,
                    url: Some("https://pbs.twimg.com/legacy.jpg".into()),
                    thumbnail_url: Some("https://pbs.twimg.com/legacy.jpg".into()),
                    is_video: false,
                    date: 2,
                    text: "legacy".into(),
                    media_size: None,
                },
                TwitterFakeMessage {
                    id: 3,
                    url: Some("https://pbs.twimg.com/local.jpg".into()),
                    thumbnail_url: None,
                    is_video: false,
                    date: 3,
                    text: "dup".into(),
                    media_size: None,
                },
            ],
        }];
        let added = merge_legacy_chats(&settings, legacy).expect("merge");
        assert_eq!(added, 1);
        let chat = get_twitter_fake_chat(&settings, &chat_id_for_username("bob")).expect("chat");
        assert_eq!(chat.media_count(), 2);
    }

    #[test]
    fn numeric_id_is_stable_and_negative() {
        let id = chat_id_for_username("carol");
        let a = fake_chat_numeric_id(&id);
        let b = fake_chat_numeric_id(&id);
        assert_eq!(a, b);
        assert!(a < 0);
    }
}
