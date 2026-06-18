use rusqlite::{Connection, Result};
use std::sync::Mutex;
use tauri::State;

pub struct DbState {
    pub conn: Mutex<Connection>,
}

pub fn init_db() -> Result<Connection> {
    // In a real app we'd use app_data_dir()
    let conn = Connection::open("plasma_hub.db")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS downloads (
            id INTEGER PRIMARY KEY,
            url TEXT NOT NULL,
            status TEXT NOT NULL,
            platform TEXT NOT NULL,
            file_path TEXT
        )",
        (), // empty list of parameters.
    )?;

    Ok(conn)
}

#[tauri::command]
pub fn get_downloads(state: State<'_, DbState>) -> Result<Vec<String>, String> {
    // Placeholder command
    let conn = state.conn.lock().unwrap();
    let mut stmt = conn
        .prepare("SELECT url FROM downloads")
        .map_err(|e| e.to_string())?;

    let download_iter = stmt
        .query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    let mut urls = Vec::new();
    for url in download_iter {
        urls.push(url.unwrap());
    }

    Ok(urls)
}
