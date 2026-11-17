use super::{client, format_count, format_duration, FormatOption, MediaFormats, MediaInfo};
use regex::Regex;
use serde_json::Value;

pub(super) async fn analyze_reddit(url: &str) -> Result<MediaInfo, String> {
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
pub(super) fn reddit_media_info(
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

pub(super) fn reddit_audio_url(video_url: &str) -> Option<String> {
    let base = video_url.rsplit_once('/')?.0;
    Some(format!("{base}/DASH_audio.mp4"))
}

pub(crate) fn reddit_id(url: &str) -> Option<String> {
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
