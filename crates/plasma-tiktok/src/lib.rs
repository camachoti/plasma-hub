use plasma_core::AppEnv;
use serde::{Deserialize, Serialize};

#[cfg(not(target_os = "android"))]
use regex::Regex;
#[cfg(not(target_os = "android"))]
use serde_json::Value;
#[cfg(not(target_os = "android"))]
use std::{fs, path::Path, process::Stdio};
#[cfg(not(target_os = "android"))]
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
};

#[cfg(not(target_os = "android"))]
use plasma_download::{DownloadCancellation, DOWNLOAD_CANCELED_ERROR};
#[cfg(not(target_os = "android"))]
use plasma_youtube::ensure_ytdlp;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TikTokMediaInfo {
    pub platform: String,
    pub title: String,
    pub author: String,
    pub author_full: Option<String>,
    pub duration: String,
    pub views: Option<String>,
    pub likes: Option<String>,
    pub thumb_hue: u16,
    pub thumb_hue2: u16,
    pub thumbnail_url: Option<String>,
    pub original_url: String,
    pub formats: TikTokFormats,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TikTokFormats {
    pub video: Vec<TikTokFormatOption>,
    pub audio: Vec<TikTokFormatOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TikTokFormatOption {
    pub id: String,
    pub label: String,
    pub sub: String,
    pub size: String,
    pub best: bool,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TikWmResponse {
    code: i64,
    msg: String,
    data: Option<TikWmData>,
}

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

#[derive(Debug, Deserialize)]
struct TikWmAuthor {
    unique_id: String,
    nickname: String,
}

#[cfg(not(target_os = "android"))]
#[derive(Clone, Serialize)]
struct TikTokDownloadProgressEvent {
    id: String,
    progress: f64,
}

#[cfg(not(target_os = "android"))]
#[derive(Clone, Serialize)]
struct TikTokDownloadErrorEvent {
    id: String,
    error: String,
}

#[cfg(not(target_os = "android"))]
#[derive(Clone, Serialize)]
struct TikTokDownloadDoneEvent {
    id: String,
}

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

fn format_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

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

pub async fn analyze_tiktok_with_tikwm(url: &str) -> Result<TikTokMediaInfo, String> {
    tracing::debug!("analyzing TikTok with TikWM fallback");
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
async fn download_tiktok_direct(
    app_handle: &AppEnv,
    id: &str,
    source_url: &str,
    format_id: &str,
    media_url: &str,
    output_path: &Path,
    cancellation: &DownloadCancellation,
) -> Result<(), String> {
    let _ = cancellation;
    let destination = output_path.to_string_lossy().to_string();
    let headers = tiktok_download_headers();
    let identity = format!("tiktok|{source_url}|{format_id}");
    match plasma_download::download_url_to_file_with_identity(
        app_handle,
        id,
        media_url,
        &destination,
        Some(headers.clone()),
        Some(&identity),
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(error) if error.contains("403") => {
            tracing::debug!("TikTok direct URL expired; continuing the same file");
            let refreshed = analyze_tiktok_with_tikwm(source_url).await?;
            let refreshed_url = if format_id.contains("audio") {
                refreshed
                    .formats
                    .audio
                    .first()
                    .and_then(|format| format.url.clone())
            } else {
                refreshed
                    .formats
                    .video
                    .first()
                    .and_then(|format| format.url.clone())
            }
            .ok_or_else(|| "TikWM não retornou uma URL renovada.".to_string())?;
            plasma_download::download_url_to_file_with_identity(
                app_handle,
                id,
                &refreshed_url,
                &destination,
                Some(headers),
                Some(&identity),
            )
            .await
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(target_os = "android"))]
fn tiktok_download_headers() -> std::collections::HashMap<String, String> {
    let mut headers = std::collections::HashMap::new();
    headers.insert("referer".to_string(), "https://www.tiktok.com/".to_string());
    headers.insert(
        "user-agent".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/150 Safari/537.36"
            .to_string(),
    );
    headers
}

#[cfg(not(target_os = "android"))]
pub async fn analyze_tiktok_native(
    app_handle: &AppEnv,
    url: String,
) -> Result<TikTokMediaInfo, String> {
    let binary_path = ensure_ytdlp(app_handle).await?;
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
        tracing::debug!(error = %native_error, "yt-dlp TikTok analyzer failed");
        return analyze_tiktok_with_tikwm(&url)
            .await
            .map_err(|fallback_error| format!("{}. Fallback: {}", native_error, fallback_error));
    }

    let data: Value = match serde_json::from_slice(&output.stdout) {
        Ok(data) => data,
        Err(error) => {
            tracing::debug!(%error, "invalid yt-dlp TikTok response");
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
    let author = if author_id.starts_with('@') || author_id == "TikTok" {
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

#[cfg(target_os = "android")]
pub async fn analyze_tiktok_native(
    _app_handle: &AppEnv,
    _url: String,
) -> Result<TikTokMediaInfo, String> {
    Err("Analisador TikTok nativo indisponível no Android.".to_string())
}

#[cfg(not(target_os = "android"))]
pub async fn download_tiktok_native(
    app_handle: &AppEnv,
    id: String,
    url: String,
    format_id: String,
    filename: String,
    direct_url: Option<String>,
) -> Result<(), String> {
    let cancellation = DownloadCancellation::register(id.clone());
    let download_dir = app_handle.paths().download_dir()?;
    let output_path = download_dir.join(filename);

    if let Some(direct_url) = direct_url.filter(|url| !url.trim().is_empty()) {
        let result = download_tiktok_direct(
            app_handle,
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
                app_handle.emit(
                    "tiktok-download-done",
                    TikTokDownloadDoneEvent { id: id.clone() },
                );
                return Ok(());
            }
            Err(error) => {
                if error == DOWNLOAD_CANCELED_ERROR {
                    return Err(error);
                }
                app_handle.emit(
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

    let binary_path = ensure_ytdlp(app_handle).await?;
    let info_path = output_path.with_extension("plasma-info.json");
    if !info_path.is_file() {
        let dumped = Command::new(&binary_path)
            .arg("--no-playlist")
            .arg("-J")
            .arg("-f")
            .arg(if format_id == "bestaudio" {
                "bestaudio/best"
            } else {
                "best[vcodec^=avc1]/best[vcodec=h264]/best"
            })
            .arg(&url)
            .output()
            .await
            .map_err(|error| error.to_string())?;
        if !dumped.status.success() {
            let error = String::from_utf8_lossy(&dumped.stderr);
            return Err(format!("yt-dlp falhou: {error}"));
        }
        fs::write(&info_path, dumped.stdout).map_err(|error| error.to_string())?;
    }

    let mut command = Command::new(&binary_path);
    command
        .arg("--load-info-json")
        .arg(&info_path)
        .arg("--no-clean-info-json")
        .arg("--no-playlist")
        .arg("--continue")
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
    let app_clone = (*app_handle).clone();
    let id_clone = id.clone();
    tokio::spawn(async move {
        while let Ok(Some(line)) = reader.next_line().await {
            if let Some(captures) = progress_regex.captures(&line) {
                if let Ok(progress) = captures[1].parse::<f64>() {
                    app_clone.emit(
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
            return Err(DOWNLOAD_CANCELED_ERROR.to_string());
        }
    };

    if status.success() {
        app_handle.emit(
            "tiktok-download-done",
            TikTokDownloadDoneEvent { id: id.clone() },
        );
        Ok(())
    } else {
        let error = "O download nativo do TikTok falhou.".to_string();
        app_handle.emit(
            "tiktok-download-error",
            TikTokDownloadErrorEvent {
                id,
                error: error.clone(),
            },
        );
        Err(error)
    }
}

#[cfg(target_os = "android")]
pub async fn download_tiktok_native(
    _app_handle: &AppEnv,
    _id: String,
    _url: String,
    _format_id: String,
    _filename: String,
    _direct_url: Option<String>,
) -> Result<(), String> {
    Err("Download TikTok nativo indisponível no Android.".to_string())
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListedTikTokVideo {
    pub id: String,
    pub title: String,
    pub author: String,
    pub page_url: String,
    pub media_url: String,
    pub thumbnail_url: Option<String>,
}

pub async fn list_tiktok_videos(
    sources: &[String],
    interests: &[String],
) -> Result<Vec<ListedTikTokVideo>, String> {
    let client = reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124.0.0.0 Safari/537.36",
        )
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;

    let mut targets: Vec<(bool, String)> = Vec::new();
    for raw in sources {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        let stripped = item
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.")
            .trim_start_matches("tiktok.com/");
        if let Some(handle) = stripped.strip_prefix('@') {
            let clean = handle.split(['/', '?', '#']).next().unwrap_or("").trim();
            if !clean.is_empty() {
                targets.push((true, clean.to_string()));
                continue;
            }
        }
        targets.push((false, item.to_string()));
    }

    if targets.is_empty() {
        for interest in interests.iter().take(2) {
            let kw = match interest.as_str() {
                "humor" => "comedy funny",
                "jogos" => "gaming clips",
                "esportes" => "sports highlights",
                "animais" => "cute animals",
                "ciencia" => "science facts",
                "comida" => "cooking recipe",
                _ => "oddly satisfying",
            };
            targets.push((false, kw.to_string()));
        }
    }

    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (is_user, target) in targets.into_iter().take(3) {
        let req = if is_user {
            client
                .get("https://www.tikwm.com/api/user/posts")
                .query(&[("unique_id", target.as_str()), ("count", "8")])
        } else {
            client
                .get("https://www.tikwm.com/api/feed/search")
                .query(&[("keywords", target.as_str()), ("count", "8")])
        };
        let Ok(resp) = req.send().await else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(json_val) = resp.json::<serde_json::Value>().await else {
            continue;
        };
        for item in parse_tikwm_feed_videos(&json_val) {
            if seen.insert(item.id.clone()) {
                out.push(item);
            }
            if out.len() >= 12 {
                return Ok(out);
            }
        }
    }

    Ok(out)
}

pub fn parse_tikwm_feed_videos(root: &serde_json::Value) -> Vec<ListedTikTokVideo> {
    if root.get("code").and_then(serde_json::Value::as_i64) != Some(0) {
        return Vec::new();
    }
    let Some(videos) = root
        .pointer("/data/videos")
        .and_then(serde_json::Value::as_array)
    else {
        return Vec::new();
    };
    videos
        .iter()
        .filter_map(|v| {
            let id = v
                .get("video_id")
                .or_else(|| v.get("id"))
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())?
                .to_string();
            let raw_play = v.get("play").and_then(serde_json::Value::as_str)?;
            let media_url = normalize_tikwm_url(raw_play)?;
            let unique_id = v
                .pointer("/author/unique_id")
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("tiktok");
            let author = format!("@{unique_id}");
            let title = v
                .get("title")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.chars().take(160).collect())
                .unwrap_or_else(|| author.clone());
            let thumbnail_url = v
                .get("cover")
                .or_else(|| v.get("origin_cover"))
                .and_then(serde_json::Value::as_str)
                .and_then(normalize_tikwm_url);
            Some(ListedTikTokVideo {
                page_url: format!("https://www.tiktok.com/@{unique_id}/video/{id}"),
                id,
                title,
                author,
                media_url,
                thumbnail_url,
            })
        })
        .collect()
}

fn normalize_tikwm_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.starts_with("https://") {
        Some(trimmed.to_string())
    } else if trimmed.starts_with('/') {
        Some(format!("https://www.tikwm.com{trimmed}"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tikwm_feed_response() {
        let sample = serde_json::json!({
            "code": 0,
            "data": {
                "videos": [
                    {
                        "video_id": "73999112233",
                        "title": "Cool Trick",
                        "play": "/video/media/play/73999112233.mp4",
                        "cover": "https://p16-sign.tiktokcdn-us.com/cover.jpg",
                        "author": { "unique_id": "skater", "nickname": "Skater" }
                    }
                ]
            }
        });
        let list = parse_tikwm_feed_videos(&sample);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "73999112233");
        assert_eq!(list[0].author, "@skater");
        assert_eq!(
            list[0].media_url,
            "https://www.tikwm.com/video/media/play/73999112233.mp4"
        );
        assert_eq!(
            list[0].page_url,
            "https://www.tiktok.com/@skater/video/73999112233"
        );
    }
}
