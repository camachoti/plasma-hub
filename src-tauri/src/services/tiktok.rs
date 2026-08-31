use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager};

#[cfg(not(target_os = "android"))]
use regex::Regex;
#[cfg(not(target_os = "android"))]
use serde_json::Value;
#[cfg(not(target_os = "android"))]
use std::{fs, io::Write, path::Path, process::Stdio};
#[cfg(not(target_os = "android"))]
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

#[cfg(not(target_os = "android"))]
use super::{
    download_control::{DownloadCancellation, DOWNLOAD_CANCELED_ERROR},
    youtube::ensure_ytdlp,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TikTokMediaInfo {
    platform: String,
    title: String,
    author: String,
    author_full: Option<String>,
    duration: String,
    views: Option<String>,
    likes: Option<String>,
    thumb_hue: u16,
    thumb_hue2: u16,
    thumbnail_url: Option<String>,
    original_url: String,
    formats: TikTokFormats,
}

#[derive(Debug, Serialize)]
struct TikTokFormats {
    video: Vec<TikTokFormatOption>,
    audio: Vec<TikTokFormatOption>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TikTokFormatOption {
    id: String,
    label: String,
    sub: String,
    size: String,
    best: bool,
    url: Option<String>,
}

#[cfg(not(target_os = "android"))]
#[derive(Debug, Deserialize)]
struct TikWmResponse {
    code: i64,
    msg: String,
    data: Option<TikWmData>,
}

#[cfg(not(target_os = "android"))]
#[derive(Debug, Deserialize)]
struct TikWmData {
    id: String,
    title: String,
    cover: Option<String>,
    duration: f64,
    play: Option<String>,
    music: Option<String>,
    size: Option<u64>,
    play_count: Option<u64>,
    digg_count: Option<u64>,
    author: Option<TikWmAuthor>,
}

#[cfg(not(target_os = "android"))]
#[derive(Debug, Deserialize)]
struct TikWmAuthor {
    unique_id: String,
    nickname: String,
}

#[derive(Clone, Serialize)]
struct TikTokDownloadProgressEvent {
    id: String,
    progress: f64,
}

#[derive(Clone, Serialize)]
struct TikTokDownloadErrorEvent {
    id: String,
    error: String,
}

#[derive(Clone, Serialize)]
struct TikTokDownloadDoneEvent {
    id: String,
}

#[cfg(not(target_os = "android"))]
fn format_duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{}:{:02}", minutes, seconds)
    }
}

#[cfg(not(target_os = "android"))]
fn format_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

#[cfg(not(target_os = "android"))]
fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return "—".to_string();
    }
    let megabytes = bytes as f64 / 1024.0 / 1024.0;
    if megabytes < 1.0 {
        format!("{:.0} KB", megabytes * 1024.0)
    } else {
        format!("{:.1} MB", megabytes)
    }
}

#[cfg(not(target_os = "android"))]
async fn analyze_tiktok_with_tikwm(url: &str) -> Result<TikTokMediaInfo, String> {
    eprintln!("[tiktok_native] analyzing with TikWM fallback");
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36",
        )
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get("https://www.tikwm.com/api/")
        .query(&[("url", url)])
        .send()
        .await
        .map_err(|error| format!("TikWM indisponível: {}", error))?
        .error_for_status()
        .map_err(|error| format!("TikWM HTTP: {}", error))?
        .json::<TikWmResponse>()
        .await
        .map_err(|error| format!("Resposta inválida do TikWM: {}", error))?;

    if response.code != 0 {
        return Err(format!("TikWM: {}", response.msg));
    }
    let data = response
        .data
        .ok_or_else(|| "TikWM não retornou dados do vídeo.".to_string())?;
    let author = data
        .author
        .as_ref()
        .map(|author| format!("@{}", author.unique_id))
        .unwrap_or_else(|| "TikTok".to_string());
    let author_full = data.author.as_ref().map(|author| author.nickname.clone());
    let title = if data.title.trim().is_empty() {
        format!("TikTok Video #{}", data.id)
    } else {
        data.title.chars().take(160).collect()
    };

    let video = data
        .play
        .map(|video_url| {
            vec![TikTokFormatOption {
                id: "tikwm-video".to_string(),
                label: "Melhor qualidade".to_string(),
                sub: "MP4 · TikTok".to_string(),
                size: data
                    .size
                    .map(format_size)
                    .unwrap_or_else(|| "—".to_string()),
                best: true,
                url: Some(video_url),
            }]
        })
        .unwrap_or_default();
    let audio = data
        .music
        .map(|audio_url| {
            vec![TikTokFormatOption {
                id: "tikwm-audio".to_string(),
                label: "Som original".to_string(),
                sub: "MP3 · TikTok".to_string(),
                size: "—".to_string(),
                best: true,
                url: Some(audio_url),
            }]
        })
        .unwrap_or_default();

    if video.is_empty() {
        return Err("TikWM não retornou uma URL de vídeo.".to_string());
    }

    Ok(TikTokMediaInfo {
        platform: "tiktok".to_string(),
        title,
        author,
        author_full,
        duration: format_duration(data.duration),
        views: data.play_count.map(format_count),
        likes: data.digg_count.map(format_count),
        thumb_hue: 320,
        thumb_hue2: 260,
        thumbnail_url: data.cover,
        original_url: url.to_string(),
        formats: TikTokFormats { video, audio },
    })
}

#[cfg(not(target_os = "android"))]
async fn request_tiktok_media(
    client: &reqwest::Client,
    media_url: &str,
    cancellation: &DownloadCancellation,
) -> Result<reqwest::Response, String> {
    tokio::select! {
        response = client
            .get(media_url)
            .header("accept", "video/mp4,audio/mpeg,video/*;q=0.9,audio/*;q=0.9,*/*;q=0.8")
            .header("referer", "https://www.tiktok.com/")
            .send() => response.map_err(|error| error.to_string()),
        _ = cancellation.wait() => Err(DOWNLOAD_CANCELED_ERROR.to_string()),
    }
}

#[cfg(not(target_os = "android"))]
async fn download_tiktok_direct(
    app_handle: &tauri::AppHandle,
    id: &str,
    source_url: &str,
    format_id: &str,
    media_url: &str,
    output_path: &Path,
    cancellation: &DownloadCancellation,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/150 Safari/537.36",
        )
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .map_err(|error| error.to_string())?;
    let mut response = request_tiktok_media(&client, media_url, cancellation).await?;

    if response.status() == reqwest::StatusCode::FORBIDDEN {
        eprintln!("[tiktok_native] direct URL expired; refreshing with TikWM");
        let refreshed = analyze_tiktok_with_tikwm(source_url).await?;
        let refreshed_url = if format_id.contains("audio") {
            refreshed
                .formats
                .audio
                .first()
                .and_then(|format| format.url.as_deref())
        } else {
            refreshed
                .formats
                .video
                .first()
                .and_then(|format| format.url.as_deref())
        }
        .ok_or_else(|| "TikWM não retornou uma URL renovada.".to_string())?;
        response = request_tiktok_media(&client, refreshed_url, cancellation).await?;
    }

    if !response.status().is_success() {
        return Err(format!("TikTok CDN HTTP {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let partial_path = format!("{}.part", output_path.to_string_lossy());
    let result = async {
        let mut file = fs::File::create(&partial_path).map_err(|error| error.to_string())?;
        let mut received = 0_u64;
        loop {
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk.map_err(|error| error.to_string())?,
                _ = cancellation.wait() => return Err(DOWNLOAD_CANCELED_ERROR.to_string()),
            };
            let Some(chunk) = chunk else { break };
            file.write_all(&chunk).map_err(|error| error.to_string())?;
            received += chunk.len() as u64;
            let progress = if total > 0 {
                ((received as f64 / total as f64) * 100.0).min(99.0)
            } else {
                50.0
            };
            let _ = app_handle.emit(
                "tiktok-download-progress",
                TikTokDownloadProgressEvent {
                    id: id.to_string(),
                    progress,
                },
            );
        }
        drop(file);
        let _ = fs::remove_file(output_path);
        fs::rename(&partial_path, output_path).map_err(|error| error.to_string())?;
        Ok(())
    }
    .await;

    if result.is_err() {
        let _ = fs::remove_file(&partial_path);
    }
    result
}

#[tauri::command]
#[cfg(not(target_os = "android"))]
pub async fn analyze_tiktok_native(
    app_handle: tauri::AppHandle,
    url: String,
) -> Result<TikTokMediaInfo, String> {
    let binary_path = ensure_ytdlp(&app_handle).await?;
    let output = Command::new(binary_path)
        .arg("--dump-single-json")
        .arg("--no-playlist")
        .arg("--skip-download")
        .arg(&url)
        .output()
        .await
        .map_err(|error| error.to_string())?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let native_error = if error.is_empty() {
            "Não foi possível analisar o vídeo do TikTok.".to_string()
        } else {
            format!("TikTok nativo: {}", error)
        };
        eprintln!("[tiktok_native] yt-dlp analyzer failed: {}", native_error);
        return analyze_tiktok_with_tikwm(&url)
            .await
            .map_err(|fallback_error| format!("{}. Fallback: {}", native_error, fallback_error));
    }

    let data: Value = match serde_json::from_slice(&output.stdout) {
        Ok(data) => data,
        Err(error) => {
            eprintln!("[tiktok_native] invalid yt-dlp response: {}", error);
            return analyze_tiktok_with_tikwm(&url).await;
        }
    };
    let title = data
        .get("title")
        .or_else(|| data.get("description"))
        .and_then(Value::as_str)
        .unwrap_or("TikTok Video")
        .chars()
        .take(160)
        .collect::<String>();
    let author_id = data
        .get("uploader")
        .or_else(|| data.get("channel_id"))
        .and_then(Value::as_str)
        .unwrap_or("TikTok");
    let author = if author_id.starts_with('@') {
        author_id.to_string()
    } else if author_id == "TikTok" {
        author_id.to_string()
    } else {
        format!("@{}", author_id)
    };
    let video_url = data.get("url").and_then(Value::as_str).map(str::to_string);
    let audio_url = data
        .get("formats")
        .and_then(Value::as_array)
        .and_then(|formats| {
            formats.iter().find_map(|format| {
                let is_audio_only = format
                    .get("vcodec")
                    .and_then(Value::as_str)
                    .is_some_and(|codec| codec == "none");
                is_audio_only
                    .then(|| {
                        format
                            .get("url")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                    })
                    .flatten()
            })
        });

    Ok(TikTokMediaInfo {
        platform: "tiktok".to_string(),
        title,
        author,
        author_full: data
            .get("channel")
            .or_else(|| data.get("uploader"))
            .and_then(Value::as_str)
            .map(str::to_string),
        duration: data
            .get("duration")
            .and_then(Value::as_f64)
            .map(format_duration)
            .unwrap_or_else(|| "—".to_string()),
        views: data
            .get("view_count")
            .and_then(Value::as_u64)
            .map(format_count),
        likes: data
            .get("like_count")
            .and_then(Value::as_u64)
            .map(format_count),
        thumb_hue: 320,
        thumb_hue2: 260,
        thumbnail_url: data
            .get("thumbnail")
            .and_then(Value::as_str)
            .map(str::to_string),
        original_url: url,
        formats: TikTokFormats {
            video: vec![TikTokFormatOption {
                id: "best".to_string(),
                label: "Melhor qualidade".to_string(),
                sub: "MP4 · TikTok".to_string(),
                size: "—".to_string(),
                best: true,
                url: video_url,
            }],
            audio: vec![TikTokFormatOption {
                id: "bestaudio".to_string(),
                label: "Som original".to_string(),
                sub: "MP3 · TikTok".to_string(),
                size: "—".to_string(),
                best: true,
                url: audio_url,
            }],
        },
    })
}

#[tauri::command]
#[cfg(target_os = "android")]
pub async fn analyze_tiktok_native(
    _app_handle: tauri::AppHandle,
    _url: String,
) -> Result<TikTokMediaInfo, String> {
    Err("Analisador TikTok nativo indisponível no Android.".to_string())
}

#[tauri::command]
#[cfg(not(target_os = "android"))]
pub async fn download_tiktok_native(
    app_handle: tauri::AppHandle,
    id: String,
    url: String,
    format_id: String,
    filename: String,
    direct_url: Option<String>,
) -> Result<(), String> {
    let cancellation = DownloadCancellation::register(id.clone());
    let download_dir = app_handle
        .path()
        .download_dir()
        .map_err(|error| error.to_string())?;
    let output_path = download_dir.join(filename);

    if let Some(direct_url) = direct_url.filter(|url| !url.trim().is_empty()) {
        let result = download_tiktok_direct(
            &app_handle,
            &id,
            &url,
            &format_id,
            &direct_url,
            &output_path,
            &cancellation,
        )
        .await;
        match result {
            Ok(()) => {
                let _ = app_handle.emit(
                    "tiktok-download-done",
                    TikTokDownloadDoneEvent { id: id.clone() },
                );
                return Ok(());
            }
            Err(error) => {
                if error == DOWNLOAD_CANCELED_ERROR {
                    return Err(error);
                }
                let _ = app_handle.emit(
                    "tiktok-download-error",
                    TikTokDownloadErrorEvent {
                        id,
                        error: error.clone(),
                    },
                );
                return Err(error);
            }
        }
    }

    let binary_path = ensure_ytdlp(&app_handle).await?;

    let mut command = Command::new(binary_path);
    command
        .arg("--no-playlist")
        .arg("--no-part")
        .arg("--newline");
    if format_id == "bestaudio" {
        command
            .arg("-f")
            .arg("bestaudio/best")
            .arg("--extract-audio")
            .arg("--audio-format")
            .arg("mp3");
    } else {
        command
            .arg("-f")
            .arg("best[vcodec^=avc1]/best[vcodec=h264]/best");
    }
    let mut child = command
        .arg("-o")
        .arg(&output_path)
        .arg(&url)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|error| error.to_string())?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Não foi possível acompanhar o download do TikTok.".to_string())?;
    let mut reader = BufReader::new(stdout).lines();
    let progress_regex = Regex::new(r"\[download\]\s+([\d\.]+)%").map_err(|e| e.to_string())?;
    let app_clone = app_handle.clone();
    let id_clone = id.clone();
    tokio::spawn(async move {
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(captures) = progress_regex.captures(&line) {
                if let Ok(progress) = captures[1].parse::<f64>() {
                    let _ = app_clone.emit(
                        "tiktok-download-progress",
                        TikTokDownloadProgressEvent {
                            id: id_clone.clone(),
                            progress,
                        },
                    );
                }
            }
        }
    });

    let status = tokio::select! {
        status = child.wait() => status.map_err(|error| error.to_string())?,
        _ = cancellation.wait() => {
            let _ = child.kill().await;
            let _ = fs::remove_file(&output_path);
            return Err(DOWNLOAD_CANCELED_ERROR.to_string());
        }
    };

    if status.success() {
        let _ = app_handle.emit(
            "tiktok-download-done",
            TikTokDownloadDoneEvent { id: id.clone() },
        );
        Ok(())
    } else {
        let error = "O download nativo do TikTok falhou.".to_string();
        let _ = app_handle.emit(
            "tiktok-download-error",
            TikTokDownloadErrorEvent {
                id,
                error: error.clone(),
            },
        );
        Err(error)
    }
}

#[tauri::command]
#[cfg(target_os = "android")]
pub async fn download_tiktok_native(
    _app_handle: tauri::AppHandle,
    _id: String,
    _url: String,
    _format_id: String,
    _filename: String,
    _direct_url: Option<String>,
) -> Result<(), String> {
    Err("Download TikTok nativo indisponível no Android.".to_string())
}
