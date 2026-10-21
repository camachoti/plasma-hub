//! SQLite connection handling and schema migrations.
//!
//! The database file and its schema are shared with the previous Tauri build:
//! `plasma-hub.sqlite3` lives in the app data directory and uses the same
//! tables. Migrations are tracked with SQLite's `user_version`, so existing
//! databases are upgraded in place instead of being recreated.

use plasma_core::{AppEnv, Error, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DB_FILE_NAME: &str = "plasma-hub.sqlite3";

/// Highest schema version known by this build.
pub const SCHEMA_VERSION: i64 = 2;

const SCHEMA_V1: &str = r#"
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

CREATE TABLE IF NOT EXISTS download_history (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    raw_json TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS topic_download_state (
    chat_id TEXT NOT NULL,
    topic_id TEXT NOT NULL,
    topic_kind TEXT NOT NULL,
    last_message_id INTEGER NOT NULL,
    downloaded_at INTEGER NOT NULL,
    PRIMARY KEY (chat_id, topic_id, topic_kind)
);
"#;

const SCHEMA_V2: &str = r#"
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
"#;

const MIGRATIONS: &[(i64, &str)] = &[(1, SCHEMA_V1), (2, SCHEMA_V2)];

/// A database handle, identified by its file path.
///
/// Connections are short-lived and opened per operation for now. Keeping the
/// path in a cheap, cloneable value lets the UI and services share the same
/// database without passing a `rusqlite::Connection` across threads.
#[derive(Debug, Clone)]
pub struct Db {
    path: PathBuf,
}

impl Db {
    pub fn open(app: &AppEnv) -> Result<Self> {
        Self::open_at(db_path(app)?)
    }

    pub fn open_at(path: impl Into<PathBuf>) -> Result<Self> {
        let db = Self { path: path.into() };
        // Fail early (and run migrations) instead of surfacing a broken
        // database on the first read.
        db.connect()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn connect(&self) -> Result<Connection> {
        connect_path(&self.path)
    }
}

/// Resolves the shared database file, creating the data directory if needed.
pub fn db_path(app: &AppEnv) -> Result<PathBuf> {
    let dir = app.paths().data_dir();
    std::fs::create_dir_all(dir)?;
    Ok(dir.join(DB_FILE_NAME))
}

/// Opens a connection, applies the required pragmas, and runs migrations.
pub fn connect(app: &AppEnv) -> Result<Connection> {
    connect_path(&db_path(app)?)
}

/// Opens a connection at `path`, applying pragmas and migrations.
pub fn connect_path(path: &Path) -> Result<Connection> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }

    let mut connection = Connection::open(path).map_err(db_error)?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(db_error)?;
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(db_error)?;
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(db_error)?;

    migrate(&mut connection)?;
    Ok(connection)
}

/// Applies all pending migrations in order.
pub fn migrate(connection: &mut Connection) -> Result<()> {
    let current: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(db_error)?;

    if current > SCHEMA_VERSION {
        return Err(Error::msg(format!(
            "banco criado por uma versão mais nova do Plasma Hub (schema {current}, suportado {SCHEMA_VERSION})"
        )));
    }

    for (version, sql) in MIGRATIONS {
        if current >= *version {
            continue;
        }

        let transaction = connection.transaction().map_err(db_error)?;
        transaction.execute_batch(sql).map_err(db_error)?;
        transaction
            .pragma_update(None, "user_version", version)
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)?;
    }

    Ok(())
}

pub(crate) fn db_error(error: rusqlite::Error) -> Error {
    Error::msg(error.to_string())
}

pub(crate) fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn temp_db() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(DB_FILE_NAME);
        (dir, path)
    }

    #[test]
    fn creates_schema_and_sets_user_version() {
        let (_dir, path) = temp_db();
        let connection = connect_path(&path).expect("connect");

        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("user_version");
        assert_eq!(version, SCHEMA_VERSION);

        for table in [
            "telegram_message_pages",
            "telegram_messages",
            "download_history",
            "topic_download_state",
            "settings",
        ] {
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    params![table],
                    |row| row.get(0),
                )
                .expect("table lookup");
            assert_eq!(count, 1, "missing table {table}");
        }
    }

    #[test]
    fn upgrades_a_v0_database_without_losing_rows() {
        let (_dir, path) = temp_db();
        {
            let connection = Connection::open(&path).expect("open legacy db");
            connection
                .execute_batch(
                    r#"
                    CREATE TABLE telegram_message_pages (
                        cache_key TEXT PRIMARY KEY,
                        last_fetched_at INTEGER,
                        updated_at INTEGER NOT NULL
                    );
                    CREATE TABLE telegram_messages (
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
                    INSERT INTO telegram_messages
                        (cache_key, message_id, raw_json, updated_at)
                        VALUES ('chat:1', 42, '{"id":42}', 1);
                    "#,
                )
                .expect("legacy schema");
            let version: i64 = connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .expect("user_version");
            assert_eq!(version, 0);
        }

        let connection = connect_path(&path).expect("migrate");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("user_version");
        assert_eq!(version, SCHEMA_VERSION);

        let raw: String = connection
            .query_row(
                "SELECT raw_json FROM telegram_messages WHERE cache_key = 'chat:1' AND message_id = 42",
                [],
                |row| row.get(0),
            )
            .expect("preserved row");
        assert_eq!(raw, r#"{"id":42}"#);

        let settings_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))
            .expect("settings table");
        assert_eq!(settings_count, 0);
    }

    #[test]
    fn rejects_a_future_schema() {
        let (_dir, path) = temp_db();
        {
            let connection = Connection::open(&path).expect("open db");
            connection
                .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .expect("set future version");
        }

        let error = connect_path(&path).expect_err("must reject future schema");
        assert!(error.to_string().contains("versão mais nova"));
    }

    #[test]
    fn migrations_are_idempotent() {
        let (_dir, path) = temp_db();
        connect_path(&path).expect("first open");
        let connection = connect_path(&path).expect("second open");
        let version: i64 = connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .expect("user_version");
        assert_eq!(version, SCHEMA_VERSION);
    }
}
