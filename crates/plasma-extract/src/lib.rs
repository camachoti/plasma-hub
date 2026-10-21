//! Unified native media extraction for the Rust shell.
//!
//! The React downloader has one extractor per platform. This crate provides the
//! same contract to the Rust UI: a [`MediaInfo`] with video/audio [`FormatOption`]
//! lists, plus a download dispatcher that reuses the existing native downloaders
//! for Twitter, YouTube and TikTok and the shared HTTPS downloader for direct
//! Instagram/Reddit/web media.

use plasma_core::AppEnv;
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaInfo {
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
    pub formats: MediaFormats,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MediaFormats {
    pub video: Vec<FormatOption>,
    pub audio: Vec<FormatOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatOption {
    pub id: String,
    pub label: String,
    pub sub: String,
    pub size: String,
    pub best: Option<bool>,
    pub url: Option<String>,
    pub has_audio: bool,
}

impl FormatOption {
    pub fn new(id: impl Into<String>, label: impl Into<String>, sub: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            sub: sub.into(),
            size: "—".to_string(),
            best: None,
            url: None,
            has_audio: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Twitter,
    YouTube,
    TikTok,
    Instagram,
    Reddit,
    Web,
}

impl Platform {
    pub fn detect(url: &str) -> Self {
        let lower = url.to_ascii_lowercase();
        let host = lower
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(&lower)
            .split('/')
            .next()
            .unwrap_or_default()
            .trim_start_matches("www.");
        if host == "x.com" || host == "twitter.com" {
            Self::Twitter
        } else if host == "youtube.com"
            || host == "m.youtube.com"
            || host == "youtu.be"
            || host.ends_with(".youtube.com")
        {
            Self::YouTube
        } else if host == "tiktok.com" || host.ends_with(".tiktok.com") {
            Self::TikTok
        } else if host == "instagram.com" || host.ends_with(".instagram.com") {
            Self::Instagram
        } else if host == "reddit.com" || host == "redd.it" || host.ends_with(".reddit.com") {
            Self::Reddit
        } else {
            Self::Web
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Twitter => "twitter",
            Self::YouTube => "youtube",
            Self::TikTok => "tiktok",
            Self::Instagram => "instagram",
            Self::Reddit => "reddit",
            Self::Web => "web",
        }
    }
}

/// Resolves the metadata for a supported URL.
pub async fn analyze_url(
    env: &AppEnv,
    url: &str,
    cookies: Option<String>,
) -> Result<MediaInfo, String> {
    let url = url.trim();
    if url.is_empty() {
        return Err("Informe uma URL.".to_string());
    }
    match Platform::detect(url) {
        Platform::Twitter => analyze_twitter(url, cookies).await,
        Platform::YouTube => analyze_youtube(url).await,
        Platform::TikTok => analyze_tiktok(env, url).await,
        Platform::Instagram => analyze_instagram(url, cookies).await,
        Platform::Reddit => analyze_reddit(url).await,
        Platform::Web => {
            let _ = env;
            Err(
                "Não há analisador para este domínio. Use “Baixar URL” para um link direto."
                    .to_string(),
            )
        }
    }
}

/// One selected download. `file_name` is a plain filename inside the
/// downloads folder.
#[derive(Clone, Debug)]
pub struct DownloadSpec {
    pub id: String,
    pub original_url: String,
    pub format_id: String,
    pub direct_url: Option<String>,
    pub file_name: String,
    pub has_audio: bool,
    pub cookies: Option<String>,
}

/// Downloads one selected format, dispatching to the native downloader used by
/// that platform.
pub async fn download_format(env: &AppEnv, spec: DownloadSpec) -> Result<(), String> {
    let DownloadSpec {
        id,
        original_url,
        format_id,
        direct_url,
        file_name,
        has_audio,
        cookies,
    } = spec;
    match Platform::detect(&original_url) {
        Platform::Twitter => {
            let direct_url = direct_url.ok_or_else(|| {
                "Esta variante do X/Twitter não tem URL direta. Reanalise o post.".to_string()
            })?;
            plasma_twitter::download_twitter_native(env, id, direct_url, file_name, cookies).await
        }
        Platform::YouTube => {
            let format_id = if has_audio {
                format_id
            } else {
                let audio_extension = if file_name.ends_with(".webm") {
                    "webm"
                } else {
                    "m4a"
                };
                format!("{format_id}+bestaudio[ext={audio_extension}]/best")
            };
            plasma_youtube::download_youtube_native(env, id, original_url, format_id, file_name)
                .await
        }
        Platform::TikTok => {
            #[cfg(target_os = "android")]
            {
                let direct_url = direct_url.ok_or_else(|| {
                    "O TikTok não retornou uma URL direta para este formato.".to_string()
                })?;
                download_direct(env, &id, &original_url, &direct_url, &file_name).await
            }
            #[cfg(not(target_os = "android"))]
            {
                plasma_tiktok::download_tiktok_native(
                    env,
                    id,
                    original_url,
                    format_id,
                    file_name,
                    direct_url,
                )
                .await
            }
        }
        Platform::Instagram | Platform::Reddit | Platform::Web => {
            let direct_url = direct_url
                .ok_or_else(|| "Este formato não tem URL direta para download.".to_string())?;
            download_direct(env, &id, &original_url, &direct_url, &file_name).await
        }
    }
}

async fn download_direct(
    env: &AppEnv,
    id: &str,
    original_url: &str,
    direct_url: &str,
    file_name: &str,
) -> Result<(), String> {
    let directory = env
        .paths()
        .download_dir()
        .unwrap_or_else(|_| env.paths().data_dir().join("downloads"));
    let path = directory.join(sanitize_filename(file_name));
    let headers = request_headers(original_url);
    plasma_download::download_url_to_file(
        env,
        id,
        direct_url,
        &path.to_string_lossy(),
        Some(headers),
    )
    .await
}

fn request_headers(original_url: &str) -> std::collections::HashMap<String, String> {
    let mut headers = std::collections::HashMap::new();
    headers.insert(
        "user-agent".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36"
            .to_string(),
    );
    match Platform::detect(original_url) {
        Platform::Instagram => {
            headers.insert(
                "referer".to_string(),
                "https://www.instagram.com/".to_string(),
            );
        }
        Platform::Reddit => {
            headers.insert("referer".to_string(), "https://www.reddit.com/".to_string());
        }
        Platform::TikTok => {
            headers.insert("referer".to_string(), "https://www.tiktok.com/".to_string());
        }
        _ => {}
    }
    headers
}

async fn analyze_twitter(url: &str, cookies: Option<String>) -> Result<MediaInfo, String> {
    let info = plasma_twitter::analyze_twitter_tweet_native(url.to_string(), cookies).await?;
    let video = info
        .formats
        .video
        .into_iter()
        .map(|item| FormatOption {
            id: item.id,
            label: item.label,
            sub: item.sub,
            size: item.size,
            best: item.best,
            url: item.url,
            has_audio: true,
        })
        .collect();
    let audio = info
        .formats
        .audio
        .into_iter()
        .map(|item| FormatOption {
            id: item.id,
            label: item.label,
            sub: item.sub,
            size: item.size,
            best: item.best,
            url: item.url,
            has_audio: true,
        })
        .collect();
    Ok(MediaInfo {
        platform: info.platform,
        title: info.title,
        author: info.author,
        author_full: info.author_full,
        duration: info.duration,
        views: None,
        likes: None,
        thumb_hue: info.thumb_hue,
        thumb_hue2: info.thumb_hue2,
        thumbnail_url: info.thumbnail_url,
        original_url: info.original_url,
        formats: MediaFormats { video, audio },
    })
}

async fn analyze_youtube(url: &str) -> Result<MediaInfo, String> {
    let video_id = youtube_id(url)
        .ok_or_else(|| "Não foi possível identificar o ID do YouTube.".to_string())?;
    let client = client()?;
    let response = client
        .get(format!("https://www.youtube.com/watch?v={video_id}"))
        .header("accept-language", "pt-BR,pt;q=0.9,en;q=0.8")
        .send()
        .await
        .map_err(|error| format!("YouTube indisponível: {error}"))?
        .error_for_status()
        .map_err(|error| format!("YouTube HTTP: {error}"))?;
    let html = response
        .text()
        .await
        .map_err(|error| format!("Falha ao ler o YouTube: {error}"))?;
    let start = html
        .find("ytInitialPlayerResponse = ")
        .ok_or_else(|| "Dados do YouTube não encontrados na página.".to_string())?
        + "ytInitialPlayerResponse = ".len();
    let rest = &html[start..];
    let end = json_object_end(rest)
        .ok_or_else(|| "Fim dos dados do YouTube não encontrado.".to_string())?;
    let data: Value = serde_json::from_str(&rest[..end])
        .map_err(|error| format!("JSON inválido do YouTube: {error}"))?;

    let status = data
        .pointer("/playabilityStatus/status")
        .and_then(Value::as_str)
        .unwrap_or("OK");
    if matches!(status, "ERROR" | "LOGIN_REQUIRED") {
        return Err(data
            .pointer("/playabilityStatus/reason")
            .and_then(Value::as_str)
            .unwrap_or("Vídeo indisponível ou requer login.")
            .to_string());
    }

    let details = data.get("videoDetails").unwrap_or(&Value::Null);
    let title = details
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("YouTube Video")
        .to_string();
    let author = details
        .get("author")
        .and_then(Value::as_str)
        .unwrap_or("YouTube")
        .to_string();
    let duration = details
        .get("lengthSeconds")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<u64>().ok())
        .map(format_duration)
        .unwrap_or_else(|| "—".to_string());
    let views = details
        .get("viewCount")
        .and_then(Value::as_str)
        .and_then(|value| value.parse::<u64>().ok())
        .map(format_count);
    let thumbnail_url = details
        .pointer("/thumbnail/thumbnails")
        .and_then(Value::as_array)
        .and_then(|items| items.last())
        .and_then(|item| item.get("url"))
        .and_then(Value::as_str)
        .map(str::to_string);

    let muxed = data
        .pointer("/streamingData/formats")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let adaptive = data
        .pointer("/streamingData/adaptiveFormats")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut video = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut all = muxed
        .iter()
        .map(|format| (format.clone(), true))
        .chain(adaptive.iter().map(|format| (format.clone(), false)))
        .collect::<Vec<_>>();
    all.sort_by_key(|(format, _)| {
        std::cmp::Reverse(format.get("height").and_then(Value::as_i64).unwrap_or(0))
    });
    for (format, muxed_format) in all {
        let mime = format
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !mime.starts_with("video/") {
            continue;
        }
        let has_audio = format
            .get("acodec")
            .and_then(Value::as_str)
            .map(|value| value != "none")
            .unwrap_or(muxed_format);
        if cfg!(target_os = "android") && !has_audio {
            continue;
        }
        let label = format
            .get("qualityLabel")
            .or_else(|| format.get("quality"))
            .and_then(Value::as_str)
            .unwrap_or("Vídeo")
            .to_string();
        if !seen.insert(label.clone()) {
            continue;
        }
        let id = format
            .get("format_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                format
                    .get("itag")
                    .and_then(Value::as_i64)
                    .map(|itag| itag.to_string())
            })
            .unwrap_or_else(|| "best".to_string());
        let codec = mime.split(';').next().unwrap_or(mime).to_string();
        let mut option = FormatOption::new(id, label, codec);
        option.size = format_size(&format);
        option.url = format
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string);
        option.has_audio = has_audio;
        if video.is_empty() {
            option.best = Some(true);
        }
        video.push(option);
    }
    if video.is_empty() && !cfg!(target_os = "android") {
        let mut fallback = FormatOption::new("best", "Melhor qualidade", "MP4 · YouTube");
        fallback.best = Some(true);
        fallback.has_audio = true;
        video.push(fallback);
    }
    let audio = adaptive
        .iter()
        .filter(|format| {
            format
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .starts_with("audio/")
        })
        .take(3)
        .enumerate()
        .map(|(index, format)| {
            let mime = format
                .get("mimeType")
                .and_then(Value::as_str)
                .unwrap_or("audio/mp4")
                .split(';')
                .next()
                .unwrap_or("audio/mp4")
                .to_string();
            let id = format
                .get("format_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    format
                        .get("itag")
                        .and_then(Value::as_i64)
                        .map(|itag| itag.to_string())
                })
                .unwrap_or_else(|| "bestaudio".to_string());
            let mut option = FormatOption::new(
                id,
                if index == 0 {
                    "Alta qualidade".to_string()
                } else {
                    format!(
                        "Áudio {}",
                        format
                            .get("format_note")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                    )
                },
                mime,
            );
            option.size = format_size(format);
            option.url = format
                .get("url")
                .and_then(Value::as_str)
                .map(str::to_string);
            option.has_audio = true;
            option.best = Some(index == 0);
            option
        })
        .collect();

    Ok(MediaInfo {
        platform: "youtube".to_string(),
        title,
        author,
        author_full: None,
        duration,
        views,
        likes: None,
        thumb_hue: 0,
        thumb_hue2: 20,
        thumbnail_url,
        original_url: url.to_string(),
        formats: MediaFormats { video, audio },
    })
}

async fn analyze_tiktok(env: &AppEnv, url: &str) -> Result<MediaInfo, String> {
    let info = match plasma_tiktok::analyze_tiktok_native(env, url.to_string()).await {
        Ok(info) => info,
        Err(native_error) => plasma_tiktok::analyze_tiktok_with_tikwm(url)
            .await
            .map_err(|fallback_error| format!("{native_error}. Fallback: {fallback_error}"))?,
    };
    let video = info
        .formats
        .video
        .into_iter()
        .map(|item| FormatOption {
            id: item.id,
            label: item.label,
            sub: item.sub,
            size: item.size,
            best: Some(item.best),
            url: item.url,
            has_audio: true,
        })
        .collect();
    let audio = info
        .formats
        .audio
        .into_iter()
        .map(|item| FormatOption {
            id: item.id,
            label: item.label,
            sub: item.sub,
            size: item.size,
            best: Some(item.best),
            url: item.url,
            has_audio: true,
        })
        .collect();
    Ok(MediaInfo {
        platform: info.platform,
        title: info.title,
        author: info.author,
        author_full: info.author_full,
        duration: info.duration,
        views: info.views,
        likes: info.likes,
        thumb_hue: info.thumb_hue,
        thumb_hue2: info.thumb_hue2,
        thumbnail_url: info.thumbnail_url,
        original_url: info.original_url,
        formats: MediaFormats { video, audio },
    })
}

async fn analyze_instagram(url: &str, cookies: Option<String>) -> Result<MediaInfo, String> {
    let code = instagram_code(url)
        .ok_or_else(|| "Não foi possível identificar o código do Instagram.".to_string())?;
    let client = client()?;
    let variables = format!("{{\"shortcode\":\"{}\"}}", code);
    let mut request = client
        .get("https://www.instagram.com/graphql/query")
        .query(&[
            ("query_hash", "2c4c2e343a8f64c625ba02b2aa12c7f8"),
            ("variables", variables.as_str()),
        ])
        .header("accept", "application/json")
        .header("x-ig-app-id", "936619743392459")
        .header(
            "user-agent",
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15",
        );
    if let Some(cookies) = cookies.as_deref().filter(|value| !value.trim().is_empty()) {
        request = request.header("cookie", cookies.trim());
    }
    if let Ok(response) = request.send().await {
        if response.status().is_success() {
            if let Ok(value) = response.json::<Value>().await {
                if let Some(media) = value.pointer("/data/shortcode_media") {
                    if !media.is_null() {
                        return instagram_from_media(media, url);
                    }
                }
            }
        }
    }
    instagram_from_embed(&client, &code, url).await
}

fn instagram_from_media(media: &Value, url: &str) -> Result<MediaInfo, String> {
    let caption = media
        .pointer("/edge_media_to_caption/edges/0/node/text")
        .and_then(Value::as_str)
        .unwrap_or("Instagram")
        .chars()
        .take(180)
        .collect::<String>();
    let author = media
        .pointer("/owner/username")
        .and_then(Value::as_str)
        .map(|author| format!("@{author}"))
        .unwrap_or_else(|| "instagram".to_string());
    let author_full = media
        .pointer("/owner/full_name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let duration = media
        .get("video_duration")
        .and_then(Value::as_f64)
        .map(|value| format_duration(value.round() as u64))
        .unwrap_or_else(|| "—".to_string());
    let thumbnail_url = media
        .get("thumbnail_src")
        .or_else(|| media.get("display_url"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let likes = media
        .pointer("/edge_media_preview_like/count")
        .and_then(Value::as_u64)
        .map(format_count);
    let views = media
        .get("video_view_count")
        .and_then(Value::as_u64)
        .map(format_count);

    let mut video = Vec::new();
    if let Some(children) = media
        .pointer("/edge_sidecar_to_children/edges")
        .and_then(Value::as_array)
    {
        for (index, edge) in children.iter().enumerate() {
            let node = edge.get("node").unwrap_or(&Value::Null);
            if let Some(video_url) = node.get("video_url").and_then(Value::as_str) {
                video.push(instagram_video_option(
                    format!("instagram-video-{index}"),
                    "Vídeo do carrossel",
                    video_url,
                ));
            } else if let Some(image_url) = node.get("display_url").and_then(Value::as_str) {
                video.push(instagram_image_option(
                    format!("instagram-image-{index}"),
                    "Imagem do carrossel",
                    image_url,
                ));
            }
        }
    }
    if video.is_empty() {
        if let Some(video_url) = media.get("video_url").and_then(Value::as_str) {
            video.push(instagram_video_option(
                "instagram-video".to_string(),
                "Vídeo",
                video_url,
            ));
        } else if let Some(image_url) = media.get("display_url").and_then(Value::as_str) {
            video.push(instagram_image_option(
                "instagram-image".to_string(),
                "Imagem",
                image_url,
            ));
        }
    }
    if video.is_empty() {
        return Err("O Instagram não retornou mídia para este post.".to_string());
    }
    if let Some(first) = video.first_mut() {
        first.best = Some(true);
    }

    Ok(MediaInfo {
        platform: "instagram".to_string(),
        title: if caption.trim().is_empty() {
            "Instagram".to_string()
        } else {
            caption
        },
        author,
        author_full,
        duration,
        views,
        likes,
        thumb_hue: 330,
        thumb_hue2: 25,
        thumbnail_url,
        original_url: url.to_string(),
        formats: MediaFormats {
            video,
            audio: Vec::new(),
        },
    })
}

fn instagram_video_option(id: String, label: impl Into<String>, video_url: &str) -> FormatOption {
    let mut option = FormatOption::new(id, label, "MP4 · Instagram");
    option.url = Some(video_url.to_string());
    option.has_audio = true;
    option
}

fn instagram_image_option(id: String, label: impl Into<String>, image_url: &str) -> FormatOption {
    let mut option = FormatOption::new(id, label, "JPG · Instagram");
    option.url = Some(image_url.to_string());
    option
}

async fn instagram_from_embed(
    client: &reqwest::Client,
    code: &str,
    url: &str,
) -> Result<MediaInfo, String> {
    let html = client
        .get(format!(
            "https://www.instagram.com/p/{code}/embed/captioned/"
        ))
        .header(
            "user-agent",
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15",
        )
        .send()
        .await
        .map_err(|error| format!("Instagram embed indisponível: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Instagram embed HTTP: {error}"))?
        .text()
        .await
        .map_err(|error| format!("Falha ao ler o Instagram embed: {error}"))?;

    let normalized = html.replace("\\\"", "\"");
    let video_url = json_string_after(&normalized, "\"video_url\":\"");
    let image_url = json_string_after(&normalized, "\"display_url\":\"");
    let thumbnail =
        json_string_after(&normalized, "\"thumbnail_src\":\"").or_else(|| image_url.clone());
    let mut video = Vec::new();
    if let Some(video_url) = video_url {
        video.push(instagram_video_option(
            "instagram-video".to_string(),
            "Vídeo",
            &video_url,
        ));
    } else if let Some(image_url) = image_url {
        video.push(instagram_image_option(
            "instagram-image".to_string(),
            "Imagem",
            &image_url,
        ));
    }
    if video.is_empty() {
        return Err("O Instagram não retornou mídia para este post.".to_string());
    }
    if let Some(first) = video.first_mut() {
        first.best = Some(true);
    }
    let caption = json_string_after(
        &normalized,
        "\"edge_media_to_caption\":{\"edges\":[{\"node\":{\"text\":\"",
    )
    .filter(|caption| !caption.trim().is_empty());
    let title = caption
        .map(|caption| caption.chars().take(180).collect())
        .unwrap_or_else(|| "Instagram".to_string());
    let author = instagram_author_from_html(&normalized)
        .map(|author| format!("@{author}"))
        .unwrap_or_else(|| "instagram".to_string());

    Ok(MediaInfo {
        platform: "instagram".to_string(),
        title,
        author,
        author_full: None,
        duration: "—".to_string(),
        views: None,
        likes: None,
        thumb_hue: 330,
        thumb_hue2: 25,
        thumbnail_url: thumbnail,
        original_url: url.to_string(),
        formats: MediaFormats {
            video,
            audio: Vec::new(),
        },
    })
}

async fn analyze_reddit(url: &str) -> Result<MediaInfo, String> {
    let id = reddit_id(url)
        .ok_or_else(|| "Não foi possível identificar o post do Reddit.".to_string())?;
    let client = client()?;
    let value: Value = client
        .get(format!(
            "https://www.reddit.com/comments/{id}.json?raw_json=1"
        ))
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|error| format!("Reddit indisponível: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Reddit HTTP: {error}"))?
        .json()
        .await
        .map_err(|error| format!("Resposta inválida do Reddit: {error}"))?;
    let post = value
        .get(0)
        .and_then(|listing| listing.pointer("/data/children/0/data"))
        .ok_or_else(|| "O Reddit não retornou dados do post.".to_string())?;

    let title = post
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Reddit")
        .to_string();
    let author = post
        .get("author")
        .and_then(Value::as_str)
        .map(|author| format!("u/{author}"))
        .unwrap_or_else(|| "reddit".to_string());
    let subreddit = post
        .get("subreddit_name_prefixed")
        .and_then(Value::as_str)
        .unwrap_or("r/reddit")
        .to_string();
    let thumbnail_url = post
        .get("thumbnail")
        .and_then(Value::as_str)
        .filter(|thumbnail| thumbnail.starts_with("http"))
        .map(|thumbnail| thumbnail.replace("&amp;", "&"));
    let likes = post
        .get("score")
        .and_then(Value::as_i64)
        .map(|score| format_count(score.max(0) as u64));
    let duration = post
        .pointer("/media/reddit_video/duration")
        .or_else(|| post.pointer("/secure_media/reddit_video/duration"))
        .and_then(Value::as_u64)
        .map(format_duration)
        .unwrap_or_else(|| "—".to_string());

    let mut video = Vec::new();
    let fallback_url = post
        .pointer("/media/reddit_video/fallback_url")
        .or_else(|| post.pointer("/secure_media/reddit_video/fallback_url"))
        .and_then(Value::as_str);
    if let Some(fallback_url) = fallback_url {
        let clean_url = fallback_url.split('?').next().unwrap_or(fallback_url);
        let mut option = FormatOption::new("reddit-video", "Vídeo", "MP4 · Reddit");
        option.url = Some(clean_url.to_string());
        option.best = Some(true);
        video.push(option);
        if let Some(audio_url) = reddit_audio_url(clean_url) {
            let mut audio = FormatOption::new("reddit-audio", "Áudio", "MP4 · Reddit");
            audio.url = Some(audio_url);
            audio.has_audio = true;
            return Ok(reddit_media_info(
                title,
                author,
                subreddit,
                duration,
                likes,
                thumbnail_url,
                url,
                video,
                vec![audio],
            ));
        }
    }
    if video.is_empty() {
        let post_url = post.get("url").and_then(Value::as_str).unwrap_or(url);
        if post_url.starts_with("http") {
            let mut option = FormatOption::new("reddit-link", "Mídia do post", "Link · Reddit");
            option.url = Some(post_url.replace("&amp;", "&"));
            option.best = Some(true);
            if post_url.contains("i.redd.it")
                || post_url.contains(".jpg")
                || post_url.contains(".png")
                || post_url.contains(".gif")
            {
                option.sub = "Imagem · Reddit".to_string();
            }
            video.push(option);
        }
    }
    if video.is_empty() {
        return Err("O Reddit não retornou mídia para este post.".to_string());
    }
    Ok(reddit_media_info(
        title,
        author,
        subreddit,
        duration,
        likes,
        thumbnail_url,
        url,
        video,
        Vec::new(),
    ))
}

#[allow(clippy::too_many_arguments)]
fn reddit_media_info(
    title: String,
    author: String,
    subreddit: String,
    duration: String,
    likes: Option<String>,
    thumbnail_url: Option<String>,
    url: &str,
    video: Vec<FormatOption>,
    audio: Vec<FormatOption>,
) -> MediaInfo {
    MediaInfo {
        platform: "reddit".to_string(),
        title,
        author,
        author_full: Some(subreddit),
        duration,
        views: None,
        likes,
        thumb_hue: 20,
        thumb_hue2: 200,
        thumbnail_url,
        original_url: url.to_string(),
        formats: MediaFormats { video, audio },
    }
}

fn reddit_audio_url(video_url: &str) -> Option<String> {
    let base = video_url.rsplit_once('/')?.0;
    Some(format!("{base}/DASH_audio.mp4"))
}

fn instagram_code(url: &str) -> Option<String> {
    let re =
        Regex::new(r"(?:instagram\.com|instagr\.am)/(?:p|reel|reels|tv)/([A-Za-z0-9_-]+)").ok()?;
    re.captures(url)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
}

fn reddit_id(url: &str) -> Option<String> {
    let re = Regex::new(r"/comments/([A-Za-z0-9]+)").ok()?;
    if let Some(id) = re
        .captures(url)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
    {
        return Some(id);
    }
    let re = Regex::new(r"redd\.it/([A-Za-z0-9]+)").ok()?;
    re.captures(url)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
}

fn youtube_id(url: &str) -> Option<String> {
    if let Some(rest) = url.split("youtu.be/").nth(1) {
        let id = rest
            .split(['?', '&', '/'])
            .next()
            .filter(|id| id.len() == 11);
        if let Some(id) = id {
            return Some(id.to_string());
        }
    }
    if let Some(query) = url.split('?').nth(1) {
        for pair in query.split('&') {
            if let Some(value) = pair.strip_prefix("v=") {
                let id = value.split(['&', '#']).next().unwrap_or(value);
                if id.len() == 11 {
                    return Some(id.to_string());
                }
            }
        }
    }
    let re = Regex::new(
        r"(?:youtube\.com|youtube-nocookie\.com)/(?:shorts|embed|live)/([A-Za-z0-9_-]{11})",
    )
    .ok()?;
    re.captures(url)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36",
        )
        .build()
        .map_err(|error| error.to_string())
}

fn sanitize_filename(input: &str) -> String {
    let mut value = input
        .chars()
        .map(|ch| {
            if ch.is_control() || "/\\:*?\"<>|".contains(ch) {
                '_'
            } else {
                ch
            }
        })
        .collect::<String>();
    value = value.trim().trim_matches('.').trim().to_string();
    if value.is_empty() {
        value = "download".to_string();
    }
    value.chars().take(160).collect()
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
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

fn format_size(format: &Value) -> String {
    format
        .get("filesize")
        .or_else(|| format.get("filesize_approx"))
        .and_then(Value::as_u64)
        .or_else(|| {
            format
                .get("contentLength")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<u64>().ok())
        })
        .map(format_bytes)
        .unwrap_or_else(|| "—".to_string())
}

fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "—".to_string();
    }
    let value = bytes as f64;
    if value >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GB", value / (1024.0 * 1024.0 * 1024.0))
    } else if value >= 1024.0 * 1024.0 {
        format!("{:.1} MB", value / (1024.0 * 1024.0))
    } else {
        format!("{:.0} KB", value / 1024.0)
    }
}

fn json_object_end(input: &str) -> Option<usize> {
    let mut depth = 0_i32;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

fn json_string_after(html: &str, marker: &str) -> Option<String> {
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    let end = rest.find('"')?;
    Some(unescape_json_string(&rest[..end]))
}

fn unescape_json_string(value: &str) -> String {
    // Instagram embeds are JSON-in-JSON, so decode twice when possible. A
    // single decode also covers ordinary escaped URLs from other pages.
    let mut decoded = value.to_string();
    for _ in 0..2 {
        let quoted = format!("\"{decoded}\"");
        match serde_json::from_str::<String>(&quoted) {
            Ok(next) => decoded = next,
            Err(_) => break,
        }
    }
    decoded.replace("&amp;", "&")
}

fn instagram_author_from_html(html: &str) -> Option<String> {
    let marker = "\"username\":\"";
    json_string_after(html, marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_platforms_and_web() {
        assert_eq!(
            Platform::detect("https://x.com/user/status/1"),
            Platform::Twitter
        );
        assert_eq!(
            Platform::detect("https://youtu.be/dQw4w9WgXcQ"),
            Platform::YouTube
        );
        assert_eq!(
            Platform::detect("https://www.tiktok.com/@user/video/1"),
            Platform::TikTok
        );
        assert_eq!(
            Platform::detect("https://www.instagram.com/reel/abc/"),
            Platform::Instagram
        );
        assert_eq!(
            Platform::detect("https://www.reddit.com/r/rust/comments/abc/x/"),
            Platform::Reddit
        );
        assert_eq!(
            Platform::detect("https://cdn.example.com/video.mp4"),
            Platform::Web
        );
    }

    #[test]
    fn extracts_platform_ids() {
        assert_eq!(
            youtube_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            youtube_id("https://youtu.be/dQw4w9WgXcQ?t=1"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            instagram_code("https://www.instagram.com/reel/AbC-12/"),
            Some("AbC-12".to_string())
        );
        assert_eq!(
            reddit_id("https://www.reddit.com/r/rust/comments/abc123/title/"),
            Some("abc123".to_string())
        );
    }

    #[test]
    fn finds_the_end_of_an_embedded_json_object() {
        let input = r#"{"a":{"b":"} still string"},"c":1};</script>"#;
        let end = json_object_end(input).unwrap();
        assert!(input[..end].ends_with('}'));
        assert!(serde_json::from_str::<Value>(&input[..end]).is_ok());
    }

    #[test]
    fn decodes_escaped_instagram_urls() {
        assert_eq!(
            unescape_json_string(r"https:\\/\\/cdn.example.com\\/video.mp4?x=1\\u0026y=2"),
            "https://cdn.example.com/video.mp4?x=1&y=2"
        );
    }

    #[test]
    fn formats_names_and_counts() {
        assert_eq!(sanitize_filename("a/b:c?.mp4"), "a_b_c_.mp4");
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_count(1500), "1.5K");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    }
}
