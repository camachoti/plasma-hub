use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct MessageCacheMeta {
    last_fetched_at: Option<i64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageCacheGetResult {
    success: bool,
    messages: Vec<Value>,
    meta: MessageCacheMeta,
    error: Option<String>,
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

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    Ok(dir.join("plasma-hub.sqlite3"))
}

fn connect(app: &AppHandle) -> Result<Connection, String> {
    let connection = Connection::open(db_path(app)?).map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(|error| error.to_string())?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(|error| error.to_string())?;
    ensure_schema(&connection)?;
    Ok(connection)
}

fn ensure_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS telegram_message_pages (
                cache_key TEXT PRIMARY KEY,
                last_fetched_at INTEGER,
                updated_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS telegram_messages (
                cache_key TEXT NOT NULL,
                message_id INTEGER NOT NULL,
                date INTEGER,
                has_media INTEGER NOT NULL DEFAULT 0,
                is_video INTEGER NOT NULL DEFAULT 0,
                sender_id TEXT,
                sender_name TEXT,
                raw_json TEXT NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY (cache_key, message_id)
            );

            CREATE INDEX IF NOT EXISTS idx_telegram_messages_cache_date
                ON telegram_messages(cache_key, date);

            CREATE INDEX IF NOT EXISTS idx_telegram_messages_cache_media
                ON telegram_messages(cache_key, has_media, is_video, date);
            "#,
        )
        .map_err(|error| error.to_string())
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
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

fn read_meta(connection: &Connection, cache_key: &str) -> Result<MessageCacheMeta, String> {
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
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub fn telegram_message_cache_get(
    app: AppHandle,
    cache_key: String,
) -> Result<MessageCacheGetResult, String> {
    let connection = connect(&app)?;
    let meta = read_meta(&connection, &cache_key)?;
    let mut statement = connection
        .prepare(
            "SELECT raw_json FROM telegram_messages WHERE cache_key = ?1 ORDER BY message_id ASC",
        )
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![cache_key], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?;

    let mut messages = Vec::new();
    for row in rows {
        let raw = row.map_err(|error| error.to_string())?;
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

#[tauri::command]
pub fn telegram_message_cache_meta(
    app: AppHandle,
    cache_key: String,
) -> Result<MessageCacheMeta, String> {
    let connection = connect(&app)?;
    read_meta(&connection, &cache_key)
}

#[tauri::command]
pub fn telegram_message_cache_save(
    app: AppHandle,
    cache_key: String,
    messages: Vec<Value>,
    meta: Option<MessageCacheMeta>,
) -> Result<MessageCacheSaveResult, String> {
    let mut connection = connect(&app)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let updated_at = now_millis();

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
            .map_err(|error| error.to_string())?;

        for message in &messages {
            let Some(message_id) = value_i64(message, "id") else {
                continue;
            };
            let raw_json = serde_json::to_string(message).map_err(|error| error.to_string())?;
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
                .map_err(|error| error.to_string())?;
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
            .map_err(|error| error.to_string())?;
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
            .map_err(|error| error.to_string())?;
    }

    transaction.commit().map_err(|error| error.to_string())?;

    Ok(MessageCacheSaveResult {
        success: true,
        saved_count: messages.len(),
        error: None,
    })
}

#[tauri::command]
pub fn telegram_message_cache_shared_media(
    app: AppHandle,
    cache_key: String,
    limit: i64,
) -> Result<SharedMediaCacheResult, String> {
    let connection = connect(&app)?;
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
        .map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![cache_key, limit.max(1)], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| error.to_string())?;

    let mut media = Vec::new();
    for row in rows {
        let raw = row.map_err(|error| error.to_string())?;
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
