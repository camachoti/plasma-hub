use super::parse::*;
use super::*;

pub async fn download_twitter_native(
    app_handle: &AppEnv,
    id: String,
    url: String,
    filename: String,
    cookies: Option<String>,
) -> Result<(), String> {
    let cancellation = DownloadCancellation::register(id.clone());
    let result =
        download_twitter_native_inner(app_handle, &id, &url, &filename, cookies, &cancellation)
            .await;
    if let Err(error) = &result {
        if is_cancellation_error(error) {
            return result;
        }
        app_handle.emit(
            "twitter-download-error",
            TwitterDownloadErrorEvent {
                id: id.clone(),
                error: error.clone(),
            },
        );
    }
    result
}

pub async fn download_twitter_profile_native(
    app_handle: &AppEnv,
    id: String,
    username: String,
    media_urls: Vec<String>,
    cookies: Option<String>,
) -> Result<(), String> {
    let cancellation = DownloadCancellation::register(id.clone());
    let result = download_twitter_profile_native_inner(
        app_handle,
        &id,
        &username,
        media_urls,
        cookies,
        &cancellation,
    )
    .await;
    if let Err(error) = &result {
        if is_cancellation_error(error) {
            return result;
        }
        app_handle.emit(
            "twitter-download-error",
            TwitterDownloadErrorEvent {
                id: id.clone(),
                error: error.clone(),
            },
        );
    }
    result
}

fn twitter_download_dir(app_handle: &AppEnv) -> Result<PathBuf, String> {
    app_handle
        .paths()
        .download_dir()
        .or_else(|_| Ok(app_handle.paths().data_dir().join("downloads")))
}

async fn download_twitter_profile_native_inner(
    app_handle: &AppEnv,
    id: &str,
    username: &str,
    media_urls: Vec<String>,
    cookies: Option<String>,
    cancellation: &DownloadCancellation,
) -> Result<(), String> {
    if media_urls.is_empty() {
        return Err("Nenhuma mídia encontrada para baixar".to_string());
    }

    let download_dir =
        twitter_download_dir(app_handle)?.join(format!("@{}", sanitize_filename(username)));
    fs::create_dir_all(&download_dir).map_err(|e| e.to_string())?;

    let cookie_header = normalize_cookie_input(cookies.as_deref())?;
    let client = build_twitter_client(cookie_header.as_deref())?;
    let total_items = media_urls.len() as f64;

    for (idx, media_url) in media_urls.iter().enumerate() {
        if cancellation.is_canceled() {
            return Err(DOWNLOAD_CANCELED_ERROR.to_string());
        }
        let ext = media_extension(media_url);
        let output_path = download_dir.join(format!("{:04}.{}", idx + 1, ext));
        let mut response = tokio::select! {
            result = client.get(media_url).send() => result.map_err(|e| e.to_string())?,
            _ = cancellation.wait() => return Err(DOWNLOAD_CANCELED_ERROR.to_string()),
        };

        if !response.status().is_success() {
            return Err(format!(
                "Twitter profile media HTTP {} em {}",
                response.status(),
                media_url
            ));
        }

        let mut file = fs::File::create(&output_path).map_err(|e| e.to_string())?;
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            if cancellation.is_canceled() {
                drop(file);
                let _ = fs::remove_file(&output_path);
                return Err(DOWNLOAD_CANCELED_ERROR.to_string());
            }
            file.write_all(&chunk).map_err(|e| e.to_string())?;
        }

        let progress = (((idx + 1) as f64 / total_items) * 100.0).min(99.0);
        app_handle.emit(
            "twitter-download-progress",
            TwitterDownloadProgressEvent {
                id: id.to_string(),
                progress,
            },
        );
    }

    app_handle.emit(
        "twitter-download-done",
        TwitterDownloadDoneEvent { id: id.to_string() },
    );
    Ok(())
}

async fn download_twitter_native_inner(
    app_handle: &AppEnv,
    id: &str,
    url: &str,
    filename: &str,
    cookies: Option<String>,
    cancellation: &DownloadCancellation,
) -> Result<(), String> {
    let download_dir = twitter_download_dir(app_handle)?;
    fs::create_dir_all(&download_dir).map_err(|e| e.to_string())?;
    let output_path = download_dir.join(filename);

    if cancellation.is_canceled() {
        return Err(DOWNLOAD_CANCELED_ERROR.to_string());
    }
    let cookie_header = normalize_cookie_input(cookies.as_deref())?;
    if cookie_header.is_some() {
        tracing::debug!("using Twitter cookies for media download");
    }
    let mut headers = std::collections::HashMap::new();
    headers.insert("referer".to_string(), "https://x.com/".to_string());
    headers.insert(
        "user-agent".to_string(),
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120 Safari/537.36".to_string(),
    );
    if let Some(cookie_header) = cookie_header {
        headers.insert("cookie".to_string(), cookie_header);
    }
    plasma_download::download_url_to_file(
        app_handle,
        id,
        url,
        &output_path.to_string_lossy(),
        Some(headers),
    )
    .await?;

    app_handle.emit(
        "twitter-download-done",
        TwitterDownloadDoneEvent { id: id.to_string() },
    );
    Ok(())
}
