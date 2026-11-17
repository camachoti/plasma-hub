use super::{
    client, format_count, format_duration, json_string_after, FormatOption, MediaFormats, MediaInfo,
};
use regex::Regex;
use serde_json::Value;

pub(super) async fn analyze_instagram(
    url: &str,
    cookies: Option<String>,
) -> Result<MediaInfo, String> {
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

pub(super) fn instagram_from_media(media: &Value, url: &str) -> Result<MediaInfo, String> {
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

pub(super) fn instagram_video_option(
    id: String,
    label: impl Into<String>,
    video_url: &str,
) -> FormatOption {
    let mut option = FormatOption::new(id, label, "MP4 · Instagram");
    option.url = Some(video_url.to_string());
    option.has_audio = true;
    option
}

pub(super) fn instagram_image_option(
    id: String,
    label: impl Into<String>,
    image_url: &str,
) -> FormatOption {
    let mut option = FormatOption::new(id, label, "JPG · Instagram");
    option.url = Some(image_url.to_string());
    option
}

pub(super) async fn instagram_from_embed(
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

pub(crate) fn instagram_code(url: &str) -> Option<String> {
    let re =
        Regex::new(r"(?:instagram\.com|instagr\.am)/(?:p|reel|reels|tv)/([A-Za-z0-9_-]+)").ok()?;
    re.captures(url)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
}

pub(super) fn instagram_author_from_html(html: &str) -> Option<String> {
    let marker = "\"username\":\"";
    json_string_after(html, marker)
}
