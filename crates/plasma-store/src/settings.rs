//! Typed key/value settings persisted in SQLite.
//!
//! This replaces the webview `localStorage` store for global preferences. The
//! key strings intentionally match the existing frontend keys so the one-shot
//! migration (performed when the Rust UI starts) can copy values without
//! rewriting them.

use crate::db::{self, Db};
use plasma_core::{AppEnv, Error, Result};
use rusqlite::{params, OptionalExtension};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// A stable settings key.
///
/// Known global preferences are exposed as associated constants. Dynamic keys
/// (for example, per-chat timeline anchors during the migration) can use
/// [`SettingsStore::get_raw`] and [`SettingsStore::set_raw`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SettingKey(&'static str);

impl SettingKey {
    pub const SKIP_LOGIN: Self = Self("skip_login");
    pub const TELEGRAM_TDLIB_SESSION: Self = Self("telegram_tdlib_session");
    pub const APPEARANCE_PALETTE: Self = Self("plasma_appearance_palette");
    pub const APPEARANCE_DENSITY: Self = Self("plasma_appearance_density");
    pub const INFO_PANEL_OPEN: Self = Self("plasma_info_panel_open");
    pub const CHAT_MEDIA_SIZE: Self = Self("plasma_chat_media_size");
    pub const TWITTER_COOKIES: Self = Self("plasma_twitter_optional_cookies");
    pub const CLEAR_TWITTER_GROUPS_ON_CLEAR_CACHE: Self =
        Self("clear_twitter_groups_on_clear_cache");

    pub const fn from_static(key: &'static str) -> Self {
        Self(key)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl std::fmt::Display for SettingKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl From<SettingKey> for String {
    fn from(key: SettingKey) -> Self {
        key.0.to_string()
    }
}

/// SQLite-backed settings store.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    db: Db,
}

impl SettingsStore {
    pub fn open(app: &AppEnv) -> Result<Self> {
        Ok(Self { db: Db::open(app)? })
    }

    pub fn open_at(path: impl Into<PathBuf>) -> Result<Self> {
        Ok(Self {
            db: Db::open_at(path.into())?,
        })
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    pub fn get(&self, key: SettingKey) -> Result<Option<String>> {
        self.get_raw(key.as_str())
    }

    pub fn set(&self, key: SettingKey, value: &str) -> Result<()> {
        self.set_raw(key.as_str(), value)
    }

    pub fn remove(&self, key: SettingKey) -> Result<()> {
        self.remove_raw(key.as_str())
    }

    pub fn get_bool(&self, key: SettingKey) -> Result<bool> {
        Ok(self.get(key)?.as_deref() == Some("true"))
    }

    pub fn set_bool(&self, key: SettingKey, value: bool) -> Result<()> {
        self.set(key, if value { "true" } else { "false" })
    }

    pub fn get_json<T: DeserializeOwned>(&self, key: SettingKey) -> Result<Option<T>> {
        match self.get(key)? {
            Some(raw) => serde_json::from_str(&raw)
                .map(Some)
                .map_err(|error| Error::msg(format!("setting {}: {error}", key.as_str()))),
            None => Ok(None),
        }
    }

    pub fn set_json<T: Serialize>(&self, key: SettingKey, value: &T) -> Result<()> {
        let raw = serde_json::to_string(value)?;
        self.set(key, &raw)
    }

    pub fn get_raw(&self, key: &str) -> Result<Option<String>> {
        let connection = self.db.connect()?;
        connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db::db_error)
    }

    pub fn set_raw(&self, key: &str, value: &str) -> Result<()> {
        let connection = self.db.connect()?;
        connection
            .execute(
                r#"
                INSERT INTO settings (key, value, updated_at)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    updated_at = excluded.updated_at
                "#,
                params![key, value, db::now_millis()],
            )
            .map_err(db::db_error)?;
        Ok(())
    }

    pub fn remove_raw(&self, key: &str) -> Result<()> {
        let connection = self.db.connect()?;
        connection
            .execute("DELETE FROM settings WHERE key = ?1", params![key])
            .map_err(db::db_error)?;
        Ok(())
    }

    pub fn snapshot(&self) -> Result<BTreeMap<String, String>> {
        let connection = self.db.connect()?;
        let mut statement = connection
            .prepare("SELECT key, value FROM settings ORDER BY key")
            .map_err(db::db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(db::db_error)?;

        let mut values = BTreeMap::new();
        for row in rows {
            let (key, value) = row.map_err(db::db_error)?;
            values.insert(key, value);
        }
        Ok(values)
    }

    /// Imports values read from the legacy webview `localStorage`.
    ///
    /// Existing SQLite values always win, so the one-shot migration can be
    /// retried safely. Returns the number of imported entries.
    pub fn import_legacy<I>(&self, entries: I) -> Result<usize>
    where
        I: IntoIterator<Item = (String, String)>,
    {
        let mut connection = self.db.connect()?;
        let transaction = connection.transaction().map_err(db::db_error)?;
        let updated_at = db::now_millis();
        let mut imported = 0;

        for (key, value) in entries {
            let changed = transaction
                .execute(
                    r#"
                    INSERT INTO settings (key, value, updated_at)
                    VALUES (?1, ?2, ?3)
                    ON CONFLICT(key) DO NOTHING
                    "#,
                    params![key, value, updated_at],
                )
                .map_err(db::db_error)?;
            imported += changed;
        }

        transaction.commit().map_err(db::db_error)?;
        Ok(imported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, SettingsStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(db::DB_FILE_NAME);
        let store = SettingsStore::open_at(path).expect("open settings");
        (dir, store)
    }

    #[test]
    fn round_trips_raw_and_typed_values() {
        let (_dir, store) = store();

        assert_eq!(store.get(SettingKey::SKIP_LOGIN).expect("get"), None);
        store.set(SettingKey::SKIP_LOGIN, "true").expect("set");
        assert_eq!(
            store.get(SettingKey::SKIP_LOGIN).expect("get"),
            Some("true".to_string())
        );
        assert!(store.get_bool(SettingKey::SKIP_LOGIN).expect("bool"));

        store
            .set(SettingKey::APPEARANCE_PALETTE, "ember")
            .expect("set palette");
        assert_eq!(
            store
                .get(SettingKey::APPEARANCE_PALETTE)
                .expect("get palette")
                .as_deref(),
            Some("ember")
        );

        store
            .set_json(SettingKey::INFO_PANEL_OPEN, &true)
            .expect("set json");
        assert_eq!(
            store
                .get_json::<bool>(SettingKey::INFO_PANEL_OPEN)
                .expect("get json"),
            Some(true)
        );

        store
            .set_bool(SettingKey::SKIP_LOGIN, false)
            .expect("unset");
        assert!(!store.get_bool(SettingKey::SKIP_LOGIN).expect("bool"));
        store.remove(SettingKey::SKIP_LOGIN).expect("remove");
        assert_eq!(store.get(SettingKey::SKIP_LOGIN).expect("get"), None);
    }

    #[test]
    fn snapshot_is_sorted_and_import_preserves_existing_values() {
        let (_dir, store) = store();
        store
            .set(SettingKey::APPEARANCE_PALETTE, "forest")
            .expect("set");
        store
            .set(SettingKey::APPEARANCE_DENSITY, "cozy")
            .expect("set");

        let snapshot = store.snapshot().expect("snapshot");
        let keys: Vec<_> = snapshot.keys().cloned().collect();
        assert_eq!(
            keys,
            vec![
                SettingKey::APPEARANCE_DENSITY.as_str().to_string(),
                SettingKey::APPEARANCE_PALETTE.as_str().to_string()
            ]
        );

        let imported = store
            .import_legacy([
                ("plasma_appearance_palette".to_string(), "ion".to_string()),
                ("plasma_info_panel_open".to_string(), "true".to_string()),
                ("skip_login".to_string(), "true".to_string()),
            ])
            .expect("import");

        assert_eq!(imported, 2);
        assert_eq!(
            store
                .get(SettingKey::APPEARANCE_PALETTE)
                .expect("palette")
                .as_deref(),
            Some("forest")
        );
        assert!(store.get_bool(SettingKey::INFO_PANEL_OPEN).expect("info"));
        assert!(store.get_bool(SettingKey::SKIP_LOGIN).expect("skip"));
    }

    #[test]
    fn dynamic_keys_use_the_raw_api() {
        let (_dir, store) = store();
        let key = "plasma_timeline_anchor_123_456";
        store.set_raw(key, "789").expect("set raw");
        assert_eq!(store.get_raw(key).expect("get raw").as_deref(), Some("789"));
        store.remove_raw(key).expect("remove raw");
        assert_eq!(store.get_raw(key).expect("get raw"), None);
    }
}
