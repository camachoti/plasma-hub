use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::Ordering;

use plasma_core::AppEnv;
use tdlib_rs::functions;

use super::*;

pub async fn tdlib_download_message_media(
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    folder_path: String,
) -> plasma_core::Result<TdlibDownloadResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibDownloadResult {
                success: false,
                skipped: false,
                file_path: None,
                file_name: None,
                size: 0,
                mime_type: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

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
            Err(error) => {
                return Ok(TdlibDownloadResult {
                    success: false,
                    skipped: false,
                    file_path: None,
                    file_name: None,
                    size: 0,
                    mime_type: None,
                    error: Some(error.message),
                });
            }
        },
    };

    let Some(media) = message_media_file(message.content, message.id) else {
        return Ok(TdlibDownloadResult {
            success: false,
            skipped: false,
            file_path: None,
            file_name: None,
            size: 0,
            mime_type: None,
            error: Some("Mensagem sem midia baixavel pelo TDLib.".to_string()),
        });
    };
    let media_file = media.file;
    let file_name = media.file_name;
    let mime_type = media.mime_type;

    let destination = Path::new(&folder_path).join(&file_name);
    let (size, skipped) =
        match download_media_file_to_path(client_id, media_file, &destination, |_, _| {}, || false)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                return Ok(TdlibDownloadResult {
                    success: false,
                    skipped: false,
                    file_path: None,
                    file_name: Some(file_name),
                    size: 0,
                    mime_type: Some(mime_type),
                    error: Some(error.to_string()),
                });
            }
        };

    Ok(TdlibDownloadResult {
        success: true,
        skipped,
        file_path: Some(destination.to_string_lossy().to_string()),
        file_name: Some(file_name),
        size,
        mime_type: Some(mime_type),
        error: None,
    })
}

pub async fn tdlib_cache_message_media(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<TdlibDownloadResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibDownloadResult {
                success: false,
                skipped: false,
                file_path: None,
                file_name: None,
                size: 0,
                mime_type: None,
                error: status.error.or(Some(status.state)),
            });
        }
    };

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
            Err(error) => {
                return Ok(TdlibDownloadResult {
                    success: false,
                    skipped: false,
                    file_path: None,
                    file_name: None,
                    size: 0,
                    mime_type: None,
                    error: Some(error.message),
                });
            }
        },
    };

    let Some(media) = message_media_file(message.content, message.id) else {
        return Ok(TdlibDownloadResult {
            success: false,
            skipped: false,
            file_path: None,
            file_name: None,
            size: 0,
            mime_type: None,
            error: Some("Mensagem sem midia baixavel pelo TDLib.".to_string()),
        });
    };

    let media_file = media.file;
    let file_name = media.file_name;
    let mime_type = media.mime_type;
    let app_cache_dir = app.paths().cache_dir();
    let destination_dir = app_cache_dir
        .join("telegram-media-cache")
        .join(chat_id.to_string());
    let destination = destination_dir.join(sanitize_filename(&format!("{message_id}_{file_name}")));

    let (size, skipped) =
        match download_media_file_to_path(client_id, media_file, &destination, |_, _| {}, || false)
            .await
        {
            Ok(result) => result,
            Err(error) => {
                return Ok(TdlibDownloadResult {
                    success: false,
                    skipped: false,
                    file_path: None,
                    file_name: Some(file_name),
                    size: 0,
                    mime_type: Some(mime_type),
                    error: Some(error.to_string()),
                });
            }
        };

    Ok(TdlibDownloadResult {
        success: true,
        skipped,
        file_path: Some(destination.to_string_lossy().to_string()),
        file_name: Some(file_name),
        size,
        mime_type: Some(mime_type),
        error: None,
    })
}

pub async fn tdlib_download_message_thumbnail(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<TdlibThumbnailResult> {
    if let Ok(meta) = read_native_media_meta(app, chat_id, message_id) {
        if let Some(path) = meta.thumbnail_path {
            if Path::new(&path).exists() {
                return Ok(TdlibThumbnailResult {
                    success: true,
                    file_path: Some(path),
                    error: None,
                });
            }
        }
    }

    with_priority(
        &state.queue,
        TdlibPriority::Background,
        "download_thumbnail",
        || async move {
            let client_id = match ready_client_id(state).await {
                Ok(client_id) => client_id,
                Err(status) => {
                    return Ok(TdlibThumbnailResult {
                        success: false,
                        file_path: None,
                        error: status.error.or(Some(status.state)),
                    });
                }
            };

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
                    Err(error) => {
                        return Ok(TdlibThumbnailResult {
                            success: false,
                            file_path: None,
                            error: Some(error.message),
                        });
                    }
                },
            };

            let local_video_path = local_video_path(&message.content);
            let minithumbnail = message_minithumbnail(&message.content).map(str::to_string);
            let thumbnail = message_thumbnail_file(message.content);

            let media_dir = match native_media_dir(app, chat_id, message_id) {
                Ok(dir) => dir,
                Err(error) => {
                    return Ok(TdlibThumbnailResult {
                        success: false,
                        file_path: None,
                        error: Some(error.to_string()),
                    });
                }
            };

            let thumbnail_path = match thumbnail {
                Some(thumbnail) => {
                    download_thumbnail_to_cache(client_id, Some(thumbnail), &media_dir).await
                }
                None => {
                    // ffmpeg blocks; keep it off the async worker that serves TDLib.
                    let generated = match local_video_path {
                        Some(path) => {
                            let thumbnail_dir = media_dir.clone();
                            tokio::task::spawn_blocking(move || {
                                generate_video_thumbnail_to_cache(
                                    &path,
                                    &thumbnail_dir.join("thumbnail.jpg"),
                                )
                            })
                            .await
                            .ok()
                            .flatten()
                        }
                        None => None,
                    };
                    generated.or_else(|| {
                        minithumbnail.and_then(|data| write_minithumbnail(&data, &media_dir))
                    })
                }
            };

            match thumbnail_path {
                Some(path) => {
                    let mut meta = read_native_media_meta(app, chat_id, message_id)
                        .unwrap_or_else(|_| empty_native_media_meta(chat_id, message_id));
                    meta.thumbnail_path = Some(path.clone());
                    meta.updated_at = now_millis();
                    let _ = write_native_media_meta(app, &meta);
                    Ok(TdlibThumbnailResult {
                        success: true,
                        file_path: Some(path),
                        error: None,
                    })
                }
                None => {
                    log_thumbnail_failure(chat_id, message_id);
                    Ok(TdlibThumbnailResult {
                        success: false,
                        file_path: None,
                        error: Some(
                            "TDLib nao disponibilizou uma thumbnail para esta mídia.".to_string(),
                        ),
                    })
                }
            }
        },
    )
    .await
}

pub async fn tdlib_download_chat_avatar(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
) -> plasma_core::Result<TdlibAvatarResult> {
    if let Some(path) = cached_chat_avatar(app, chat_id) {
        return Ok(TdlibAvatarResult {
            success: true,
            file_path: Some(path.to_string_lossy().to_string()),
            error: None,
        });
    }

    with_priority(
        &state.queue,
        TdlibPriority::Background,
        "download_avatar",
        || async move {
            let client_id = match ready_client_id(state).await {
                Ok(client_id) => client_id,
                Err(status) => {
                    return Ok(TdlibAvatarResult {
                        success: false,
                        file_path: None,
                        error: status.error.or(Some(status.state)),
                    });
                }
            };

            let chat = match functions::get_chat(chat_id, client_id).await {
                Ok(tdlib_rs::enums::Chat::Chat(chat)) => chat,
                Err(error) => {
                    return Ok(TdlibAvatarResult {
                        success: false,
                        file_path: None,
                        error: Some(error.message),
                    });
                }
            };

            let Some(photo) = chat.photo else {
                return Ok(TdlibAvatarResult {
                    success: false,
                    file_path: None,
                    error: Some("Chat sem avatar disponivel pelo TDLib.".to_string()),
                });
            };

            let small = photo.small;
            let big = photo.big;
            let avatar_path =
                match download_avatar_to_cache(app, client_id, chat_id, Some(small)).await {
                    Some(path) => Some(path),
                    None => download_avatar_to_cache(app, client_id, chat_id, Some(big)).await,
                };

            match avatar_path {
                Some(path) => Ok(TdlibAvatarResult {
                    success: true,
                    file_path: Some(path),
                    error: None,
                }),
                None => {
                    tracing::debug!(chat_id, "Telegram avatar unavailable");
                    Ok(TdlibAvatarResult {
                        success: false,
                        file_path: None,
                        error: Some("TDLib nao concluiu o download do avatar.".to_string()),
                    })
                }
            }
        },
    )
    .await
}

pub async fn tdlib_stop_download(state: &TdlibManager) -> plasma_core::Result<()> {
    state.download_aborted.store(true, Ordering::Release);
    Ok(())
}

pub async fn tdlib_start_mass_download(
    app: &AppEnv,
    state: &TdlibManager,
    request: TdlibMassDownloadRequest,
) -> plasma_core::Result<TdlibMassDownloadResult> {
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            return Ok(TdlibMassDownloadResult {
                success: false,
                downloaded_count: 0,
                skipped_count: 0,
                failed_count: 0,
                total: 0,
                aborted: false,
                error: status.error.or(Some(status.state)),
                tracking_error: None,
            });
        }
    };

    state.download_aborted.store(false, Ordering::Release);

    let mut from_message_id = 0;
    let mut downloaded_count = 0usize;
    let mut skipped_count = 0usize;
    let mut failed_count = 0usize;
    let mut items: Vec<TdlibDownloadItem> = Vec::new();
    let mut pending_downloads: Vec<PendingMassDownload> = Vec::new();
    let mut scanned_messages: Vec<tdlib_rs::types::Message> = Vec::new();

    emit_mass_progress(
        app,
        request.chat_id,
        0,
        0.0,
        "Escaneando midias com TDLib...",
        true,
        &items,
    );

    loop {
        if state.download_aborted.load(Ordering::Acquire) {
            break;
        }

        let history_result = if let Some(topic_id) = request.topic_id {
            if request.topic_kind.as_deref() == Some("thread") {
                functions::get_message_thread_history(
                    request.chat_id,
                    tdlib_message_id_from_app_id(topic_id),
                    from_message_id,
                    0,
                    100,
                    client_id,
                )
                .await
            } else {
                let forum_topic_id = match i32::try_from(topic_id) {
                    Ok(topic_id) => topic_id,
                    Err(_) => {
                        return Ok(TdlibMassDownloadResult {
                            success: false,
                            downloaded_count,
                            skipped_count,
                            failed_count,
                            total: items.len(),
                            aborted: false,
                            error: Some("ID de tópico inválido".to_string()),
                            tracking_error: None,
                        });
                    }
                };
                functions::get_forum_topic_history(
                    request.chat_id,
                    forum_topic_id,
                    from_message_id,
                    0,
                    100,
                    client_id,
                )
                .await
            }
        } else {
            functions::get_chat_history(request.chat_id, from_message_id, 0, 100, false, client_id)
                .await
        };

        let history = match history_result {
            Ok(tdlib_rs::enums::Messages::Messages(messages)) => messages,
            Err(error) => {
                return Ok(TdlibMassDownloadResult {
                    success: false,
                    downloaded_count,
                    skipped_count,
                    failed_count,
                    total: items.len(),
                    aborted: false,
                    error: Some(error.message),
                    tracking_error: None,
                });
            }
        };

        if history.messages.is_empty() {
            break;
        }

        let mut next_from_message_id = from_message_id;

        for message in history.messages.into_iter().flatten() {
            next_from_message_id = message.id;
            scanned_messages.push(message);
        }

        if next_from_message_id == 0 || next_from_message_id == from_message_id {
            break;
        }
        from_message_id = next_from_message_id;
    }

    let last_scanned_message_id = scanned_messages
        .iter()
        .map(|message| app_message_id_from_tdlib(message.id))
        .max()
        .unwrap_or(0);
    scanned_messages.reverse();
    let album_split_mode = request.album_split_mode.as_deref().unwrap_or("separator");
    let mut album_folder_names = HashMap::<i64, String>::new();
    let mut last_separator_name: Option<String> = None;

    for message in &scanned_messages {
        let text = content_text(&message.content);
        let (has_media, _, _, _, _) = content_media_flags(&message.content);
        if has_media && request.split_by_album && message.media_album_id != 0 {
            match album_split_mode {
                "comment" => {
                    if !text.trim().is_empty() {
                        album_folder_names
                            .entry(message.media_album_id)
                            .or_insert_with(|| sanitize_folder_name(text.trim()));
                    }
                }
                _ => {
                    if let Some(separator_name) = &last_separator_name {
                        album_folder_names
                            .entry(message.media_album_id)
                            .or_insert_with(|| separator_name.clone());
                    }
                }
            }
        } else if !has_media && is_likely_album_separator_text(&text) {
            last_separator_name = Some(sanitize_folder_name(text.trim()));
        }
    }

    let mut sender_folder_names = HashMap::<String, String>::new();
    for message in scanned_messages {
        let sender_id = message.sender_id.clone();
        let media_album_id = message.media_album_id;
        let mini = message_minithumbnail(&message.content).map(str::to_owned);
        let Some(media) = message_media_file(message.content, message.id) else {
            continue;
        };
        let thumbnail_path = preview_path(app, request.chat_id, message.id, &media, mini);
        let media_file = media.file;
        let file_name = media.file_name;

        let mut folder = Path::new(&request.folder_path).to_path_buf();
        if request.split_by_user {
            let sender_folder =
                resolve_sender_folder_name(client_id, &sender_id, &mut sender_folder_names).await;
            folder = folder.join(sender_folder);
        }
        if request.split_by_album && media_album_id != 0 {
            let album_folder = album_folder_names
                .get(&media_album_id)
                .cloned()
                .unwrap_or_else(|| sanitize_folder_name(&format!("Album_{media_album_id}")));
            folder = folder.join(album_folder);
        }

        let destination = folder.join(&file_name);
        let item_index = items.len();
        items.push(TdlibDownloadItem {
            name: file_name.clone(),
            message_id: message.id,
            status: "pending".to_string(),
            progress: 0,
            size: effective_file_size(&media_file),
            file_path: Some(destination.to_string_lossy().to_string()),
            thumbnail_path,
        });
        pending_downloads.push(PendingMassDownload {
            file: media_file,
            thumbnail: media.thumbnail,
            destination,
            item_index,
        });
    }

    let total = items.len();
    if !state.download_aborted.load(Ordering::Acquire) {
        emit_mass_progress(
            app,
            request.chat_id,
            total,
            0.0,
            format!("{total} midias encontradas. Iniciando downloads..."),
            false,
            &items,
        );
    }

    for pending in pending_downloads {
        if state.download_aborted.load(Ordering::Acquire) {
            break;
        }

        let item_index = pending.item_index;
        let file_name = items[item_index].name.clone();
        if pending.destination.exists() {
            skipped_count += 1;
            items[item_index].status = "skipped".to_string();
            items[item_index].progress = 100;
            emit_mass_progress(
                app,
                request.chat_id,
                total,
                (downloaded_count + skipped_count + failed_count) as f64,
                format!("Ignorado (ja existe): {file_name}"),
                false,
                &items,
            );
            continue;
        }

        items[item_index].status = "downloading".to_string();
        if let Some(path) = download_thumbnail_path(client_id, pending.thumbnail).await {
            items[item_index].thumbnail_path = Some(path);
        }
        emit_mass_progress(
            app,
            request.chat_id,
            total,
            (downloaded_count + skipped_count + failed_count) as f64,
            file_name.clone(),
            false,
            &items,
        );

        let abort_flag = &state.download_aborted;
        let download_result = download_media_file_to_path(
            client_id,
            pending.file,
            &pending.destination,
            |percent, stage| {
                items[item_index].progress = percent;
                let status = if stage == "saving" {
                    format!("Salvando {file_name} ({percent}%)")
                } else {
                    format!("{file_name} ({percent}%)")
                };
                emit_mass_progress(
                    app,
                    request.chat_id,
                    total,
                    (downloaded_count + skipped_count + failed_count) as f64
                        + (f64::from(percent) / 100.0),
                    status,
                    false,
                    &items,
                );
            },
            || abort_flag.load(Ordering::Acquire),
        )
        .await;

        match download_result {
            Ok((size, true)) => {
                skipped_count += 1;
                items[item_index].status = "skipped".to_string();
                items[item_index].progress = 100;
                if size > 0 {
                    items[item_index].size = size;
                }
            }
            Ok((size, false)) => {
                downloaded_count += 1;
                items[item_index].status = "completed".to_string();
                items[item_index].progress = 100;
                if size > 0 {
                    items[item_index].size = size;
                }
            }
            Err(error) => {
                if error.is_cancelled() {
                    items[item_index].status = "stopped".to_string();
                    break;
                }
                failed_count += 1;
                items[item_index].status = "failed".to_string();
                items[item_index].progress = 0;
                emit_mass_progress(
                    app,
                    request.chat_id,
                    total,
                    (downloaded_count + skipped_count + failed_count) as f64,
                    format!("{file_name}: {error}"),
                    false,
                    &items,
                );
                continue;
            }
        }

        emit_mass_progress(
            app,
            request.chat_id,
            total,
            (downloaded_count + skipped_count + failed_count) as f64,
            file_name,
            false,
            &items,
        );
    }

    let aborted = state.download_aborted.load(Ordering::Acquire);
    let current_file = if aborted {
        format!("Parado: {downloaded_count} baixados, {skipped_count} ignorados")
    } else {
        format!("Concluido: {downloaded_count} baixados, {skipped_count} ignorados, {failed_count} falharam")
    };

    emit_mass_progress(
        app,
        request.chat_id,
        total,
        (downloaded_count + skipped_count + failed_count) as f64,
        current_file,
        false,
        &items,
    );

    let tracking_error = if !aborted && failed_count == 0 {
        request.topic_id.and_then(|topic_id| {
            plasma_store::record_topic_mass_download(
                app,
                request.chat_id,
                topic_id,
                request.topic_kind.as_deref().unwrap_or("forum"),
                last_scanned_message_id,
            )
            .err()
            .map(|error| error.to_string())
        })
    } else {
        None
    };

    Ok(TdlibMassDownloadResult {
        success: true,
        downloaded_count,
        skipped_count,
        failed_count,
        total,
        aborted,
        error: None,
        tracking_error,
    })
}

/// Preview available before the file itself starts downloading.
fn preview_path(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
    media: &MessageMediaFile,
    mini: Option<String>,
) -> Option<String> {
    if let Some(path) = local_thumbnail_path(media.thumbnail.clone()) {
        return Some(path);
    }
    let dir = native_media_dir(app, chat_id, message_id).ok()?;
    let existing = dir.join("thumbnail.jpg");
    if existing.is_file() {
        return Some(existing.to_string_lossy().to_string());
    }
    let data = mini?;
    std::fs::create_dir_all(&dir).ok()?;
    write_minithumbnail(&data, &dir)
}
