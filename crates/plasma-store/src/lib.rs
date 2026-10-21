pub mod db;
pub mod settings;

pub use db::Db;
pub use settings::{SettingKey, SettingsStore};

use db::{db_error, now_millis};
use plasma_core::{AppEnv, Error, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_DOWNLOAD_HISTORY_ITEMS: usize = 300;
const MAX_DOWNLOAD_HISTORY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MessageCacheMeta {
    pub last_fetched_at: Option<i64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCacheGetResult {
    pub success: bool,
    pub messages: Vec<Value>,
    pub meta: MessageCacheMeta,
    pub error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCacheSaveResult {
    success: bool,
    saved_count: usize,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedMediaCacheResult {
    success: bool,
    media: Vec<Value>,
    error: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TopicDownloadState {
    topic_id: String,
    topic_kind: String,
    last_message_id: i64,
    downloaded_at: i64,
}

pub fn record_topic_mass_download(
    app: &AppEnv,
    chat_id: i64,
    topic_id: i64,
    topic_kind: &str,
    last_message_id: i64,
) -> Result<()> {
    let connection = db::connect(app)?;
    connection.execute(
        "INSERT INTO topic_download_state (chat_id, topic_id, topic_kind, last_message_id, downloaded_at)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(chat_id, topic_id, topic_kind) DO UPDATE SET
           last_message_id = MAX(topic_download_state.last_message_id, excluded.last_message_id),
           downloaded_at = excluded.downloaded_at",
        params![chat_id.to_string(), topic_id.to_string(), topic_kind, last_message_id, now_millis()],
    )
    .map_err(db_error)?;
    Ok(())
}

pub fn topic_download_state_get(app: &AppEnv, chat_id: String) -> Result<Vec<TopicDownloadState>> {
    let connection = db::connect(app)?;
    let mut statement = connection
        .prepare(
            "SELECT topic_id, topic_kind, last_message_id, downloaded_at
         FROM topic_download_state WHERE chat_id = ?1",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![chat_id], |row| {
            Ok(TopicDownloadState {
                topic_id: row.get(0)?,
                topic_kind: row.get(1)?,
                last_message_id: row.get(2)?,
                downloaded_at: row.get(3)?,
            })
        })
        .map_err(db_error)?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(db_error)
}

pub fn download_history_get(app: &AppEnv) -> Result<Option<String>> {
    let connection = db::connect(app)?;
    match connection.query_row(
        "SELECT raw_json FROM download_history WHERE singleton = 1",
        [],
        |row| row.get::<_, String>(0),
    ) {
        Ok(history) => Ok(Some(history)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(error) => Err(db_error(error)),
    }
}

pub fn download_history_save(app: &AppEnv, history_json: String) -> Result<()> {
    if history_json.len() > MAX_DOWNLOAD_HISTORY_BYTES {
        return Err(Error::msg(
            "Histórico de downloads excede o limite permitido.",
        ));
    }
    let items = serde_json::from_str::<Vec<Value>>(&history_json)
        .map_err(|_| Error::msg("Histórico de downloads inválido."))?;
    if items.len() > MAX_DOWNLOAD_HISTORY_ITEMS {
        return Err(Error::msg(
            "Histórico de downloads excede o limite de itens.",
        ));
    }

    let connection = db::connect(app)?;
    connection
        .execute(
            r#"
            INSERT INTO download_history (singleton, raw_json, updated_at)
            VALUES (1, ?1, ?2)
            ON CONFLICT(singleton) DO UPDATE SET
                raw_json = excluded.raw_json,
                updated_at = excluded.updated_at
            "#,
            params![history_json, now_millis()],
        )
        .map_err(db_error)?;
    Ok(())
}

fn value_i64(value: &Value, key: &str) -> Option<i64> {
    value.get(key).and_then(|item| {
        item.as_i64()
            .or_else(|| item.as_u64().and_then(|number| i64::try_from(number).ok()))
            .or_else(|| item.as_str().and_then(|text| text.parse::<i64>().ok()))
    })
}

fn value_bool(value: &Value, key: &str) -> bool {
    value
        .get(key)
        .and_then(|item| {
            item.as_bool()
                .or_else(|| item.as_i64().map(|number| number != 0))
        })
        .unwrap_or(false)
}

fn value_string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(|item| {
        item.as_str()
            .map(ToString::to_string)
            .or_else(|| item.as_i64().map(|number| number.to_string()))
            .or_else(|| item.as_u64().map(|number| number.to_string()))
    })
}

fn read_meta(connection: &Connection, cache_key: &str) -> Result<MessageCacheMeta> {
    let result = connection.query_row(
        "SELECT last_fetched_at FROM telegram_message_pages WHERE cache_key = ?1",
        params![cache_key],
        |row| {
            Ok(MessageCacheMeta {
                last_fetched_at: row.get::<_, Option<i64>>(0)?,
            })
        },
    );

    match result {
        Ok(meta) => Ok(meta),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(MessageCacheMeta::default()),
        Err(error) => Err(db_error(error)),
    }
}

pub fn telegram_message_cache_get(
    app: &AppEnv,
    cache_key: String,
) -> Result<MessageCacheGetResult> {
    let connection = db::connect(app)?;
    let meta = read_meta(&connection, &cache_key)?;
    let mut statement = connection
        .prepare(
            "SELECT raw_json FROM telegram_messages WHERE cache_key = ?1 ORDER BY message_id ASC",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![cache_key], |row| row.get::<_, String>(0))
        .map_err(db_error)?;

    let mut messages = Vec::new();
    for row in rows {
        let raw = row.map_err(db_error)?;
        if let Ok(value) = serde_json::from_str::<Value>(&raw) {
            messages.push(value);
        }
    }

    Ok(MessageCacheGetResult {
        success: true,
        messages,
        meta,
        error: None,
    })
}

pub fn telegram_message_cache_meta(app: &AppEnv, cache_key: String) -> Result<MessageCacheMeta> {
    let connection = db::connect(app)?;
    read_meta(&connection, &cache_key)
}

pub fn telegram_message_cache_save(
    app: &AppEnv,
    cache_key: String,
    messages: Vec<Value>,
    meta: Option<MessageCacheMeta>,
) -> Result<MessageCacheSaveResult> {
    let mut connection = db::connect(app)?;
    let transaction = connection.transaction().map_err(db_error)?;
    let updated_at = now_millis();

    transaction
        .execute(
            "DELETE FROM telegram_messages WHERE cache_key = ?1",
            params![cache_key],
        )
        .map_err(db_error)?;

    {
        let mut statement = transaction
            .prepare(
                r#"
                INSERT INTO telegram_messages (
                    cache_key, message_id, date, has_media, is_video,
                    sender_id, sender_name, raw_json, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(cache_key, message_id) DO UPDATE SET
                    date = excluded.date,
                    has_media = excluded.has_media,
                    is_video = excluded.is_video,
                    sender_id = excluded.sender_id,
                    sender_name = excluded.sender_name,
                    raw_json = excluded.raw_json,
                    updated_at = excluded.updated_at
                "#,
            )
            .map_err(db_error)?;

        for message in &messages {
            let Some(message_id) = value_i64(message, "id") else {
                continue;
            };
            let raw_json = serde_json::to_string(message)?;
            statement
                .execute(params![
                    cache_key,
                    message_id,
                    value_i64(message, "date"),
                    value_bool(message, "hasMedia") as i64,
                    value_bool(message, "isVideo") as i64,
                    value_string(message, "senderId"),
                    value_string(message, "senderName"),
                    raw_json,
                    updated_at,
                ])
                .map_err(db_error)?;
        }
    }

    if let Some(meta) = meta {
        transaction
            .execute(
                r#"
                INSERT INTO telegram_message_pages (cache_key, last_fetched_at, updated_at)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(cache_key) DO UPDATE SET
                    last_fetched_at = COALESCE(excluded.last_fetched_at, telegram_message_pages.last_fetched_at),
                    updated_at = excluded.updated_at
                "#,
                params![cache_key, meta.last_fetched_at, updated_at],
            )
            .map_err(db_error)?;
    } else {
        transaction
            .execute(
                r#"
                INSERT INTO telegram_message_pages (cache_key, last_fetched_at, updated_at)
                VALUES (?1, NULL, ?2)
                ON CONFLICT(cache_key) DO UPDATE SET updated_at = excluded.updated_at
                "#,
                params![cache_key, updated_at],
            )
            .map_err(db_error)?;
    }

    transaction.commit().map_err(db_error)?;

    Ok(MessageCacheSaveResult {
        success: true,
        saved_count: messages.len(),
        error: None,
    })
}

/// Marks messages as deleted in every cached page of a chat, including forum
/// topic keys. TDLib stops returning deleted messages, so the marker has to be
/// persisted separately to keep the local timeline stable across restarts.
pub fn telegram_message_cache_mark_deleted(
    app: &AppEnv,
    chat_id: &str,
    message_ids: &[i64],
) -> Result<usize> {
    use std::collections::HashSet;

    if message_ids.is_empty() {
        return Ok(0);
    }

    let ids: HashSet<i64> = message_ids.iter().copied().collect();
    let mut connection = db::connect(app)?;
    let transaction = connection.transaction().map_err(db_error)?;
    let pattern = format!("{chat_id}::%");

    let mut updates = Vec::new();
    {
        let mut statement = transaction
            .prepare(
                "SELECT cache_key, message_id, raw_json FROM telegram_messages                  WHERE cache_key = ?1 OR cache_key LIKE ?2",
            )
            .map_err(db_error)?;
        let mut rows = statement
            .query(params![chat_id, pattern])
            .map_err(db_error)?;
        while let Some(row) = rows.next().map_err(db_error)? {
            let cache_key: String = row.get(0).map_err(db_error)?;
            let message_id: i64 = row.get(1).map_err(db_error)?;
            if !ids.contains(&message_id) {
                continue;
            }
            let raw: String = row.get(2).map_err(db_error)?;
            let Ok(mut value) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            if value
                .get("isDeleted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let Some(object) = value.as_object_mut() else {
                continue;
            };
            object.insert("isDeleted".to_string(), Value::Bool(true));
            updates.push((cache_key, message_id, serde_json::to_string(&value)?));
        }
    }

    let updated_at = now_millis();
    for (cache_key, message_id, raw) in &updates {
        transaction
            .execute(
                "UPDATE telegram_messages SET raw_json = ?1, updated_at = ?2                  WHERE cache_key = ?3 AND message_id = ?4",
                params![raw, updated_at, cache_key, message_id],
            )
            .map_err(db_error)?;
    }

    transaction.commit().map_err(db_error)?;
    Ok(updates.len())
}

pub fn telegram_message_cache_shared_media(
    app: &AppEnv,
    cache_key: String,
    limit: i64,
) -> Result<SharedMediaCacheResult> {
    let connection = db::connect(app)?;
    let mut statement = connection
        .prepare(
            r#"
            SELECT raw_json
            FROM telegram_messages
            WHERE cache_key = ?1 AND has_media = 1
            ORDER BY date DESC, message_id DESC
            LIMIT ?2
            "#,
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map(params![cache_key, limit.max(1)], |row| {
            row.get::<_, String>(0)
        })
        .map_err(db_error)?;

    let mut media = Vec::new();
    for row in rows {
        let raw = row.map_err(db_error)?;
        let Ok(message) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(message_id) = value_i64(&message, "id") else {
            continue;
        };
        media.push(serde_json::json!({
            "id": message_id,
            "isVideo": value_bool(&message, "isVideo"),
            "mediaSize": value_i64(&message, "mediaSize"),
        }));
    }

    media.reverse();

    Ok(SharedMediaCacheResult {
        success: true,
        media,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleted_messages_are_marked_in_chat_and_topic_pages() {
        use plasma_core::AppPaths;

        let root =
            std::env::temp_dir().join(format!("plasma-store-deleted-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = AppPaths::new(
            root.join("data"),
            root.join("local"),
            root.join("cache"),
            Some(root.join("downloads")),
        )
        .unwrap();
        let env = AppEnv::headless(paths);

        let message = serde_json::json!({
            "id": 7,
            "date": 10,
            "hasMedia": false,
            "isDeleted": false,
        });
        telegram_message_cache_save(&env, "123".to_string(), vec![message.clone()], None).unwrap();
        telegram_message_cache_save(&env, "123::forum:5".to_string(), vec![message], None).unwrap();

        assert_eq!(
            telegram_message_cache_mark_deleted(&env, "123", &[7]).unwrap(),
            2
        );
        for key in ["123", "123::forum:5"] {
            let page = telegram_message_cache_get(&env, key.to_string()).unwrap();
            assert_eq!(page.messages[0]["isDeleted"], true);
        }
        assert_eq!(
            telegram_message_cache_mark_deleted(&env, "123", &[7]).unwrap(),
            0
        );

        let _ = std::fs::remove_dir_all(env.paths().data_dir().parent().unwrap());
    }

    #[test]
    fn schema_persists_one_download_history_snapshot() {
        let mut connection = Connection::open_in_memory().unwrap();
        db::migrate(&mut connection).unwrap();
        let first = r#"[{"id":"first"}]"#;
        let second = r#"[{"id":"second"}]"#;

        connection
            .execute(
                r#"
                INSERT INTO download_history (singleton, raw_json, updated_at)
                VALUES (1, ?1, 1)
                ON CONFLICT(singleton) DO UPDATE SET raw_json = excluded.raw_json
                "#,
                params![first],
            )
            .unwrap();
        connection
            .execute(
                r#"
                INSERT INTO download_history (singleton, raw_json, updated_at)
                VALUES (1, ?1, 2)
                ON CONFLICT(singleton) DO UPDATE SET raw_json = excluded.raw_json
                "#,
                params![second],
            )
            .unwrap();

        let stored: String = connection
            .query_row(
                "SELECT raw_json FROM download_history WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, second);
    }
}
