use std::sync::atomic::Ordering;
use std::time::Duration;

use plasma_core::AppEnv;
use tdlib_rs::{enums::LogStream, functions, types::LogStreamFile};

use super::*;

pub async fn tdlib_init(
    app: &AppEnv,
    state: &TdlibManager,
    api_id: i32,
    api_hash: String,
) -> TdlibCommandResult {
    // Requests are completed by `tdlib_rs::receive`; start the receiver before
    // sending the logging requests below, otherwise they time out.
    start_receiver(app.clone(), state);
    let _init_guard = state.init_lock.lock().await;

    let app_data_dir = app.paths().data_dir().to_path_buf();

    let database_directory = app_data_dir.join("tdlib").join("database");
    let files_directory = app_data_dir.join("tdlib").join("files");

    if let Err(error) = std::fs::create_dir_all(&database_directory) {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "error".to_string(),
            error: Some(error.to_string()),
        });
    }
    if let Err(error) = std::fs::create_dir_all(&files_directory) {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "error".to_string(),
            error: Some(error.to_string()),
        });
    }

    let client_id = {
        let mut guard = state.inner.lock().await;
        let client_id = guard.client_id.unwrap_or_else(tdlib_rs::create_client);
        guard.client_id = Some(client_id);
        guard.api_id = Some(api_id);
        guard.api_hash = Some(api_hash);
        guard.database_directory = Some(database_directory.to_string_lossy().to_string());
        guard.files_directory = Some(files_directory.to_string_lossy().to_string());
        client_id
    };

    // Keep TDLib's internal chatter out of the terminal. The file is rotated
    // by TDLib and is the first place to look when Telegram misbehaves.
    let tdlib_log_path = app_data_dir.join("tdlib").join("tdlib.log");
    let log_stream = LogStream::File(LogStreamFile {
        path: tdlib_log_path.to_string_lossy().to_string(),
        max_file_size: TDLIB_LOG_MAX_BYTES,
        redirect_stderr: false,
    });
    match tokio::time::timeout(
        Duration::from_secs(4),
        functions::set_log_stream(log_stream, client_id),
    )
    .await
    {
        Ok(Ok(())) => {
            if !TDLIB_LOG_NOTICE.swap(true, Ordering::Relaxed) {
                tracing::info!(
                    path = %tdlib_log_path.display(),
                    "TDLib internal logs redirected to file"
                );
            }
        }
        Ok(Err(error)) => {
            tracing::warn!(
                error = %error.message,
                "could not redirect TDLib logs to file; keeping stderr"
            );
        }
        Err(_) => {
            tracing::warn!("TDLib timed out while configuring its log file");
        }
    }

    // Level 1 keeps errors and warnings in the file without flooding it with
    // session/socket details. Failures here must not prevent Telegram startup.
    match tokio::time::timeout(
        Duration::from_secs(4),
        functions::set_log_verbosity_level(1, client_id),
    )
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            tracing::warn!(
                error = %error.message,
                "could not set TDLib log verbosity"
            );
        }
        Err(_) => tracing::warn!("TDLib timed out while setting log verbosity"),
    }

    match tokio::time::timeout(Duration::from_secs(8), configure_if_needed(&state.inner)).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Ok(td_error(error)),
        Err(_) => return Ok(td_timeout("TDLib demorou para configurar parametros.")),
    }

    drop(_init_guard);
    tdlib_status(state).await
}

pub async fn tdlib_status(state: &TdlibManager) -> TdlibCommandResult {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    let Some(client_id) = client_id else {
        return Ok(TdlibStatus {
            success: true,
            ready: false,
            state: "not_initialized".to_string(),
            error: None,
        });
    };

    match tokio::time::timeout(
        Duration::from_secs(4),
        functions::get_authorization_state(client_id),
    )
    .await
    {
        Err(_) => Ok(td_timeout(
            "TDLib demorou para retornar estado de autenticacao.",
        )),
        Ok(result) => match result {
            Ok(auth_state) => {
                let label = state_label(&auth_state).to_string();
                {
                    let mut guard = state.inner.lock().await;
                    guard.auth_state = label.clone();
                }
                Ok(TdlibStatus {
                    success: true,
                    ready: matches!(auth_state, AuthorizationState::Ready),
                    state: label,
                    error: None,
                })
            }
            Err(error) => Ok(td_error(error)),
        },
    }
}

pub async fn tdlib_set_phone(state: &TdlibManager, phone_number: String) -> TdlibCommandResult {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    let Some(client_id) = client_id else {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "not_initialized".to_string(),
            error: Some("TDLib nao inicializado.".to_string()),
        });
    };

    match functions::set_authentication_phone_number(phone_number, None, client_id).await {
        Ok(_) => tdlib_status(state).await,
        Err(error) => Ok(td_error(error)),
    }
}

pub async fn tdlib_check_code(state: &TdlibManager, code: String) -> TdlibCommandResult {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    let Some(client_id) = client_id else {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "not_initialized".to_string(),
            error: Some("TDLib nao inicializado.".to_string()),
        });
    };

    match functions::check_authentication_code(code, client_id).await {
        Ok(_) => tdlib_status(state).await,
        Err(error) => Ok(td_error(error)),
    }
}

pub async fn tdlib_check_password(state: &TdlibManager, password: String) -> TdlibCommandResult {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };

    let Some(client_id) = client_id else {
        return Ok(TdlibStatus {
            success: false,
            ready: false,
            state: "not_initialized".to_string(),
            error: Some("TDLib nao inicializado.".to_string()),
        });
    };

    match functions::check_authentication_password(password, client_id).await {
        Ok(_) => tdlib_status(state).await,
        Err(error) => Ok(td_error(error)),
    }
}

pub async fn tdlib_log_out(state: &TdlibManager) -> TdlibCommandResult {
    let client_id = {
        let guard = state.inner.lock().await;
        guard.client_id
    };
    let Some(client_id) = client_id else {
        return Ok(TdlibStatus {
            success: true,
            ready: false,
            state: "not_initialized".to_string(),
            error: None,
        });
    };

    match functions::log_out(client_id).await {
        Ok(_) => {
            {
                let mut guard = state.inner.lock().await;
                guard.auth_state = "logging_out".to_string();
            }
            tdlib_status(state).await
        }
        Err(error) => Ok(td_error(error)),
    }
}

pub async fn tdlib_get_me(state: &TdlibManager) -> plasma_core::Result<TdlibUserInfo> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibUserInfo {
                success: false,
                id: None,
                first_name: None,
                last_name: None,
                phone_number: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

    match functions::get_me(client_id).await {
        Ok(tdlib_rs::enums::User::User(user)) => Ok(TdlibUserInfo {
            success: true,
            id: Some(user.id),
            first_name: Some(user.first_name),
            last_name: Some(user.last_name),
            phone_number: Some(user.phone_number),
            error: None,
        }),
        Err(error) => Ok(TdlibUserInfo {
            success: false,
            id: None,
            first_name: None,
            last_name: None,
            phone_number: None,
            error: Some(error.message),
        }),
    }
}
