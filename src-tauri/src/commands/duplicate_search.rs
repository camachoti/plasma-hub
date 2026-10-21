//! Tauri command wrappers for the duplicate media search.

use crate::env::env;
use plasma_duplicates::{SearchMode, SearchResult, SearchSource};
use tauri::AppHandle;

#[tauri::command]
pub async fn search_duplicate_media(
    app: AppHandle,
    run_id: String,
    folder_path: String,
    sources: Vec<SearchSource>,
    mode: Option<SearchMode>,
) -> Result<SearchResult, String> {
    plasma_duplicates::search_duplicate_media(&env(&app), run_id, folder_path, sources, mode).await
}
