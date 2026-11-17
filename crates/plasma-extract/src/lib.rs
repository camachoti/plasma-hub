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

mod instagram;
mod reddit;
mod youtube;

use instagram::analyze_instagram;
use reddit::analyze_reddit;
use youtube::analyze_youtube;

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
            instagram::instagram_code("https://www.instagram.com/reel/AbC-12/"),
            Some("AbC-12".to_string())
        );
        assert_eq!(
            reddit::reddit_id("https://www.reddit.com/r/rust/comments/abc123/title/"),
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
