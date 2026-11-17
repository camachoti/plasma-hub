use std::path::Path;
use std::sync::atomic::Ordering;

use plasma_core::AppEnv;

use super::*;
use crate::media_http;

async fn telegram_media_ensure_cached_inner(
    app: &AppEnv,
    manager: &TdlibManager,
    client_id: i32,
    chat_id: i64,
    message_id: i64,
    priority: Option<String>,
) -> NativeMediaAssetMeta {
    let _ = cleanup_stale_native_partials(app);
    if let Ok(Some(meta)) = complete_native_media_meta(app, chat_id, message_id) {
        return meta;
    }

    let lock = native_media_lock(manager, chat_id, message_id).await;
    let _guard = lock.lock().await;
    let abort_flag = native_media_abort_flag(manager, chat_id, message_id).await;
    abort_flag.store(false, Ordering::SeqCst);

    if let Ok(Some(meta)) = complete_native_media_meta(app, chat_id, message_id) {
        return meta;
    }

    let media = match resolve_message_media(client_id, chat_id, message_id).await {
        Ok(media) => media,
        Err(error) => {
            let mut meta = empty_native_media_meta(chat_id, message_id);
            meta.state = "failed".to_string();
            meta.error = Some(error.to_string());
            let _ = write_native_media_meta(app, &meta);
            app.emit("telegram-media-error", &meta);
            return meta;
        }
    };

    let file_name = media.file_name.clone();
    let mime_type = media.mime_type.clone();
    let total_bytes = Some(effective_file_size(&media.file));
    let media_dir = match native_media_dir(app, chat_id, message_id) {
        Ok(dir) => dir,
        Err(error) => {
            let mut meta = empty_native_media_meta(chat_id, message_id);
            meta.state = "failed".to_string();
            meta.error = Some(error.to_string());
            app.emit("telegram-media-error", &meta);
            return meta;
        }
    };

    if let Err(error) = std::fs::create_dir_all(&media_dir) {
        let mut meta = empty_native_media_meta(chat_id, message_id);
        meta.state = "failed".to_string();
        meta.error = Some(error.to_string());
        app.emit("telegram-media-error", &meta);
        return meta;
    }

    let destination = media_dir.join(sanitize_filename(&file_name));
    cleanup_incomplete_file(&destination, total_bytes);
    let thumbnail_path =
        download_thumbnail_to_cache(client_id, media.thumbnail.clone(), &media_dir).await;
    let mut meta = NativeMediaAssetMeta {
        success: false,
        state: "partial".to_string(),
        chat_id,
        message_id,
        total_bytes,
        downloaded_bytes: None,
        mime_type: Some(mime_type),
        file_name: Some(file_name.clone()),
        native_file_path: Some(destination.to_string_lossy().to_string()),
        thumbnail_path,
        completed_at: None,
        updated_at: now_millis(),
        error: None,
    };
    let _ = write_native_media_meta(app, &meta);
    emit_native_media_progress(app, chat_id, message_id, "downloading", 0, total_bytes);

    let priority_label = priority.unwrap_or_else(|| "user".to_string());
    let download_result = download_media_file_to_path(
        client_id,
        media.file,
        &destination,
        |percent, stage| {
            let stage = if stage == "saving" {
                "saving"
            } else if priority_label == "background" {
                "background"
            } else {
                "downloading"
            };
            emit_native_media_progress(app, chat_id, message_id, stage, percent, total_bytes);
        },
        || abort_flag.load(Ordering::SeqCst) || manager.download_aborted.load(Ordering::SeqCst),
    )
    .await;

    match download_result {
        Ok((size, _skipped)) => {
            meta.success = true;
            meta.state = "complete".to_string();
            meta.total_bytes = Some(size);
            meta.downloaded_bytes = Some(size);
            meta.completed_at = Some(now_millis());
            meta.updated_at = now_millis();
            meta.error = None;
            let _ = write_native_media_meta(app, &meta);
            emit_native_media_progress(app, chat_id, message_id, "ready", 100, Some(size));
            app.emit("telegram-media-ready", &meta);
            meta
        }
        Err(error) => {
            meta.success = false;
            if error.is_cancelled() {
                meta.state = "partial".to_string();
                meta.error = None;
            } else {
                meta.state = "failed".to_string();
                meta.error = Some(error.to_string());
            }
            meta.updated_at = now_millis();
            let _ = write_native_media_meta(app, &meta);
            if meta.state == "failed" {
                app.emit("telegram-media-error", &meta);
            }
            meta
        }
    }
}

pub async fn telegram_media_get_meta(
    app: &AppEnv,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<NativeMediaAssetMeta> {
    read_native_media_meta(app, chat_id, message_id)
}

pub async fn telegram_media_ensure_cached(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    priority: Option<String>,
) -> plasma_core::Result<NativeMediaAssetMeta> {
    state.download_aborted.store(false, Ordering::SeqCst);
    let client_id = match ready_client_id(state).await {
        Ok(client_id) => client_id,
        Err(status) => {
            let mut meta = empty_native_media_meta(chat_id, message_id);
            meta.state = "failed".to_string();
            meta.error = status.error.or(Some(status.state));
            return Ok(meta);
        }
    };

    Ok(
        telegram_media_ensure_cached_inner(app, state, client_id, chat_id, message_id, priority)
            .await,
    )
}

pub async fn telegram_media_prepare_playback(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    mode: Option<String>,
) -> plasma_core::Result<NativePlaybackSource> {
    if let Ok(Some(meta)) = complete_native_media_meta(app, chat_id, message_id) {
        let (playback_path, playback_mime_type) = ensure_native_playback_file(app, &meta)?;
        return Ok(NativePlaybackSource {
            success: true,
            kind: "file".to_string(),
            url: Some(media_http::playback_url(chat_id, message_id)),
            file_path: Some(playback_path),
            mime_type: playback_mime_type.or(meta.mime_type.clone()),
            cache_state: meta.state,
            total_bytes: meta.total_bytes,
            downloaded_bytes: meta.downloaded_bytes,
            file_name: meta.file_name,
            error: None,
        });
    }

    let priority = mode.map(|value| {
        if value == "background" {
            value
        } else {
            "user".to_string()
        }
    });
    let meta = telegram_media_ensure_cached(app, state, chat_id, message_id, priority).await?;
    let playback = if meta.success {
        ensure_native_playback_file(app, &meta).ok()
    } else {
        None
    };
    let playback_path = playback
        .as_ref()
        .map(|(path, _)| path.clone())
        .or_else(|| meta.native_file_path.clone());
    let playback_mime_type = playback
        .and_then(|(_, mime_type)| mime_type)
        .or(meta.mime_type.clone());
    Ok(NativePlaybackSource {
        success: meta.success,
        kind: if meta.success { "file" } else { "fallback" }.to_string(),
        url: if meta.success {
            Some(media_http::playback_url(chat_id, message_id))
        } else {
            playback_path.clone()
        },
        file_path: playback_path,
        mime_type: playback_mime_type,
        cache_state: meta.state,
        total_bytes: meta.total_bytes,
        downloaded_bytes: meta.downloaded_bytes,
        file_name: meta.file_name,
        error: meta.error,
    })
}

pub async fn telegram_media_save(
    app: &AppEnv,
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
    destination_dir: Option<String>,
) -> plasma_core::Result<NativeSavedMedia> {
    let save_abort_flag = native_media_abort_flag(state, chat_id, message_id).await;
    let meta =
        telegram_media_ensure_cached(app, state, chat_id, message_id, Some("user".to_string()))
            .await?;

    if !meta.success {
        let canceled = meta.state == "partial" && meta.error.is_none();
        return Ok(NativeSavedMedia {
            success: false,
            file_path: None,
            file_name: meta.file_name,
            size: meta.total_bytes,
            skipped: false,
            error: if canceled {
                Some("STOP_ABORTED".to_string())
            } else {
                meta.error
            },
        });
    }

    let Some(source_path) = meta.native_file_path.clone() else {
        return Ok(NativeSavedMedia {
            success: false,
            file_path: None,
            file_name: meta.file_name,
            size: meta.total_bytes,
            skipped: false,
            error: Some("Midia nativa sem caminho de arquivo.".to_string()),
        });
    };

    let file_name = meta
        .file_name
        .clone()
        .unwrap_or_else(|| sanitize_filename(&format!("media_{message_id}")));
    let destination = match destination_dir {
        Some(dir) if !dir.trim().is_empty() => unique_file_destination(Path::new(&dir), &file_name),
        _ => app.paths().download_dir()?.join(&file_name),
    };

    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(plasma_core::Error::from)?;
    }

    let skipped = destination.exists();
    if !skipped {
        save_abort_flag.store(false, Ordering::SeqCst);
        let tmp_path = partial_file_path(&destination);
        let _ = std::fs::remove_file(&tmp_path);
        let total_bytes = meta.total_bytes.or_else(|| {
            std::fs::metadata(&source_path)
                .ok()
                .map(|metadata| metadata.len() as i64)
        });
        let mut on_progress = |percent, stage| {
            emit_native_media_progress(app, chat_id, message_id, stage, percent, total_bytes);
        };
        copy_file_with_progress(
            Path::new(&source_path),
            &tmp_path,
            total_bytes.unwrap_or_default(),
            &mut on_progress,
            &|| save_abort_flag.load(Ordering::SeqCst),
        )
        .await
        .inspect_err(|_error| {
            let _ = std::fs::remove_file(&tmp_path);
        })?;
        std::fs::rename(&tmp_path, &destination).map_err(|error| {
            let _ = std::fs::remove_file(&tmp_path);
            error.to_string()
        })?;
    }

    Ok(NativeSavedMedia {
        success: true,
        file_path: Some(destination.to_string_lossy().to_string()),
        file_name: Some(file_name),
        size: meta.total_bytes,
        skipped,
        error: None,
    })
}

pub async fn telegram_media_cancel(
    state: &TdlibManager,
    chat_id: i64,
    message_id: i64,
) -> plasma_core::Result<bool> {
    let abort_flag = native_media_abort_flag(state, chat_id, message_id).await;
    abort_flag.store(true, Ordering::SeqCst);
    Ok(true)
}

pub async fn telegram_media_cache_stats(
    app: &AppEnv,
) -> plasma_core::Result<NativeMediaCacheStats> {
    Ok(native_media_cache_stats(app))
}

pub async fn telegram_media_clear_cache(
    app: &AppEnv,
) -> plasma_core::Result<NativeMediaCacheStats> {
    let root = native_media_root_dir(app)?;
    if root.exists() {
        std::fs::remove_dir_all(&root).map_err(plasma_core::Error::from)?;
    }
    Ok(native_media_cache_stats(app))
}

pub async fn telegram_media_evict_cache(
    app: &AppEnv,
    max_cache_size: i64,
) -> plasma_core::Result<NativeMediaCacheStats> {
    evict_native_media_cache(app, max_cache_size)
}
