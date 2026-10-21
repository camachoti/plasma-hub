//! Tauri command wrappers for download cancellation.

#[tauri::command]
pub fn cancel_native_download(id: String) -> bool {
    plasma_download::cancel_download(&id)
}
