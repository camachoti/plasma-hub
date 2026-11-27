use std::collections::HashMap;
use std::path::Path;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use plasma_core::AppEnv;
use tdlib_rs::{
    enums::{AuthorizationState, ConnectionState, Update},
    functions,
    types::File,
};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    sync::Mutex,
};

use crate::media::*;
use crate::messages::*;
use crate::types::*;

pub(super) const TDLIB_LOG_MAX_BYTES: i64 = 10 * 1024 * 1024;

#[cfg(debug_assertions)]
pub(super) static STARTUP_TIMING_LOGGED: AtomicBool = AtomicBool::new(false);
pub(super) static TDLIB_LOG_NOTICE: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
pub(super) struct TdlibInner {
    client_id: Option<i32>,
    api_id: Option<i32>,
    api_hash: Option<String>,
    database_directory: Option<String>,
    files_directory: Option<String>,
    auth_state: String,
}

pub struct TdlibManager {
    inner: Arc<Mutex<TdlibInner>>,
    init_lock: Arc<Mutex<()>>,
    media_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    media_abort_flags: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    receiver_started: AtomicBool,
    download_aborted: AtomicBool,
    /// Priority scheduler for the single TDLib client.
    pub(crate) queue: Arc<request_queue::RequestQueue>,
}

impl Default for TdlibManager {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(TdlibInner::default())),
            init_lock: Arc::new(Mutex::new(())),
            media_locks: Arc::new(Mutex::new(HashMap::new())),
            media_abort_flags: Arc::new(Mutex::new(HashMap::new())),
            receiver_started: AtomicBool::new(false),
            download_aborted: AtomicBool::new(false),
            queue: Arc::new(request_queue::RequestQueue::new()),
        }
    }
}

pub use request_queue::{with_priority, ConversationPriorityGuard, TdlibPriority};

impl ConversationPriorityGuard {
    pub fn enter(manager: &TdlibManager) -> Self {
        Self::from_queue(manager.queue.clone())
    }
}

pub async fn wait_conversation_priority_clear(manager: &TdlibManager) {
    manager.queue.wait_leases_clear().await;
}

pub fn conversation_priority_active(manager: &TdlibManager) -> bool {
    manager.queue.conversation_busy()
}

pub(crate) async fn native_media_lock(
    manager: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> Arc<Mutex<()>> {
    let key = format!("{chat_id}:{message_id}");
    let mut locks = manager.media_locks.lock().await;
    locks
        .entry(key)
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

pub(crate) async fn native_media_abort_flag(
    manager: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> Arc<AtomicBool> {
    let key = format!("{chat_id}:{message_id}");
    let mut flags = manager.media_abort_flags.lock().await;
    flags
        .entry(key)
        .or_insert_with(|| Arc::new(AtomicBool::new(false)))
        .clone()
}

pub(super) fn state_label(state: &AuthorizationState) -> &'static str {
    match state {
        AuthorizationState::WaitTdlibParameters => "wait_tdlib_parameters",
        AuthorizationState::WaitPhoneNumber => "wait_phone_number",
        AuthorizationState::WaitCode(_) => "wait_code",
        AuthorizationState::WaitPassword(_) => "wait_password",
        AuthorizationState::WaitEmailAddress(_) => "wait_email_address",
        AuthorizationState::WaitEmailCode(_) => "wait_email_code",
        AuthorizationState::WaitOtherDeviceConfirmation(_) => "wait_other_device_confirmation",
        AuthorizationState::WaitRegistration(_) => "wait_registration",
        AuthorizationState::Ready => "ready",
        AuthorizationState::LoggingOut => "logging_out",
        AuthorizationState::Closing => "closing",
        AuthorizationState::Closed => "closed",
        _ => "unknown",
    }
}

pub(super) async fn set_state(
    inner: &Arc<Mutex<TdlibInner>>,
    app: &AppEnv,
    state: AuthorizationState,
) {
    let label = state_label(&state).to_string();
    {
        let mut guard = inner.lock().await;
        guard.auth_state = label.clone();
    }
    app.emit("tdlib-auth-state", &label);
}

pub(super) fn connection_state_label(state: &ConnectionState) -> &'static str {
    match state {
        ConnectionState::WaitingForNetwork => "waiting_for_network",
        ConnectionState::ConnectingToProxy => "connecting_to_proxy",
        ConnectionState::Connecting => "connecting",
        ConnectionState::Updating => "updating",
        ConnectionState::Ready => "ready",
    }
}

pub(super) async fn client_id_or_status(state: &TdlibManager) -> Result<i32, TdlibStatus> {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    client_id.ok_or_else(|| TdlibStatus {
        success: false,
        ready: false,
        state: "not_initialized".to_string(),
        error: Some("TDLib nao inicializado.".to_string()),
    })
}

pub(super) async fn ready_client_id(state: &TdlibManager) -> Result<i32, TdlibStatus> {
    let client_id = client_id_or_status(state).await?;
    match functions::get_authorization_state(client_id).await {
        Ok(AuthorizationState::Ready) => Ok(client_id),
        Ok(auth_state) => Err(TdlibStatus {
            success: false,
            ready: false,
            state: state_label(&auth_state).to_string(),
            error: Some("TDLib ainda nao esta autenticado.".to_string()),
        }),
        Err(error) => Err(td_error(error)),
    }
}

pub(super) fn start_receiver(app: AppEnv, manager: &TdlibManager) {
    if manager.receiver_started.swap(true, Ordering::AcqRel) {
        return;
    }

    let inner = manager.inner.clone();
    tokio::spawn(async move {
        loop {
            let result = tokio::task::spawn_blocking(tdlib_rs::receive).await;
            match result {
                Ok(Some((Update::AuthorizationState(update), _client_id))) => {
                    set_state(&inner, &app, update.authorization_state).await;
                }
                Ok(Some((Update::ConnectionState(update), _client_id))) => {
                    app.emit(
                        "tdlib-connection-state",
                        connection_state_label(&update.state),
                    );
                }
                Ok(Some((Update::NewMessage(update), client_id))) => {
                    if is_hidden_membership_service_message(&update.message.content) {
                        continue;
                    }

                    let chat_id = update.message.chat_id;
                    let message = tdlib_message_to_app(update.message);
                    let sender_id = message.sender_id.clone();
                    if message.out {
                        emit_new_message(&app, chat_id, message);
                    } else {
                        let app = app.clone();
                        tokio::spawn(async move {
                            // The lookup lives in its own task. Timing out the
                            // wait must not drop the TDLib request: a dropped
                            // oneshot makes the receiver log
                            // "unaccessible request" for every late response.
                            let lookup = tokio::spawn(async move {
                                resolve_sender_name(client_id, &sender_id).await
                            });
                            let sender_name = match tokio::time::timeout(
                                Duration::from_millis(800),
                                lookup,
                            )
                            .await
                            {
                                Ok(Ok(name)) => name,
                                _ => String::new(),
                            };
                            let message = if sender_name.trim().is_empty() {
                                message
                            } else {
                                TdlibMessageInfo {
                                    sender_name,
                                    ..message
                                }
                            };
                            emit_new_message(&app, chat_id, message);
                        });
                    }
                }
                Ok(Some((Update::DeleteMessages(update), _client_id))) => {
                    if update.is_permanent && !update.from_cache {
                        emit_deleted_messages(
                            &app,
                            update.chat_id,
                            update
                                .message_ids
                                .into_iter()
                                .map(app_message_id_from_tdlib)
                                .collect(),
                        );
                    }
                }
                Ok(Some((Update::MessageContent(update), _client_id))) => {
                    let text = content_text(&update.new_content);
                    let (has_media, is_photo, is_video, _, _) =
                        content_media_flags(&update.new_content);
                    app.emit(
                        "tdlib-message-updated",
                        serde_json::json!({
                            "chatId": update.chat_id,
                            "messageId": app_message_id_from_tdlib(update.message_id),
                            "text": text,
                            "hasMedia": has_media,
                            "isPhoto": is_photo,
                            "isVideo": is_video,
                        }),
                    );
                }
                Ok(Some((Update::ChatReadInbox(update), _client_id))) => {
                    app.emit(
                        "tdlib-chat-patch",
                        serde_json::json!({
                            "chatId": update.chat_id,
                            "unreadCount": update.unread_count,
                        }),
                    );
                }
                Ok(Some((Update::ChatTitle(update), _client_id))) => {
                    app.emit(
                        "tdlib-chat-patch",
                        serde_json::json!({
                            "chatId": update.chat_id,
                            "title": update.title,
                        }),
                    );
                }
                Ok(Some((_update, _client_id))) => {}
                Ok(None) => {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                Err(error) => {
                    app.emit("tdlib-error", format!("TDLib receiver failed: {error}"));
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
            }
        }
    });
}

pub(super) fn td_error(error: tdlib_rs::types::Error) -> TdlibStatus {
    TdlibStatus {
        success: false,
        ready: false,
        state: "error".to_string(),
        error: Some(error.message),
    }
}

pub(super) fn td_timeout(message: impl Into<String>) -> TdlibStatus {
    TdlibStatus {
        success: false,
        ready: false,
        state: "timeout".to_string(),
        error: Some(message.into()),
    }
}

pub(super) async fn download_media_file_to_path(
    client_id: i32,
    media_file: File,
    destination: &Path,
    mut on_progress: impl FnMut(u8, &'static str),
    should_abort: impl Fn() -> bool,
) -> plasma_core::Result<(i64, bool)> {
    let expected_size = effective_file_size(&media_file);
    if file_is_complete(destination, Some(expected_size)) {
        return Ok((expected_size, true));
    }
    cleanup_incomplete_file(destination, Some(expected_size));

    let file_id = media_file.id;
    let mut downloaded_file =
        match functions::download_file(file_id, 32, 0, 0, false, client_id).await {
            Ok(tdlib_rs::enums::File::File(file)) => file,
            Err(error) => return Err(error.message.into()),
        };

    let total_size = if media_file.size > 0 {
        media_file.size
    } else if media_file.expected_size > 0 {
        media_file.expected_size
    } else if downloaded_file.size > 0 {
        downloaded_file.size
    } else {
        downloaded_file.expected_size
    };
    let mut last_percent = 0u8;
    on_progress(0, "downloading");

    while !downloaded_file.local.is_downloading_completed {
        if should_abort() {
            let _ = functions::cancel_download_file(file_id, false, client_id).await;
            return Err(plasma_core::Error::Cancelled);
        }

        let downloaded_size = downloaded_file.local.downloaded_size.max(0);
        if total_size > 0 {
            let percent = ((downloaded_size.saturating_mul(95)) / total_size).clamp(0, 95) as u8;
            if percent != last_percent {
                last_percent = percent;
                on_progress(percent, "downloading");
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        downloaded_file = match functions::get_file(file_id, client_id).await {
            Ok(tdlib_rs::enums::File::File(file)) => file,
            Err(error) => return Err(error.message.into()),
        };
    }

    if downloaded_file.local.path.is_empty() || !downloaded_file.local.is_downloading_completed {
        return Err("TDLib nao concluiu o download do arquivo.".into());
    }

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(plasma_core::Error::from)?;
    }

    let partial_destination = partial_file_path(destination);
    let _ = std::fs::remove_file(&partial_destination);

    let copy_result = copy_file_with_progress(
        Path::new(&downloaded_file.local.path),
        &partial_destination,
        effective_file_size(&downloaded_file),
        &mut on_progress,
        &should_abort,
    )
    .await;
    if let Err(error) = copy_result {
        let _ = std::fs::remove_file(&partial_destination);
        return Err(error);
    }
    std::fs::rename(&partial_destination, destination).map_err(|error| {
        let _ = std::fs::remove_file(&partial_destination);
        error.to_string()
    })?;

    Ok((effective_file_size(&downloaded_file), false))
}

pub(super) async fn copy_file_with_progress(
    source: &Path,
    destination: &Path,
    total_size: i64,
    on_progress: &mut impl FnMut(u8, &'static str),
    should_abort: &impl Fn() -> bool,
) -> plasma_core::Result<()> {
    on_progress(96, "saving");
    let mut input = tokio::fs::File::open(source)
        .await
        .map_err(plasma_core::Error::from)?;
    let mut output = tokio::fs::File::create(destination)
        .await
        .map_err(plasma_core::Error::from)?;
    let mut buffer = vec![0_u8; 1024 * 1024];
    let mut copied = 0_i64;
    let mut last_percent = 96_u8;

    loop {
        if should_abort() {
            return Err(plasma_core::Error::Cancelled);
        }
        let read = input
            .read(&mut buffer)
            .await
            .map_err(plasma_core::Error::from)?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .await
            .map_err(plasma_core::Error::from)?;
        copied = copied.saturating_add(read as i64);

        if total_size > 0 {
            let percent = 96 + ((copied.saturating_mul(4)) / total_size).clamp(0, 4) as u8;
            if percent != last_percent {
                last_percent = percent;
                on_progress(percent, "saving");
            }
        }
    }

    output.flush().await.map_err(plasma_core::Error::from)?;
    on_progress(100, "saving");
    Ok(())
}

pub(super) async fn download_thumbnail_path(
    client_id: i32,
    thumbnail: Option<File>,
) -> Option<String> {
    let thumbnail = thumbnail?;
    if thumbnail.local.is_downloading_completed && !thumbnail.local.path.is_empty() {
        return Some(thumbnail.local.path);
    }

    let file_id = thumbnail.id;
    let mut file = match functions::download_file(file_id, 16, 0, 0, true, client_id).await {
        Ok(tdlib_rs::enums::File::File(file)) => file,
        _ => return None,
    };

    for _ in 0..50 {
        if file.local.is_downloading_completed && !file.local.path.is_empty() {
            return Some(file.local.path);
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        file = match functions::get_file(file_id, client_id).await {
            Ok(tdlib_rs::enums::File::File(file)) => file,
            _ => return None,
        };
    }

    None
}

pub(super) async fn download_thumbnail_to_cache(
    client_id: i32,
    thumbnail: Option<File>,
    media_dir: &Path,
) -> Option<String> {
    let source_path = download_thumbnail_path(client_id, thumbnail).await?;
    if !Path::new(&source_path).exists() {
        return None;
    }

    let extension = Path::new(&source_path)
        .extension()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "jpg".to_string());
    let destination = media_dir.join(format!("thumbnail.{extension}"));

    if destination.exists() {
        return Some(destination.to_string_lossy().to_string());
    }

    if std::fs::create_dir_all(media_dir).is_err() {
        return None;
    }

    match std::fs::copy(&source_path, &destination) {
        Ok(_) => Some(destination.to_string_lossy().to_string()),
        Err(_) => Some(source_path),
    }
}

pub(super) fn generate_video_thumbnail_to_cache(
    source_path: &str,
    destination: &Path,
) -> Option<String> {
    if destination.is_file() {
        return Some(destination.to_string_lossy().to_string());
    }
    if !Path::new(source_path).is_file() {
        return None;
    }
    std::fs::create_dir_all(destination.parent()?).ok()?;

    plasma_media::generate_video_thumbnail_to_path(Path::new(source_path), destination).ok()?;
    destination
        .is_file()
        .then(|| destination.to_string_lossy().to_string())
}

pub(super) async fn download_avatar_to_cache(
    app: &AppEnv,
    client_id: i32,
    chat_id: i64,
    photo: Option<File>,
) -> Option<String> {
    let source_path = download_thumbnail_path(client_id, photo).await?;
    if !Path::new(&source_path).exists() {
        return None;
    }

    let avatar_dir = app
        .paths()
        .cache_dir()
        .join("plasma-media")
        .join("telegram-avatars");
    if std::fs::create_dir_all(&avatar_dir).is_err() {
        return None;
    }

    let extension = Path::new(&source_path)
        .extension()
        .map(|value| value.to_string_lossy().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "jpg".to_string());
    let destination = avatar_dir.join(format!("{chat_id}.{extension}"));
    if destination.exists() {
        return Some(destination.to_string_lossy().to_string());
    }

    match std::fs::copy(&source_path, &destination) {
        Ok(_) => Some(destination.to_string_lossy().to_string()),
        Err(_) => Some(source_path),
    }
}

pub(super) async fn resolve_message_media(
    client_id: i32,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<MessageMediaFile> {
    let message = match functions::get_message(chat_id, message_id, client_id).await {
        Ok(tdlib_rs::enums::Message::Message(message)) => message,
        Err(_) => match functions::get_message(
            chat_id,
            tdlib_message_id_from_app_id(message_id),
            client_id,
        )
        .await
        {
            Ok(tdlib_rs::enums::Message::Message(message)) => message,
            Err(error) => return Err(error.message.into()),
        },
    };

    message_media_file(message.content, message.id)
        .ok_or_else(|| plasma_core::Error::msg("Mensagem sem midia baixavel pelo TDLib."))
}

pub(super) fn emit_native_media_progress(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
    stage: &str,
    progress: u8,
    total_bytes: Option<i64>,
) {
    let downloaded_bytes = total_bytes
        .map(|total| ((total.max(0) as f64) * (f64::from(progress) / 100.0)).round() as i64);
    app.emit(
        "telegram-media-progress",
        NativeMediaProgress {
            chat_id,
            message_id,
            stage: stage.to_string(),
            progress,
            downloaded_bytes,
            total_bytes,
        },
    );
}

pub(super) fn missing_tdlib_config(field: &str) -> tdlib_rs::types::Error {
    tdlib_rs::types::Error {
        code: 400,
        message: format!("Configuração do TDLib incompleta: {field}."),
    }
}

pub(super) async fn configure_if_needed(
    inner: &Arc<Mutex<TdlibInner>>,
) -> Result<(), tdlib_rs::types::Error> {
    let (client_id, api_id, api_hash, database_directory, files_directory) = {
        let guard = inner.lock().await;
        let Some(client_id) = guard.client_id else {
            return Err(missing_tdlib_config("client_id"));
        };
        let Some(api_id) = guard.api_id else {
            return Err(missing_tdlib_config("api_id"));
        };
        let Some(api_hash) = guard.api_hash.clone() else {
            return Err(missing_tdlib_config("api_hash"));
        };
        let Some(database_directory) = guard.database_directory.clone() else {
            return Err(missing_tdlib_config("database_directory"));
        };
        let Some(files_directory) = guard.files_directory.clone() else {
            return Err(missing_tdlib_config("files_directory"));
        };
        (
            client_id,
            api_id,
            api_hash,
            database_directory,
            files_directory,
        )
    };

    let state = functions::get_authorization_state(client_id).await?;
    if matches!(state, AuthorizationState::WaitTdlibParameters) {
        functions::set_tdlib_parameters(
            false,
            database_directory,
            files_directory,
            String::new(),
            true,
            true,
            true,
            false,
            api_id,
            api_hash,
            "pt-BR".to_string(),
            "Linux".to_string(),
            String::new(),
            env!("CARGO_PKG_VERSION").to_string(),
            client_id,
        )
        .await?;
    }

    Ok(())
}

mod auth;
mod chat_media;
mod chats;
mod files;
mod forum;
mod links;
mod outbound;
mod request_queue;
mod transfer;

pub use auth::*;
pub use chat_media::*;
pub use chats::*;
pub use files::*;
pub use forum::*;
pub use links::*;
pub use outbound::*;
pub use transfer::*;
