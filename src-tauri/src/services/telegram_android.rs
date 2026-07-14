use serde::Serialize;

#[derive(Default)]
pub struct TdlibManager;

#[derive(Serialize)]
pub struct TdlibStatus {
    pub success: bool,
    pub ready: bool,
    pub state: String,
    pub error: Option<String>,
}

type TdlibCommandResult = Result<TdlibStatus, String>;

fn unavailable_status() -> TdlibStatus {
    TdlibStatus {
        success: false,
        ready: false,
        state: "unavailable".to_string(),
        error: Some("Telegram nativo ainda não está disponível no Android.".to_string()),
    }
}

#[tauri::command]
pub async fn tdlib_init(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, TdlibManager>,
    _api_id: i32,
    _api_hash: String,
) -> TdlibCommandResult {
    Ok(unavailable_status())
}

#[tauri::command]
pub async fn tdlib_status(_state: tauri::State<'_, TdlibManager>) -> TdlibCommandResult {
    Ok(unavailable_status())
}

#[tauri::command]
pub async fn tdlib_set_phone(
    _state: tauri::State<'_, TdlibManager>,
    _phone_number: String,
) -> TdlibCommandResult {
    Ok(unavailable_status())
}

#[tauri::command]
pub async fn tdlib_check_code(
    _state: tauri::State<'_, TdlibManager>,
    _code: String,
) -> TdlibCommandResult {
    Ok(unavailable_status())
}

#[tauri::command]
pub async fn tdlib_check_password(
    _state: tauri::State<'_, TdlibManager>,
    _password: String,
) -> TdlibCommandResult {
    Ok(unavailable_status())
}

#[tauri::command]
pub async fn tdlib_get_me(_state: tauri::State<'_, TdlibManager>) -> Result<serde_json::Value, String> {
    Err("Telegram nativo ainda não está disponível no Android.".to_string())
}

#[tauri::command]
pub async fn tdlib_download_message_media(
    _state: tauri::State<'_, TdlibManager>,
    _chat_id: i64,
    _message_id: i64,
    _folder_path: String,
) -> Result<serde_json::Value, String> {
    Err("Downloads do Telegram ainda não estão disponíveis no Android.".to_string())
}

#[tauri::command]
pub async fn tdlib_stop_download(_state: tauri::State<'_, TdlibManager>) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn tdlib_start_mass_download(
    _app: tauri::AppHandle,
    _state: tauri::State<'_, TdlibManager>,
    _request: serde_json::Value,
) -> Result<serde_json::Value, String> {
    Err("Downloads em massa do Telegram ainda não estão disponíveis no Android.".to_string())
}
