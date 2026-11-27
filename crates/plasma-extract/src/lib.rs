//! Unified native media extraction for the Rust shell.
//!
//! The React downloader has one extractor per platform. This crate provides the
//! same contract to the Rust UI: a [`MediaInfo`] with video/audio [`FormatOption`]
//! lists, plus a download dispatcher that reuses the existing native downloaders
//! for Twitter, YouTube and TikTok and the shared HTTPS downloader for direct
//! Instagram/Reddit/web media.

use plasma_core::AppEnv;
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
        let trimmed = url.trim();
        // Accept both full URLs and bare hosts pasted from share sheets. Parsing
        // also normalizes ports, trailing dots, userinfo and percent-encoding,
        // which the previous string split mishandled.
        let parsed = reqwest::Url::parse(trimmed)
            .or_else(|_| reqwest::Url::parse(&format!("https://{trimmed}")));
        let Ok(parsed) = parsed else {
            return Self::Web;
        };
        let Some(host) = parsed.host_str() else {
            return Self::Web;
        };
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        let host = host.strip_prefix("www.").unwrap_or(&host);
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

/// One video ready for the vertical player.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReelClip {
    pub platform: String,
    pub id: String,
    pub title: String,
    pub author: String,
    pub page_url: String,
    pub media_url: String,
    pub thumbnail_url: Option<String>,
    pub format_id: String,
    pub has_audio: bool,
}

/// A page of reel clips and, for Reddit, the cursor of the next page.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReelPage {
    pub items: Vec<ReelClip>,
    pub after: Option<String>,
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

pub use plasma_reddit::{
    default_reel_interests, subreddits_for_interests, ReelInterest, REEL_INTERESTS,
};

pub fn normalize_reels_subreddits(names: &[String]) -> Result<Vec<String>, String> {
    plasma_reddit::normalize_subreddits(names)
}

pub fn normalize_reels_instagram(names: &[String]) -> Result<Vec<String>, String> {
    plasma_instagram::normalize_usernames(names)
}

pub fn normalize_reels_twitter(names: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for raw in names {
        let trimmed = raw
            .trim()
            .trim_start_matches('@')
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_start_matches("www.")
            .trim_start_matches("x.com/")
            .trim_start_matches("twitter.com/");
        let handle = trimmed.split(['/', '?', '#']).next().unwrap_or("").trim();
        if handle.is_empty() {
            continue;
        }
        let valid = handle.len() <= 30
            && handle
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !valid {
            return Err(format!("Perfil do X/Twitter inválido: {handle}"));
        }
        if !out
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(handle))
        {
            out.push(handle.to_string());
        }
    }
    Ok(out)
}

pub fn normalize_reels_youtube(names: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for raw in names {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        if item.len() > 120 {
            return Err("Fonte do YouTube muito longa.".to_string());
        }
        if !out
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(item))
        {
            out.push(item.to_string());
        }
    }
    Ok(out)
}

pub fn normalize_reels_tiktok(names: &[String]) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for raw in names {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        if item.len() > 120 {
            return Err("Fonte do TikTok muito longa.".to_string());
        }
        if !out
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(item))
        {
            out.push(item.to_string());
        }
    }
    Ok(out)
}

/// Public TikTok videos from `@profiles` or interest topics via TikWM.
pub async fn list_tiktok_reels(
    sources: &[String],
    interests: &[String],
) -> Result<Vec<ReelClip>, String> {
    let videos = plasma_tiktok::list_tiktok_videos(sources, interests).await?;
    Ok(videos
        .into_iter()
        .map(|v| ReelClip {
            platform: "tiktok".to_string(),
            id: v.id,
            title: v.title,
            author: v.author,
            page_url: v.page_url,
            media_url: v.media_url,
            thumbnail_url: v.thumbnail_url,
            format_id: "tikwm-video".to_string(),
            has_audio: true,
        })
        .collect())
}

/// Public videos from the chosen communities. `after` continues the same listing.
pub async fn list_reddit_reels(
    subreddits: &[String],
    after: Option<&str>,
) -> Result<ReelPage, String> {
    let page = plasma_reddit::list_videos(subreddits, after).await?;
    let items = page
        .items
        .into_iter()
        .map(|video| ReelClip {
            platform: "reddit".to_string(),
            id: video.id,
            title: video.title,
            author: video.author,
            page_url: video.page_url,
            media_url: video.media_url,
            thumbnail_url: video.thumbnail_url,
            format_id: "reddit-video".to_string(),
            has_audio: true,
        })
        .collect();
    Ok(ReelPage {
        items,
        after: page.after,
    })
}

/// Public X / Twitter accounts mapped to the chosen interests when the user has not set custom handles.
pub fn default_twitter_for_interests(ids: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for id in ids {
        let handles: &[&str] = match id.as_str() {
            "humor" => &["NoContextHumans"],
            "jogos" => &["IGN"],
            "esportes" => &["espn"],
            "animais" => &["buitengebieden"],
            "ciencia" => &["NASA"],
            "comida" => &["Tasty"],
            _ => &["Rainmaker1973"],
        };
        for handle in handles {
            if !out
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(handle))
            {
                out.push((*handle).to_string());
            }
            if out.len() >= 3 {
                return out;
            }
        }
    }
    if out.is_empty() {
        out.push("Rainmaker1973".to_string());
    }
    out
}

/// Public Instagram accounts mapped to the chosen interests when the user has not set custom handles.
pub fn default_instagram_for_interests(ids: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for id in ids {
        let handles: &[&str] = match id.as_str() {
            "humor" => &["9gag"],
            "jogos" => &["ign"],
            "esportes" => &["espn"],
            "animais" => &["thedodo"],
            "ciencia" => &["nasa"],
            "comida" => &["buzzfeedtasty"],
            _ => &["natgeo"],
        };
        for handle in handles {
            if !out
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(handle))
            {
                out.push((*handle).to_string());
            }
            if out.len() >= 2 {
                return out;
            }
        }
    }
    if out.is_empty() {
        out.push("natgeo".to_string());
    }
    out
}

/// First public page of reels for one profile. Cookies are optional; missing direct URLs resolve lazily.
pub async fn list_instagram_reels(
    username: &str,
    cookies: Option<String>,
) -> Result<Vec<ReelClip>, String> {
    let reels = plasma_instagram::list_profile_reels(username, cookies).await?;
    Ok(reels
        .into_iter()
        .map(|reel| ReelClip {
            platform: "instagram".to_string(),
            id: reel.shortcode,
            title: reel.title,
            author: reel.author,
            page_url: reel.page_url,
            media_url: reel.video_url.unwrap_or_default(),
            thumbnail_url: reel.thumbnail_url,
            format_id: "instagram-video".to_string(),
            has_audio: true,
        })
        .collect())
}

/// First public page of videos for one X / Twitter profile (uses public syndication when not logged in).
pub async fn list_twitter_reels(
    username: &str,
    cookies: Option<String>,
) -> Result<Vec<ReelClip>, String> {
    let normalized = normalize_reels_twitter(&[username.to_string()])?;
    let Some(handle) = normalized.into_iter().next() else {
        return Ok(Vec::new());
    };
    let videos = plasma_twitter::list_public_profile_videos(&handle, cookies).await?;
    Ok(videos
        .into_iter()
        .map(|video| ReelClip {
            platform: "twitter".to_string(),
            id: video.id,
            title: video.title,
            author: video.author,
            page_url: video.page_url,
            media_url: video.video_url,
            thumbnail_url: video.thumbnail_url,
            format_id: "twitter-video".to_string(),
            has_audio: true,
        })
        .collect())
}

/// Public YouTube Shorts from channels or interest topics.
pub async fn list_youtube_reels(
    sources: &[String],
    interests: &[String],
) -> Result<Vec<ReelClip>, String> {
    youtube::list_youtube_shorts(sources, interests).await
}

/// Resolved direct stream URLs for a reel (single muxed MP4 or paired video + audio).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedReelStream {
    pub video_url: String,
    pub audio_url: Option<String>,
    pub author: Option<String>,
    pub title: Option<String>,
}

/// Resolves playable HTTPS stream URL(s) for a clip when `media_url` was deferred (YouTube / Instagram).
pub async fn resolve_reel_stream(
    env: &AppEnv,
    clip: &ReelClip,
    cookies: Option<String>,
) -> Result<ResolvedReelStream, String> {
    if clip.media_url.starts_with("https://") {
        return Ok(ResolvedReelStream {
            video_url: clip.media_url.clone(),
            audio_url: None,
            author: None,
            title: None,
        });
    }
    match clip.platform.as_str() {
        "youtube" => youtube::resolve_youtube_reel_stream(env, &clip.page_url).await,
        "instagram" => {
            let media = plasma_instagram::analyze(&clip.page_url, cookies).await?;
            let author = (!media.author.trim().is_empty()).then_some(media.author);
            let title = (!media.title.trim().is_empty()).then_some(media.title);
            let video_url = media
                .video
                .into_iter()
                .find_map(|format| {
                    let url = format.url?;
                    (format.has_audio && url.starts_with("https://")).then_some(url)
                })
                .ok_or_else(|| "Este post do Instagram não entregou vídeo público.".to_string())?;
            Ok(ResolvedReelStream {
                video_url,
                audio_url: None,
                author,
                title,
            })
        }
        _ => Err("Este reel não possui URL direta de vídeo.".to_string()),
    }
}

/// Resolves a playable HTTPS stream URL for a clip when `media_url` was deferred (YouTube / Instagram).
pub async fn resolve_reel_media_url(
    env: &AppEnv,
    clip: &ReelClip,
    cookies: Option<String>,
) -> Result<String, String> {
    resolve_reel_stream(env, clip, cookies)
        .await
        .map(|s| s.video_url)
}

/// Analyzes any supported link and resolves a playable `ReelClip` (including ciphered YouTube links).
pub async fn resolve_clip_from_url(
    env: &AppEnv,
    url: &str,
    cookies: Option<String>,
) -> Result<ReelClip, String> {
    let info = analyze_url(env, url, cookies).await?;
    if let Some(clip) = clip_from_analysis(&info) {
        return Ok(clip);
    }
    if info.platform == "youtube" {
        let stream_url = youtube::resolve_youtube_reel_url(env, &info.original_url).await?;
        let format_id = info
            .formats
            .video
            .first()
            .map(|opt| opt.id.clone())
            .unwrap_or_else(|| "best".to_string());
        return Ok(ReelClip {
            platform: "youtube".to_string(),
            id: format!("youtube:{}", info.original_url),
            title: info.title,
            author: info.author,
            page_url: info.original_url,
            media_url: stream_url,
            thumbnail_url: info.thumbnail_url,
            format_id,
            has_audio: true,
        });
    }
    Err("Esse link não tem um vídeo direto para o player.".to_string())
}

/// The first direct video in an analysis, ready to drop into the reel queue.
pub fn clip_from_analysis(info: &MediaInfo) -> Option<ReelClip> {
    let option = info
        .formats
        .video
        .iter()
        .find(|option| option.has_audio && playable_video(option))
        .or_else(|| {
            info.formats
                .video
                .iter()
                .find(|option| playable_video(option))
        })?;
    Some(ReelClip {
        platform: info.platform.clone(),
        id: format!("{}:{}", info.platform, info.original_url),
        title: info.title.clone(),
        author: info.author.clone(),
        page_url: info.original_url.clone(),
        media_url: option.url.clone()?,
        thumbnail_url: info.thumbnail_url.clone(),
        format_id: option.id.clone(),
        has_audio: option.has_audio,
    })
}

fn playable_video(option: &FormatOption) -> bool {
    let Some(url) = option.url.as_deref() else {
        return false;
    };
    if !url.starts_with("https://") {
        return false;
    }
    let id = option.id.to_ascii_lowercase();
    if id.contains("image") || id.contains("gallery") || id.contains("link") {
        return false;
    }
    option.has_audio || id.contains("video")
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
            // A saved media URL continues the same file. The watch page is
            // opened again only when that URL has expired.
            if has_audio {
                if let Some(direct_url) = direct_url.as_deref().filter(|url| !url.trim().is_empty())
                {
                    match download_direct(
                        env,
                        &id,
                        &original_url,
                        direct_url,
                        &file_name,
                        cookies.as_deref(),
                    )
                    .await
                    {
                        Ok(()) => return Ok(()),
                        Err(error) if direct_url_expired(&error) => {}
                        Err(error) => return Err(error),
                    }
                }
            }
            plasma_youtube::download_youtube_native(env, id, original_url, format_id, file_name)
                .await
        }
        Platform::TikTok => {
            #[cfg(target_os = "android")]
            {
                let direct_url = direct_url.ok_or_else(|| {
                    "O TikTok não retornou uma URL direta para este formato.".to_string()
                })?;
                download_direct(
                    env,
                    &id,
                    &original_url,
                    &direct_url,
                    &file_name,
                    cookies.as_deref(),
                )
                .await
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
        Platform::Reddit if format_id == "reddit-av" => {
            let direct_url = direct_url
                .ok_or_else(|| "Este formato não tem URL direta para download.".to_string())?;
            let directory = env
                .paths()
                .download_dir()
                .unwrap_or_else(|_| env.paths().data_dir().join("downloads"));
            let path = directory.join(sanitize_filename(&file_name));
            plasma_reddit::download_with_audio(env, &id, &direct_url, &path).await
        }
        Platform::Instagram | Platform::Reddit | Platform::Web => {
            let direct_url = direct_url
                .ok_or_else(|| "Este formato não tem URL direta para download.".to_string())?;
            download_direct(
                env,
                &id,
                &original_url,
                &direct_url,
                &file_name,
                cookies.as_deref(),
            )
            .await
        }
    }
}

async fn download_direct(
    env: &AppEnv,
    id: &str,
    original_url: &str,
    direct_url: &str,
    file_name: &str,
    cookies: Option<&str>,
) -> Result<(), String> {
    let directory = env
        .paths()
        .download_dir()
        .unwrap_or_else(|_| env.paths().data_dir().join("downloads"));
    let path = directory.join(sanitize_filename(file_name));
    let headers = request_headers(original_url, cookies);
    let identity = format!(
        "{}|{original_url}|{file_name}",
        Platform::detect(original_url).as_str()
    );
    plasma_download::download_url_to_file_with_identity(
        env,
        id,
        direct_url,
        &path.to_string_lossy(),
        Some(headers),
        Some(&identity),
    )
    .await
}

fn direct_url_expired(error: &str) -> bool {
    error.contains("403") || error.contains("401") || error.contains("410")
}

fn request_headers(
    original_url: &str,
    cookies: Option<&str>,
) -> std::collections::HashMap<String, String> {
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
            if let Some(cookies) = cookies.map(str::trim).filter(|value| !value.is_empty()) {
                headers.insert("cookie".to_string(), cookies.to_string());
            }
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

async fn analyze_instagram(url: &str, cookies: Option<String>) -> Result<MediaInfo, String> {
    let post = plasma_instagram::analyze(url, cookies).await?;
    Ok(MediaInfo {
        platform: "instagram".to_string(),
        title: post.title,
        author: post.author,
        author_full: post.author_full,
        duration: post.duration,
        views: post.views,
        likes: post.likes,
        thumb_hue: post.thumb_hue,
        thumb_hue2: post.thumb_hue2,
        thumbnail_url: post.thumbnail_url,
        original_url: post.original_url,
        formats: MediaFormats {
            video: post.video.into_iter().map(instagram_format).collect(),
            audio: post.audio.into_iter().map(instagram_format).collect(),
        },
    })
}

fn instagram_format(format: plasma_instagram::PostFormat) -> FormatOption {
    FormatOption {
        id: format.id,
        label: format.label,
        sub: format.sub,
        size: format.size,
        best: format.best.then_some(true),
        url: format.url,
        has_audio: format.has_audio,
    }
}

async fn analyze_reddit(url: &str) -> Result<MediaInfo, String> {
    let post = plasma_reddit::analyze(url).await?;
    Ok(MediaInfo {
        platform: "reddit".to_string(),
        title: post.title,
        author: post.author,
        author_full: post.author_full,
        duration: post.duration,
        views: post.views,
        likes: post.likes,
        thumb_hue: post.thumb_hue,
        thumb_hue2: post.thumb_hue2,
        thumbnail_url: post.thumbnail_url,
        original_url: post.original_url,
        formats: MediaFormats {
            video: post.video.into_iter().map(reddit_format).collect(),
            audio: post.audio.into_iter().map(reddit_format).collect(),
        },
    })
}

fn reddit_format(format: plasma_reddit::PostFormat) -> FormatOption {
    FormatOption {
        id: format.id,
        label: format.label,
        sub: format.sub,
        size: format.size,
        best: format.best.then_some(true),
        url: format.url,
        has_audio: format.has_audio,
    }
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

mod youtube;

use youtube::analyze_youtube;

pub use plasma_instagram::{load_instagram_cookies, save_instagram_cookies};

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
    fn detects_platforms_from_normalized_urls() {
        assert_eq!(
            Platform::detect("https://x.com:443/user/status/1"),
            Platform::Twitter
        );
        assert_eq!(
            Platform::detect("https://WWW.YouTube.com./watch?v=1"),
            Platform::YouTube
        );
        assert_eq!(
            Platform::detect("https://m.youtube.com/watch?v=1"),
            Platform::YouTube
        );
        assert_eq!(Platform::detect("youtu.be/dQw4w9WgXcQ"), Platform::YouTube);
        assert_eq!(
            Platform::detect("https://www.reddit.com:8443/r/rust"),
            Platform::Reddit
        );
        assert_eq!(
            Platform::detect("https://youtube.com.evil.test/watch"),
            Platform::Web
        );
        assert_eq!(
            Platform::detect("https://x.com@evil.test/status/1"),
            Platform::Web
        );
        assert_eq!(Platform::detect(""), Platform::Web);
        assert_eq!(Platform::detect("not a url"), Platform::Web);
    }

    #[test]
    fn extracts_platform_ids() {
        assert_eq!(
            youtube::youtube_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            youtube::youtube_id("https://youtu.be/dQw4w9WgXcQ?t=1"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            plasma_instagram::post_code("https://www.instagram.com/reel/AbC-12/"),
            Some("AbC-12".to_string())
        );
        assert_eq!(
            plasma_reddit::post_id("https://www.reddit.com/r/rust/comments/abc123/title/"),
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
    fn formats_names_and_counts() {
        assert_eq!(sanitize_filename("a/b:c?.mp4"), "a_b_c_.mp4");
        assert_eq!(format_duration(65), "1:05");
        assert_eq!(format_count(1500), "1.5K");
        assert_eq!(format_bytes(1024 * 1024), "1.0 MB");
    }

    #[test]
    fn normalizes_twitter_reels_handles() {
        let parsed = normalize_reels_twitter(&[
            "@nasa".to_string(),
            "https://x.com/IGN/media".to_string(),
            "NASA".to_string(),
        ])
        .unwrap();
        assert_eq!(parsed, vec!["nasa".to_string(), "IGN".to_string()]);
        assert!(normalize_reels_twitter(&["invalido com espaco".to_string()]).is_err());
    }

    #[test]
    fn parses_youtube_shorts_from_initial_data() {
        let html = r#"<script>var ytInitialData = {"metadata":{"channelMetadataRenderer":{"title":"Kurzgesagt"}},"contents":[{"shortsLockupViewModel":{"entityId":"shorts-shelf-item-dQw4w9WgXcQ","overlayMetadata":{"primaryText":{"content":"Black Holes Explained"}}}}]};</script>"#;
        let clips = youtube::parse_shorts_from_html(html, "YouTube");
        assert_eq!(clips.len(), 1);
        assert_eq!(clips[0].id, "dQw4w9WgXcQ");
        assert_eq!(clips[0].author, "Kurzgesagt");
        assert_eq!(clips[0].title, "Black Holes Explained");
        assert_eq!(
            clips[0].page_url,
            "https://www.youtube.com/shorts/dQw4w9WgXcQ"
        );
    }
}
