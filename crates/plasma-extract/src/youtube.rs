use super::{
    client, format_count, format_duration, format_size, json_object_end, FormatOption,
    MediaFormats, MediaInfo, ReelClip, ResolvedReelStream,
};
use plasma_core::AppEnv;
use regex::Regex;
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

fn cached_visitor_data() -> &'static Mutex<Option<String>> {
    static VISITOR: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    VISITOR.get_or_init(|| Mutex::new(None))
}

fn remember_visitor_data(html: &str) {
    if let Ok(re) = Regex::new(r#""visitorData"\s*:\s*"([A-Za-z0-9%_-]{8,160})""#) {
        if let Some(val) = re
            .captures(html)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
        {
            if let Ok(mut guard) = cached_visitor_data().lock() {
                *guard = Some(val);
            }
        }
    }
}

async fn ensure_visitor_data(http: &reqwest::Client) -> Option<String> {
    if let Ok(guard) = cached_visitor_data().lock() {
        if let Some(existing) = guard.clone() {
            return Some(existing);
        }
    }
    let resp = http
        .get("https://www.youtube.com/sw.js_data")
        .header(
            "user-agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36",
        )
        .header("cookie", YOUTUBE_ANON_CONSENT_COOKIE)
        .send()
        .await
        .ok()?;
    let text = resp.text().await.ok()?;
    let re = Regex::new(r#""(Cgt[A-Za-z0-9%_-]{8,140})""#).ok()?;
    let found = re
        .captures(&text)
        .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))?;
    if let Ok(mut guard) = cached_visitor_data().lock() {
        *guard = Some(found.clone());
    }
    Some(found)
}

const YOUTUBE_ANON_CONSENT_COOKIE: &str =
    "CONSENT=YES+cb; SOCS=CAISNQgDEitib3FfaWRlbnRpdHlmcm9udGVuZHVpc2VydmVyXzIwMjMwODI5LjA3X3AxGgJwdSADGgYIgJnPpwY";

pub(super) async fn analyze_youtube(url: &str) -> Result<MediaInfo, String> {
    let video_id = youtube_id(url)
        .ok_or_else(|| "Não foi possível identificar o ID do YouTube.".to_string())?;
    let client = client()?;
    let response = client
        .get(format!("https://www.youtube.com/watch?v={video_id}"))
        .header("accept-language", "pt-BR,pt;q=0.9,en;q=0.8")
        .header("cookie", YOUTUBE_ANON_CONSENT_COOKIE)
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

pub(super) async fn resolve_youtube_reel_stream(
    env: &AppEnv,
    page_url: &str,
) -> Result<ResolvedReelStream, String> {
    if let Some(video_id) = youtube_id(page_url) {
        if let Ok(stream) = resolve_innertube_stream(&video_id).await {
            return Ok(stream);
        }
    }
    if let Ok(info) = analyze_youtube(page_url).await {
        if let Some(url) = info
            .formats
            .video
            .iter()
            .filter(|option| option.has_audio)
            .find_map(|option| option.url.as_ref().filter(|u| u.starts_with("https://")))
        {
            return Ok(ResolvedReelStream {
                video_url: url.clone(),
                audio_url: None,
                author: (!info.author.trim().is_empty()).then_some(info.author),
                title: (!info.title.trim().is_empty()).then_some(info.title),
            });
        }
    }
    let resolved = plasma_youtube::get_youtube_stream_url(
        env,
        page_url.to_string(),
        "18/b[ext=mp4]/bv*[ext=mp4]+ba[ext=m4a]/b".to_string(),
    )
    .await?;
    let mut lines = resolved
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("https://"));
    let Some(video_url) = lines.next().map(str::to_string) else {
        return Err("O YouTube não retornou uma URL direta de reprodução.".to_string());
    };
    let audio_url = lines.next().map(str::to_string);
    Ok(ResolvedReelStream {
        video_url,
        audio_url,
        author: None,
        title: None,
    })
}

pub(super) async fn resolve_youtube_reel_url(
    env: &AppEnv,
    page_url: &str,
) -> Result<String, String> {
    resolve_youtube_reel_stream(env, page_url)
        .await
        .map(|s| s.video_url)
}

async fn resolve_innertube_stream(video_id: &str) -> Result<ResolvedReelStream, String> {
    struct InnertubeClient {
        version: &'static str,
        name_id: &'static str,
        user_agent: &'static str,
        context_body: serde_json::Value,
    }

    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(6))
        .build()
        .map_err(|e| e.to_string())?;

    let visitor_data = ensure_visitor_data(&http).await;

    let clients = [
        InnertubeClient {
            version: "1.62.27",
            name_id: "28",
            user_agent: "com.google.android.apps.youtube.vr.oculus/1.62.27 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
            context_body: serde_json::json!({
                "clientName": "ANDROID_VR",
                "clientVersion": "1.62.27",
                "deviceMake": "Oculus",
                "deviceModel": "Quest 3",
                "osName": "Android",
                "osVersion": "12L",
                "androidSdkVersion": 32,
                "hl": "pt",
                "gl": "BR"
            }),
        },
        InnertubeClient {
            version: "1.9",
            name_id: "30",
            user_agent: "com.google.android.youtube/1.9 (Linux; U; Android 11) gzip",
            context_body: serde_json::json!({
                "clientName": "ANDROID_TESTSUITE",
                "clientVersion": "1.9",
                "osName": "Android",
                "osVersion": "11",
                "androidSdkVersion": 30,
                "hl": "pt",
                "gl": "BR"
            }),
        },
        InnertubeClient {
            version: "20.10.4",
            name_id: "5",
            user_agent: "com.google.ios.youtube/20.10.4 (iPhone16,2; U; CPU iOS 18_3_2 like Mac OS X;)",
            context_body: serde_json::json!({
                "clientName": "IOS",
                "clientVersion": "20.10.4",
                "deviceMake": "Apple",
                "deviceModel": "iPhone16,2",
                "osName": "iPhone",
                "osVersion": "18.3.2.22D82",
                "hl": "pt",
                "gl": "BR"
            }),
        },
    ];

    for mut profile in clients {
        if let Some(ref vd) = visitor_data {
            if let Some(obj) = profile.context_body.as_object_mut() {
                obj.insert("visitorData".to_string(), Value::String(vd.clone()));
            }
        }
        let payload = serde_json::json!({
            "videoId": video_id,
            "contentCheckOk": true,
            "racyCheckOk": true,
            "context": {
                "client": profile.context_body
            }
        });
        let mut req = http
            .post("https://www.youtube.com/youtubei/v1/player?prettyPrint=false")
            .header("user-agent", profile.user_agent)
            .header("content-type", "application/json")
            .header("x-youtube-client-name", profile.name_id)
            .header("x-youtube-client-version", profile.version)
            .header("origin", "https://www.youtube.com");
        if let Some(ref vd) = visitor_data {
            req = req.header("x-goog-visitor-id", vd);
        }
        let Ok(resp) = req.json(&payload).send().await else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        let Ok(data) = resp.json::<Value>().await else {
            continue;
        };
        let (author, title) = extract_player_metadata(&data);
        if let Some(url) = pick_muxed_mp4_from_player(&data) {
            return Ok(ResolvedReelStream {
                video_url: url,
                audio_url: None,
                author,
                title,
            });
        }
        if let Some(mut adaptive) = pick_adaptive_av_from_player(&data) {
            adaptive.author = author;
            adaptive.title = title;
            return Ok(adaptive);
        }
    }

    Err("Innertube não entregou stream direto.".to_string())
}

fn extract_player_metadata(data: &Value) -> (Option<String>, Option<String>) {
    let details = data.get("videoDetails");
    let author = details
        .and_then(|d| d.get("author"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let title = details
        .and_then(|d| d.get("title"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    (author, title)
}

pub(crate) fn pick_muxed_mp4_from_player(data: &Value) -> Option<String> {
    let formats = data.pointer("/streamingData/formats")?.as_array()?;
    formats
        .iter()
        .filter_map(|fmt| {
            let url = fmt.get("url")?.as_str()?;
            if !url.starts_with("https://") {
                return None;
            }
            let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
            if !mime.starts_with("video/") {
                return None;
            }
            let mp4_bonus = if mime.contains("mp4") { 10_000 } else { 0 };
            let height = fmt.get("height").and_then(Value::as_i64).unwrap_or(0);
            Some((mp4_bonus + height, url.to_string()))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, url)| url)
}

pub(crate) fn pick_adaptive_av_from_player(data: &Value) -> Option<ResolvedReelStream> {
    let adaptive = data.pointer("/streamingData/adaptiveFormats")?.as_array()?;
    let best_video = adaptive
        .iter()
        .filter_map(|fmt| {
            let url = fmt.get("url")?.as_str()?;
            if !url.starts_with("https://") {
                return None;
            }
            let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
            if !mime.starts_with("video/mp4") {
                return None;
            }
            // Prefer H.264 (avc1) up to 1080p for universal software decoding on Wayland/Android.
            let h264_bonus = if mime.contains("avc1") { 50_000 } else { 0 };
            let height = fmt.get("height").and_then(Value::as_i64).unwrap_or(0);
            let capped_height = if height <= 1280 { height } else { 600 };
            Some((h264_bonus + capped_height, url.to_string()))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, url)| url)?;

    let best_audio = adaptive
        .iter()
        .filter_map(|fmt| {
            let url = fmt.get("url")?.as_str()?;
            if !url.starts_with("https://") {
                return None;
            }
            let mime = fmt.get("mimeType").and_then(Value::as_str).unwrap_or("");
            if !mime.starts_with("audio/") {
                return None;
            }
            let m4a_bonus = if mime.contains("mp4") { 100_000 } else { 0 };
            let bitrate = fmt.get("bitrate").and_then(Value::as_i64).unwrap_or(0) / 1000;
            Some((m4a_bonus + bitrate, url.to_string()))
        })
        .max_by_key(|(score, _)| *score)
        .map(|(_, url)| url);

    Some(ResolvedReelStream {
        video_url: best_video,
        audio_url: best_audio,
        author: None,
        title: None,
    })
}

pub(super) async fn list_youtube_shorts(
    sources: &[String],
    interests: &[String],
) -> Result<Vec<ReelClip>, String> {
    let targets = build_youtube_targets(sources, interests);
    if targets.is_empty() {
        return Ok(Vec::new());
    }
    let http = client()?;
    let mut clips = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut last_error = None;

    for (target_url, fallback_author) in targets.into_iter().take(4) {
        match fetch_shorts_from_page(&http, &target_url, &fallback_author).await {
            Ok(found) => {
                for clip in found {
                    if seen.insert(clip.id.clone()) {
                        clips.push(clip);
                    }
                }
            }
            Err(err) => last_error = Some(err),
        }
    }

    if clips.is_empty() {
        if let Some(err) = last_error {
            return Err(err);
        }
    }
    Ok(clips)
}

fn build_youtube_targets(sources: &[String], interests: &[String]) -> Vec<(String, String)> {
    let mut targets = Vec::new();
    for raw in sources {
        let item = raw.trim();
        if item.is_empty() {
            continue;
        }
        if item.starts_with("https://") || item.starts_with("http://") {
            let clean = item.trim_end_matches('/');
            let url = if clean.ends_with("/shorts") || clean.contains("/results?") {
                clean.to_string()
            } else {
                format!("{clean}/shorts")
            };
            targets.push((url, "YouTube".to_string()));
        } else if let Some(handle) = item.strip_prefix('@') {
            let handle = handle.trim_matches('/');
            if !handle.is_empty() {
                targets.push((
                    format!("https://www.youtube.com/@{handle}/shorts"),
                    format!("@{handle}"),
                ));
            }
        } else {
            let encoded = encode_query(&format!("{item} #shorts"));
            targets.push((
                format!(
                    "https://www.youtube.com/results?search_query={encoded}&sp=EgIYAQ%253D%253D"
                ),
                "YouTube Shorts".to_string(),
            ));
        }
    }

    if targets.is_empty() {
        for interest in interests {
            let query = match interest.as_str() {
                "humor" => "comedy funny #shorts",
                "jogos" => "gaming highlights #shorts",
                "esportes" => "sports highlights #shorts",
                "animais" => "cute animals #shorts",
                "ciencia" => "science space #shorts",
                "comida" => "cooking recipe #shorts",
                _ => "satisfying amazing #shorts",
            };
            let encoded = encode_query(query);
            targets.push((
                format!(
                    "https://www.youtube.com/results?search_query={encoded}&sp=EgIYAQ%253D%253D"
                ),
                "YouTube Shorts".to_string(),
            ));
        }
    }
    targets
}

fn encode_query(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => {
                out.push(byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

async fn fetch_shorts_from_page(
    http: &reqwest::Client,
    url: &str,
    fallback_author: &str,
) -> Result<Vec<ReelClip>, String> {
    let html = http
        .get(url)
        .header("accept-language", "pt-BR,pt;q=0.9,en;q=0.8")
        .header("cookie", YOUTUBE_ANON_CONSENT_COOKIE)
        .send()
        .await
        .map_err(|error| format!("YouTube indisponível: {error}"))?
        .error_for_status()
        .map_err(|error| format!("YouTube HTTP: {error}"))?
        .text()
        .await
        .map_err(|error| format!("Falha ao ler o YouTube: {error}"))?;

    Ok(parse_shorts_from_html(&html, fallback_author))
}

pub(crate) fn parse_shorts_from_html(html: &str, fallback_author: &str) -> Vec<ReelClip> {
    remember_visitor_data(html);
    let mut clips = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(data) = extract_initial_data(html) {
        let page_author = data
            .pointer("/metadata/channelMetadataRenderer/title")
            .and_then(Value::as_str)
            .unwrap_or(fallback_author);
        collect_shorts_nodes(&data, page_author, &mut seen, &mut clips);
    }

    if clips.is_empty() {
        if let Ok(re) = Regex::new(r#"/shorts/([A-Za-z0-9_-]{11})"#) {
            for captures in re.captures_iter(html) {
                let Some(id) = captures.get(1).map(|m| m.as_str()) else {
                    continue;
                };
                if !seen.insert(id.to_string()) {
                    continue;
                }
                clips.push(ReelClip {
                    platform: "youtube".to_string(),
                    id: id.to_string(),
                    title: "YouTube Short".to_string(),
                    author: fallback_author.to_string(),
                    page_url: format!("https://www.youtube.com/shorts/{id}"),
                    media_url: String::new(),
                    thumbnail_url: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")),
                    format_id: "best".to_string(),
                    has_audio: true,
                });
                if clips.len() >= 10 {
                    break;
                }
            }
        }
    }

    clips.truncate(10);
    clips
}

fn extract_initial_data(html: &str) -> Option<Value> {
    for marker in [
        "var ytInitialData = ",
        "window[\"ytInitialData\"] = ",
        "ytInitialData = ",
    ] {
        let Some(pos) = html.find(marker) else {
            continue;
        };
        let rest = &html[pos + marker.len()..];
        let Some(end) = json_object_end(rest) else {
            continue;
        };
        if let Ok(val) = serde_json::from_str::<Value>(&rest[..end]) {
            return Some(val);
        }
    }
    None
}

fn collect_shorts_nodes(
    node: &Value,
    fallback_author: &str,
    seen: &mut std::collections::HashSet<String>,
    out: &mut Vec<ReelClip>,
) {
    if out.len() >= 12 {
        return;
    }
    match node {
        Value::Object(map) => {
            if let Some(lockup) = map.get("shortsLockupViewModel") {
                if let Some(clip) = clip_from_shorts_lockup(lockup, fallback_author) {
                    if seen.insert(clip.id.clone()) {
                        out.push(clip);
                    }
                }
                return;
            }
            if let Some(reel) = map.get("reelItemRenderer") {
                if let Some(clip) = clip_from_reel_item(reel, fallback_author) {
                    if seen.insert(clip.id.clone()) {
                        out.push(clip);
                    }
                }
                return;
            }
            if let Some(video) = map.get("videoRenderer") {
                if let Some(clip) = clip_from_video_renderer(video, fallback_author) {
                    if seen.insert(clip.id.clone()) {
                        out.push(clip);
                    }
                }
                return;
            }
            for value in map.values() {
                collect_shorts_nodes(value, fallback_author, seen, out);
                if out.len() >= 12 {
                    break;
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_shorts_nodes(item, fallback_author, seen, out);
                if out.len() >= 12 {
                    break;
                }
            }
        }
        _ => {}
    }
}

fn clip_from_shorts_lockup(lockup: &Value, fallback_author: &str) -> Option<ReelClip> {
    let id = lockup
        .pointer("/onTap/innertubeCommand/reelWatchEndpoint/videoId")
        .and_then(Value::as_str)
        .or_else(|| {
            lockup
                .get("entityId")
                .and_then(Value::as_str)
                .and_then(|entity| entity.strip_prefix("shorts-shelf-item-"))
        })?;
    if id.len() != 11 {
        return None;
    }
    let title = lockup
        .pointer("/overlayMetadata/primaryText/content")
        .and_then(Value::as_str)
        .or_else(|| lockup.get("accessibilityText").and_then(Value::as_str))
        .unwrap_or("YouTube Short")
        .to_string();
    Some(ReelClip {
        platform: "youtube".to_string(),
        id: id.to_string(),
        title,
        author: fallback_author.to_string(),
        page_url: format!("https://www.youtube.com/shorts/{id}"),
        media_url: String::new(),
        thumbnail_url: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")),
        format_id: "best".to_string(),
        has_audio: true,
    })
}

fn clip_from_reel_item(reel: &Value, fallback_author: &str) -> Option<ReelClip> {
    let id = reel.get("videoId").and_then(Value::as_str)?;
    if id.len() != 11 {
        return None;
    }
    let title = reel
        .pointer("/headline/simpleText")
        .and_then(Value::as_str)
        .or_else(|| {
            reel.pointer("/headline/runs/0/text")
                .and_then(Value::as_str)
        })
        .unwrap_or("YouTube Short")
        .to_string();
    Some(ReelClip {
        platform: "youtube".to_string(),
        id: id.to_string(),
        title,
        author: fallback_author.to_string(),
        page_url: format!("https://www.youtube.com/shorts/{id}"),
        media_url: String::new(),
        thumbnail_url: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")),
        format_id: "best".to_string(),
        has_audio: true,
    })
}

fn clip_from_video_renderer(video: &Value, fallback_author: &str) -> Option<ReelClip> {
    let id = video.get("videoId").and_then(Value::as_str)?;
    if id.len() != 11 {
        return None;
    }
    let is_short = video
        .pointer("/navigationEndpoint/reelWatchEndpoint/videoId")
        .is_some()
        || video
            .pointer("/navigationEndpoint/commandMetadata/webCommandMetadata/url")
            .and_then(Value::as_str)
            .is_some_and(|u| u.contains("/shorts/"));
    if !is_short {
        return None;
    }
    let title = video
        .pointer("/title/runs/0/text")
        .and_then(Value::as_str)
        .or_else(|| video.pointer("/title/simpleText").and_then(Value::as_str))
        .unwrap_or("YouTube Short")
        .to_string();
    let author = video
        .pointer("/ownerText/runs/0/text")
        .and_then(Value::as_str)
        .or_else(|| {
            video
                .pointer("/longBylineText/runs/0/text")
                .and_then(Value::as_str)
        })
        .unwrap_or(fallback_author)
        .to_string();
    Some(ReelClip {
        platform: "youtube".to_string(),
        id: id.to_string(),
        title,
        author,
        page_url: format!("https://www.youtube.com/shorts/{id}"),
        media_url: String::new(),
        thumbnail_url: Some(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg")),
        format_id: "best".to_string(),
        has_audio: true,
    })
}
