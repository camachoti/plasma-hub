use super::{
    client, format_count, format_duration, format_size, json_object_end, FormatOption,
    MediaFormats, MediaInfo,
};
use regex::Regex;
use serde_json::Value;

pub(super) async fn analyze_youtube(url: &str) -> Result<MediaInfo, String> {
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

pub(crate) fn youtube_id(url: &str) -> Option<String> {
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
