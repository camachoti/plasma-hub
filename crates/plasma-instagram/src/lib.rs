//! Instagram post extraction.
//!
//! GraphQL is tried first. When Instagram refuses that query, the public embed
//! page still carries the media URL. Cookies unlock private posts.

mod cookies;

pub use cookies::{load_instagram_cookies, save_instagram_cookies, INSTAGRAM_COOKIES_FILE};

use regex::Regex;
use serde_json::Value;

/// A post reduced to the fields the download screen needs.
#[derive(Clone, Debug, PartialEq)]
pub struct PostMedia {
    pub title: String,
    pub author: String,
    pub author_full: Option<String>,
    pub duration: String,
    pub views: Option<String>,
    pub likes: Option<String>,
    pub thumbnail_url: Option<String>,
    pub original_url: String,
    pub thumb_hue: u16,
    pub thumb_hue2: u16,
    pub video: Vec<PostFormat>,
    pub audio: Vec<PostFormat>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PostFormat {
    pub id: String,
    pub label: String,
    pub sub: String,
    pub size: String,
    pub best: bool,
    pub url: Option<String>,
    pub has_audio: bool,
}

pub async fn analyze(url: &str, cookies: Option<String>) -> Result<PostMedia, String> {
    let code = post_code(url)
        .ok_or_else(|| "Não foi possível identificar o código do Instagram.".to_string())?;
    let client = client()?;
    let variables = format!("{{\"shortcode\":\"{code}\"}}");
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
                        return from_media(media, url);
                    }
                }
            }
        }
    }
    from_embed(&client, &code, url, cookies.as_deref()).await
}

pub fn post_code(url: &str) -> Option<String> {
    let re = Regex::new(
        r"(?:instagram\.com|instagr\.am)/(?:share/)?(?:p|reel|reels|tv)/([A-Za-z0-9_-]+)",
    )
    .ok()?;
    re.captures(url)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
}

fn from_media(media: &Value, url: &str) -> Result<PostMedia, String> {
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
                video.push(video_option(
                    format!("instagram-video-{index}"),
                    "Vídeo do carrossel",
                    video_url,
                ));
            } else if let Some(image_url) = node.get("display_url").and_then(Value::as_str) {
                video.push(image_option(
                    format!("instagram-image-{index}"),
                    "Imagem do carrossel",
                    image_url,
                ));
            }
        }
    }
    if video.is_empty() {
        if let Some(video_url) = media.get("video_url").and_then(Value::as_str) {
            video.push(video_option(
                "instagram-video".to_string(),
                "Vídeo",
                video_url,
            ));
        } else if let Some(image_url) = media.get("display_url").and_then(Value::as_str) {
            video.push(image_option(
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
        first.best = true;
    }
    Ok(PostMedia {
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
        thumbnail_url,
        original_url: url.to_string(),
        thumb_hue: 330,
        thumb_hue2: 25,
        video,
        audio: Vec::new(),
    })
}

fn video_option(id: String, label: impl Into<String>, video_url: &str) -> PostFormat {
    PostFormat {
        id,
        label: label.into(),
        sub: "MP4 · Instagram".to_string(),
        size: "—".to_string(),
        best: false,
        url: Some(video_url.to_string()),
        has_audio: true,
    }
}

fn image_option(id: String, label: impl Into<String>, image_url: &str) -> PostFormat {
    PostFormat {
        id,
        label: label.into(),
        sub: "JPG · Instagram".to_string(),
        size: "—".to_string(),
        best: false,
        url: Some(image_url.to_string()),
        has_audio: false,
    }
}

async fn from_embed(
    client: &reqwest::Client,
    code: &str,
    url: &str,
    cookies: Option<&str>,
) -> Result<PostMedia, String> {
    let mut request = client
        .get(format!(
            "https://www.instagram.com/p/{code}/embed/captioned/"
        ))
        .header(
            "user-agent",
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15",
        );
    if let Some(cookies) = cookies.map(str::trim).filter(|value| !value.is_empty()) {
        request = request.header("cookie", cookies);
    }
    let html = request
        .send()
        .await
        .map_err(|error| format!("Instagram embed indisponível: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Instagram embed HTTP: {error}"))?
        .text()
        .await
        .map_err(|error| format!("Falha ao ler o Instagram embed: {error}"))?;

    let normalized = html.replace("\\\"", "\"");
    let mut video = formats_from_embed_html(&normalized);
    let thumbnail = json_string_after(&normalized, "\"thumbnail_src\":\"")
        .or_else(|| video.iter().find_map(|format| format.url.clone()));
    if video.is_empty() {
        return Err("O Instagram não retornou mídia para este post.".to_string());
    }
    if let Some(first) = video.first_mut() {
        first.best = true;
    }
    let caption = json_string_after(
        &normalized,
        "\"edge_media_to_caption\":{\"edges\":[{\"node\":{\"text\":\"",
    )
    .filter(|caption| !caption.trim().is_empty());
    let title = caption
        .map(|caption| caption.chars().take(180).collect())
        .unwrap_or_else(|| "Instagram".to_string());
    let author = json_string_after(&normalized, "\"username\":\"")
        .map(|author| format!("@{author}"))
        .unwrap_or_else(|| "instagram".to_string());

    Ok(PostMedia {
        title,
        author,
        author_full: None,
        duration: "—".to_string(),
        views: None,
        likes: None,
        thumbnail_url: thumbnail,
        original_url: url.to_string(),
        thumb_hue: 330,
        thumb_hue2: 25,
        video,
        audio: Vec::new(),
    })
}

/// One public reel from a profile page.
#[derive(Clone, Debug, PartialEq)]
pub struct ListedReel {
    pub shortcode: String,
    pub title: String,
    pub author: String,
    pub page_url: String,
    pub video_url: Option<String>,
    pub thumbnail_url: Option<String>,
}

/// Keeps Instagram usernames that can be requested without a path.
pub fn normalize_usernames(names: &[String]) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for name in names {
        let name = username_from_input(name);
        if name.is_empty() {
            continue;
        }
        let valid = name.len() <= 30
            && name.chars().all(|character| {
                character.is_ascii_alphanumeric() || character == '.' || character == '_'
            });
        if !valid {
            return Err(format!("Perfil inválido: {name}"));
        }
        if normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&name))
        {
            continue;
        }
        normalized.push(name);
    }
    if normalized.is_empty() {
        return Err("Informe um perfil do Instagram.".to_string());
    }
    Ok(normalized)
}

/// Reads reels and videos from a `web_profile_info` payload.
///
/// A private profile, or a payload without a user, yields an empty list.
pub fn reels_from_profile(value: &Value) -> Vec<ListedReel> {
    let Some(user) = value.pointer("/data/user").filter(|user| !user.is_null()) else {
        return Vec::new();
    };
    if user
        .get("is_private")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Vec::new();
    }
    let author = user
        .get("username")
        .and_then(Value::as_str)
        .unwrap_or("instagram");
    let mut seen = Vec::new();
    let mut reels = Vec::new();
    for pointer in [
        "/edge_felix_video_timeline/edges",
        "/edge_owner_to_timeline_media/edges",
    ] {
        let Some(edges) = user.pointer(pointer).and_then(Value::as_array) else {
            continue;
        };
        for edge in edges {
            let Some(node) = edge.get("node") else {
                continue;
            };
            if node.get("__typename").and_then(Value::as_str) == Some("GraphImage") {
                continue;
            }
            let is_video = node
                .get("is_video")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let is_clip = node.get("product_type").and_then(Value::as_str) == Some("clips");
            if !is_video && !is_clip {
                continue;
            }
            let Some(shortcode) = node.get("shortcode").and_then(Value::as_str) else {
                continue;
            };
            if seen.iter().any(|seen: &String| seen == shortcode) {
                continue;
            }
            seen.push(shortcode.to_string());
            let title = node
                .pointer("/edge_media_to_caption/edges/0/node/text")
                .and_then(Value::as_str)
                .map(|caption| caption.chars().take(140).collect::<String>())
                .filter(|caption| !caption.trim().is_empty())
                .unwrap_or_else(|| "Instagram".to_string());
            let video_url = node
                .get("video_url")
                .and_then(Value::as_str)
                .filter(|url| url.starts_with("https://"))
                .map(str::to_string);
            let thumbnail_url = node
                .get("display_url")
                .or_else(|| node.get("thumbnail_src"))
                .and_then(Value::as_str)
                .filter(|url| url.starts_with("https://"))
                .map(str::to_string);
            reels.push(ListedReel {
                shortcode: shortcode.to_string(),
                title,
                author: format!("@{author}"),
                page_url: format!("https://www.instagram.com/reel/{shortcode}/"),
                video_url,
                thumbnail_url,
            });
        }
    }
    reels
}

/// Loads the first public page of reels for one profile.
///
/// To avoid triggering Instagram's anonymous rate limits when not logged in,
/// this performs a single profile request (`web_profile_info`, falling back to
/// the public `/embed/` profile page) and leaves missing `video_url` fields as
/// `None` so individual clips are resolved lazily only when scrolled into view.
pub async fn list_profile_reels(
    username: &str,
    cookies: Option<String>,
) -> Result<Vec<ListedReel>, String> {
    let username = normalize_usernames(&[username.to_string()])?
        .into_iter()
        .next()
        .ok_or_else(|| "Informe um perfil do Instagram.".to_string())?;
    let client = client()?;
    let mut request = client
        .get("https://www.instagram.com/api/v1/users/web_profile_info/")
        .query(&[("username", username.as_str())])
        .header("accept", "*/*")
        .header("x-ig-app-id", "936619743392459")
        .header("x-requested-with", "XMLHttpRequest")
        .header("referer", format!("https://www.instagram.com/{username}/"))
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/124.0.0.0 Safari/537.36",
        );
    if let Some(cookies) = cookies
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        request = request.header("cookie", cookies);
    }
    if let Ok(response) = request.send().await {
        if response.status().is_success() {
            if let Ok(value) = response.json::<Value>().await {
                let mut reels = reels_from_profile(&value);
                reels.truncate(10);
                if !reels.is_empty() {
                    return Ok(reels);
                }
            }
        }
    }

    list_profile_reels_from_embed(&client, &username, cookies.as_deref()).await
}

async fn list_profile_reels_from_embed(
    client: &reqwest::Client,
    username: &str,
    cookies: Option<&str>,
) -> Result<Vec<ListedReel>, String> {
    let mut request = client
        .get(format!("https://www.instagram.com/{username}/embed/"))
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36",
        );
    if let Some(cookies) = cookies.map(str::trim).filter(|value| !value.is_empty()) {
        request = request.header("cookie", cookies);
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("Instagram embed indisponível: {error}"))?;
    if matches!(response.status().as_u16(), 401 | 403 | 404 | 429) {
        return Ok(Vec::new());
    }
    let html = response
        .error_for_status()
        .map_err(|error| format!("Instagram embed HTTP: {error}"))?
        .text()
        .await
        .map_err(|error| format!("Falha ao ler perfil embed do Instagram: {error}"))?;
    Ok(reels_from_profile_embed(&html, username))
}

pub fn reels_from_profile_embed(html: &str, username: &str) -> Vec<ListedReel> {
    let normalized = html
        .replace("\\\\/", "/")
        .replace("\\/", "/")
        .replace("\\\\\"", "\"")
        .replace("\\\"", "\"")
        .replace("\\\"", "\"");
    let author = format!("@{}", username.trim_start_matches('@'));
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();

    if let Ok(re) = Regex::new(r#""shortcode"\s*:\s*"([A-Za-z0-9_-]{5,20})""#) {
        for captures in re.captures_iter(&normalized) {
            let Some(m) = captures.get(1) else {
                continue;
            };
            let code = m.as_str();
            let window_start = m.start().saturating_sub(240);
            let window_end = (m.end() + 480).min(normalized.len());
            let around = &normalized[window_start..window_end];
            if around.contains("\"is_video\":false") && !around.contains("\"is_video\":true") {
                continue;
            }
            if !seen.insert(code.to_string()) {
                continue;
            }
            let video_url =
                json_string_after(around, "\"video_url\":\"").filter(|u| u.starts_with("https://"));
            let thumbnail_url = json_string_after(around, "\"thumbnail_src\":\"")
                .or_else(|| json_string_after(around, "\"display_url\":\""))
                .filter(|u| u.starts_with("https://"));
            out.push(ListedReel {
                shortcode: code.to_string(),
                title: author.clone(),
                author: author.clone(),
                page_url: format!("https://www.instagram.com/reel/{code}/"),
                video_url,
                thumbnail_url,
            });
            if out.len() >= 10 {
                return out;
            }
        }
    }

    if let Ok(re) =
        Regex::new(r#"(?:instagram\.com|^|["'])/(?:reel|reels|p)/([A-Za-z0-9_-]{5,20})"#)
    {
        for captures in re.captures_iter(&normalized) {
            let Some(code) = captures.get(1).map(|m| m.as_str()) else {
                continue;
            };
            if !seen.insert(code.to_string()) {
                continue;
            }
            out.push(ListedReel {
                shortcode: code.to_string(),
                title: author.clone(),
                author: author.clone(),
                page_url: format!("https://www.instagram.com/reel/{code}/"),
                video_url: None,
                thumbnail_url: None,
            });
            if out.len() >= 10 {
                break;
            }
        }
    }

    out
}

fn username_from_input(value: &str) -> String {
    let value = value.trim().trim_start_matches('@');
    let value = value
        .trim_start_matches("https://")
        .trim_start_matches("http://");
    let value = value
        .trim_start_matches("www.instagram.com/")
        .trim_start_matches("instagram.com/");
    value
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36",
        )
        .build()
        .map_err(|error| error.to_string())
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

fn formats_from_embed_html(html: &str) -> Vec<PostFormat> {
    let mut video = Vec::new();
    for (index, url) in json_strings(html, "\"video_url\":\"")
        .into_iter()
        .enumerate()
    {
        video.push(video_option(
            format!("instagram-video-{index}"),
            if index == 0 {
                "Vídeo"
            } else {
                "Vídeo do carrossel"
            },
            &url,
        ));
    }
    if video.is_empty() {
        for (index, url) in json_strings(html, "\"display_url\":\"")
            .into_iter()
            .enumerate()
        {
            video.push(image_option(
                format!("instagram-image-{index}"),
                if index == 0 {
                    "Imagem"
                } else {
                    "Imagem do carrossel"
                },
                &url,
            ));
        }
    }
    video
}

fn json_strings(html: &str, marker: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(marker) {
        let after = &rest[start + marker.len()..];
        let Some(end) = after.find('"') else {
            break;
        };
        let value = unescape_json_string(&after[..end]);
        if !value.is_empty() && !values.contains(&value) {
            values.push(value);
        }
        rest = &after[end..];
    }
    values
}

fn json_string_after(html: &str, marker: &str) -> Option<String> {
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    let end = rest.find('"')?;
    Some(unescape_json_string(&rest[..end]))
}

fn unescape_json_string(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_reel_code() {
        assert_eq!(
            post_code("https://www.instagram.com/reel/AbC-12/"),
            Some("AbC-12".to_string())
        );
        assert_eq!(
            post_code("https://www.instagram.com/share/reel/Zy9/"),
            Some("Zy9".to_string())
        );
    }

    #[test]
    fn embed_keeps_every_carousel_video() {
        let html =
            r#"{"video_url":"https://cdn.example/a.mp4","video_url":"https://cdn.example/b.mp4"}"#;
        let formats = formats_from_embed_html(html);
        assert_eq!(formats.len(), 2);
        assert_eq!(formats[1].url.as_deref(), Some("https://cdn.example/b.mp4"));
    }

    #[test]
    fn profile_keeps_reels_and_skips_photos_and_private_accounts() {
        let profile = serde_json::json!({
            "data": {
                "user": {
                    "username": "ada",
                    "is_private": false,
                    "edge_felix_video_timeline": {
                        "edges": [{
                            "node": {
                                "shortcode": "Reel1",
                                "is_video": true,
                                "product_type": "clips",
                                "video_url": "https://cdninstagram.example/reel.mp4",
                                "display_url": "https://cdninstagram.example/reel.jpg",
                                "edge_media_to_caption": {
                                    "edges": [{ "node": { "text": "Um reel" } }]
                                }
                            }
                        }]
                    },
                    "edge_owner_to_timeline_media": {
                        "edges": [
                            {
                                "node": {
                                    "shortcode": "Reel1",
                                    "is_video": true,
                                    "video_url": "https://cdninstagram.example/dup.mp4"
                                }
                            },
                            {
                                "node": {
                                    "shortcode": "Vid2",
                                    "is_video": true,
                                    "display_url": "https://cdninstagram.example/vid.jpg"
                                }
                            },
                            {
                                "node": {
                                    "__typename": "GraphImage",
                                    "shortcode": "Pic1",
                                    "is_video": false,
                                    "display_url": "https://cdninstagram.example/pic.jpg"
                                }
                            }
                        ]
                    }
                }
            }
        });
        let reels = reels_from_profile(&profile);
        assert_eq!(reels.len(), 2);
        assert_eq!(reels[0].shortcode, "Reel1");
        assert_eq!(reels[0].author, "@ada");
        assert_eq!(
            reels[0].video_url.as_deref(),
            Some("https://cdninstagram.example/reel.mp4")
        );
        assert_eq!(reels[1].shortcode, "Vid2");
        assert!(reels[1].video_url.is_none());
        let private = serde_json::json!({
            "data": { "user": { "username": "ada", "is_private": true } }
        });
        assert!(reels_from_profile(&private).is_empty());
        assert_eq!(
            normalize_usernames(&["@ada".to_string(), "https://instagram.com/Ada/".to_string()])
                .unwrap(),
            vec!["ada".to_string()]
        );
    }

    #[test]
    fn decodes_escaped_media_urls() {
        assert_eq!(
            unescape_json_string(r"https:\\/\\/cdn.example.com\\/video.mp4?x=1\\u0026y=2"),
            "https://cdn.example.com/video.mp4?x=1&y=2"
        );
    }

    #[test]
    fn extracts_reels_from_public_profile_embed() {
        let html = r#"<a href="https://www.instagram.com/reel/C9AbCdEf123/">Reel</a>{"shortcode":"C9XyZ_987"}"#;
        let reels = reels_from_profile_embed(html, "natgeo");
        assert_eq!(reels.len(), 2);
        assert_eq!(reels[0].shortcode, "C9XyZ_987");
        assert_eq!(reels[0].author, "@natgeo");
        assert_eq!(reels[1].shortcode, "C9AbCdEf123");
    }
}
