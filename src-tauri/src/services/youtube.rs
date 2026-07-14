#[cfg(not(target_os = "android"))]
use regex::Regex;
#[cfg(not(target_os = "android"))]
use serde::Serialize;
#[cfg(not(target_os = "android"))]
use std::fs;
#[cfg(not(target_os = "android"))]
use std::process::Stdio;
#[cfg(not(target_os = "android"))]
use tauri::{Emitter, Manager};
#[cfg(not(target_os = "android"))]
use tokio::io::{AsyncBufReadExt, BufReader};
#[cfg(not(target_os = "android"))]
use tokio::process::Command;

#[cfg(not(target_os = "android"))]
#[derive(Clone, Serialize)]
struct DownloadProgressEvent {
    id: String,
    progress: f64,
}

#[cfg(not(target_os = "android"))]
#[derive(Clone, Serialize)]
struct DownloadErrorEvent {
    id: String,
    error: String,
}

#[cfg(not(target_os = "android"))]
#[derive(Clone, Serialize)]
struct DownloadDoneEvent {
    id: String,
}

#[cfg(not(target_os = "android"))]
pub async fn ensure_ytdlp(app_handle: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let app_dir = app_handle
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(&app_dir).unwrap_or(());

    #[cfg(target_os = "windows")]
    let binary_name = "yt-dlp.exe";
    #[cfg(not(target_os = "windows"))]
    let binary_name = "yt-dlp";

    let binary_path = app_dir.join(binary_name);

    if !binary_path.exists() {
        #[cfg(target_os = "windows")]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe";
        #[cfg(target_os = "linux")]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp";
        #[cfg(target_os = "macos")]
        let download_url = "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos";

        let response = reqwest::get(download_url)
            .await
            .map_err(|e| e.to_string())?;
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        fs::write(&binary_path, bytes).map_err(|e| e.to_string())?;

        #[cfg(not(target_os = "windows"))]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(&binary_path)
                .map_err(|e| e.to_string())?
                .permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&binary_path, perms).map_err(|e| e.to_string())?;
        }
    }

    Ok(binary_path)
}

#[tauri::command]
#[cfg(not(target_os = "android"))]
pub async fn get_youtube_stream_url(
    app_handle: tauri::AppHandle,
    url: String,
    format_id: String,
) -> Result<String, String> {
    let binary_path = ensure_ytdlp(&app_handle).await?;

    let format_arg = if format_id == "video" || format_id == "audio" {
        "best"
    } else {
        &format_id
    };

    let output = std::process::Command::new(&binary_path)
        .arg("-g")
        .arg("-f")
        .arg(format_arg)
        .arg(&url)
        .output()
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let fallback = std::process::Command::new(&binary_path)
            .arg("-g")
            .arg(&url)
            .output()
            .map_err(|e| e.to_string())?;

        if !fallback.status.success() {
            let err = String::from_utf8_lossy(&fallback.stderr);
            return Err(format!("yt-dlp falhou: {}", err));
        }

        let url_str = String::from_utf8_lossy(&fallback.stdout).trim().to_string();
        let first_url = url_str.lines().next().unwrap_or("").to_string();
        return Ok(first_url);
    }

    let url_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let first_url = url_str.lines().next().unwrap_or("").to_string();

    Ok(first_url)
}

#[tauri::command]
#[cfg(target_os = "android")]
pub async fn get_youtube_stream_url(
    _app_handle: tauri::AppHandle,
    _url: String,
    _format_id: String,
) -> Result<String, String> {
    Err("YouTube via yt-dlp ainda não está disponível no Android.".to_string())
}

#[tauri::command]
#[cfg(not(target_os = "android"))]
pub async fn download_youtube_native(
    app_handle: tauri::AppHandle,
    id: String,
    url: String,
    format_id: String,
    filename: String,
) -> Result<(), String> {
    let binary_path = ensure_ytdlp(&app_handle).await?;

    let format_arg = if format_id == "video" || format_id == "audio" {
        "best"
    } else {
        &format_id
    };

    let download_dir = app_handle
        .path()
        .download_dir()
        .map_err(|e| e.to_string())?;
    let output_path = download_dir.join(&filename);

    let mut child = Command::new(&binary_path)
        .arg("-f")
        .arg(format_arg)
        .arg("-o")
        .arg(output_path)
        .arg("--newline")
        .arg(&url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;

    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout).lines();
    let re = Regex::new(r"\[download\]\s+([\d\.]+)%").unwrap();
    let app_clone = app_handle.clone();
    let id_clone = id.clone();

    tokio::spawn(async move {
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(caps) = re.captures(&line) {
                if let Ok(pct) = caps[1].parse::<f64>() {
                    let _ = app_clone.emit(
                        "youtube-download-progress",
                        DownloadProgressEvent {
                            id: id_clone.clone(),
                            progress: pct,
                        },
                    );
                }
            }
        }
    });

    let status = child.wait().await.map_err(|e| e.to_string())?;

    if status.success() {
        let _ = app_handle.emit("youtube-download-done", DownloadDoneEvent { id });
        Ok(())
    } else {
        let _ = app_handle.emit(
            "youtube-download-error",
            DownloadErrorEvent {
                id,
                error: "Processo de download falhou ou ffmpeg não encontrado".to_string(),
            },
        );
        Err("Download falhou".to_string())
    }
}

#[tauri::command]
#[cfg(target_os = "android")]
pub async fn download_youtube_native(
    _app_handle: tauri::AppHandle,
    _id: String,
    _url: String,
    _format_id: String,
    _filename: String,
) -> Result<(), String> {
    Err("Downloads do YouTube ainda não estão disponíveis no Android.".to_string())
}
