//! Reddit post extraction and the combined video-plus-audio download.
//!
//! Reddit serves DASH video without the soundtrack. The combined format downloads
//! both pieces into the same destination and muxes them when ffmpeg is available.

use std::path::{Path, PathBuf};

use base64::Engine;
use plasma_core::AppEnv;
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

pub async fn analyze(url: &str) -> Result<PostMedia, String> {
    let id =
        post_id(url).ok_or_else(|| "Não foi possível identificar o post do Reddit.".to_string())?;
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
    from_post(post, url).or_else(|error| {
        post.pointer("/crosspost_parent_list/0")
            .map(|parent| from_post(parent, url))
            .unwrap_or(Err(error))
    })
}

pub fn post_id(url: &str) -> Option<String> {
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

/// Downloads the silent DASH video and its soundtrack into `destination`.
///
/// Each piece keeps its own partial file, so a retry continues the same bytes.
/// Without ffmpeg the video file is still left at the destination.
pub async fn download_with_audio(
    env: &AppEnv,
    id: &str,
    video_url: &str,
    destination: &Path,
) -> Result<(), String> {
    let audio_url = audio_url(video_url)
        .ok_or_else(|| "Este vídeo do Reddit não tem trilha de áudio separada.".to_string())?;
    let video_path = sidecar(destination, "video");
    let audio_path = sidecar(destination, "audio");
    let headers = reddit_headers();
    plasma_download::download_url_to_file_with_identity(
        env,
        &format!("{id}-video"),
        video_url,
        &video_path.to_string_lossy(),
        Some(headers.clone()),
        Some(&format!("reddit|{id}|video")),
    )
    .await?;
    plasma_download::download_url_to_file_with_identity(
        env,
        &format!("{id}-audio"),
        &audio_url,
        &audio_path.to_string_lossy(),
        Some(headers),
        Some(&format!("reddit|{id}|audio")),
    )
    .await?;
    match mux(&video_path, &audio_path, destination).await {
        Ok(()) => {
            let _ = std::fs::remove_file(&video_path);
            let _ = std::fs::remove_file(&audio_path);
            Ok(())
        }
        Err(error) => {
            if destination.exists() {
                return Err(error);
            }
            std::fs::rename(&video_path, destination).map_err(|rename| rename.to_string())?;
            Err(format!("{error} O vídeo foi salvo sem o áudio."))
        }
    }
}

pub fn audio_url(video_url: &str) -> Option<String> {
    let base = video_url.rsplit_once('/')?.0;
    Some(format!("{base}/DASH_audio.mp4"))
}

fn from_post(post: &Value, url: &str) -> Result<PostMedia, String> {
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
        .map(|thumbnail| thumbnail.replace("&amp;", "&"))
        .or_else(|| {
            post.pointer("/preview/images/0/source/url")
                .and_then(Value::as_str)
                .map(|thumbnail| thumbnail.replace("&amp;", "&"))
        });
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
    let mut audio = Vec::new();
    let fallback_url = post
        .pointer("/media/reddit_video/fallback_url")
        .or_else(|| post.pointer("/secure_media/reddit_video/fallback_url"))
        .and_then(Value::as_str);
    if let Some(fallback_url) = fallback_url {
        let clean_url = fallback_url.split('?').next().unwrap_or(fallback_url);
        if let Some(soundtrack) = audio_url(clean_url) {
            video.push(PostFormat {
                id: "reddit-av".to_string(),
                label: "Vídeo com áudio".to_string(),
                sub: "MP4 · Reddit".to_string(),
                size: "—".to_string(),
                best: true,
                url: Some(clean_url.to_string()),
                has_audio: true,
            });
            audio.push(PostFormat {
                id: "reddit-audio".to_string(),
                label: "Áudio".to_string(),
                sub: "MP4 · Reddit".to_string(),
                size: "—".to_string(),
                best: true,
                url: Some(soundtrack),
                has_audio: true,
            });
        }
        video.push(PostFormat {
            id: "reddit-video".to_string(),
            label: "Vídeo".to_string(),
            sub: "MP4 · Reddit".to_string(),
            size: "—".to_string(),
            best: audio.is_empty(),
            url: Some(clean_url.to_string()),
            has_audio: false,
        });
    }
    if video.is_empty() {
        video.extend(gallery_formats(post));
    }
    if video.is_empty() {
        let post_url = post.get("url").and_then(Value::as_str).unwrap_or(url);
        if post_url.starts_with("http") {
            let image = post_url.contains("i.redd.it")
                || post_url.contains(".jpg")
                || post_url.contains(".png")
                || post_url.contains(".gif");
            video.push(PostFormat {
                id: "reddit-link".to_string(),
                label: "Mídia do post".to_string(),
                sub: if image {
                    "Imagem · Reddit".to_string()
                } else {
                    "Link · Reddit".to_string()
                },
                size: "—".to_string(),
                best: true,
                url: Some(post_url.replace("&amp;", "&")),
                has_audio: false,
            });
        }
    }
    if video.is_empty() {
        return Err("O Reddit não retornou mídia para este post.".to_string());
    }
    Ok(PostMedia {
        title,
        author,
        author_full: Some(subreddit),
        duration,
        views: None,
        likes,
        thumbnail_url,
        original_url: url.to_string(),
        thumb_hue: 20,
        thumb_hue2: 200,
        video,
        audio,
    })
}

fn gallery_formats(post: &Value) -> Vec<PostFormat> {
    let Some(items) = post
        .pointer("/gallery_data/items")
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    let Some(metadata) = post.get("media_metadata").and_then(Value::as_object) else {
        return Vec::new();
    };
    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let id = item.get("media_id")?.as_str()?;
            let url = metadata.get(id)?.pointer("/s/u")?.as_str()?;
            Some(PostFormat {
                id: format!("reddit-gallery-{index}"),
                label: format!("Imagem {}", index + 1),
                sub: "Imagem · Reddit".to_string(),
                size: "—".to_string(),
                best: index == 0,
                url: Some(url.replace("&amp;", "&")),
                has_audio: false,
            })
        })
        .collect()
}

fn sidecar(destination: &Path, kind: &str) -> PathBuf {
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("reddit");
    destination.with_file_name(format!("{name}.{kind}"))
}

fn reddit_headers() -> std::collections::HashMap<String, String> {
    let mut headers = std::collections::HashMap::new();
    headers.insert(
        "user-agent".to_string(),
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/120 Safari/537.36"
            .to_string(),
    );
    headers.insert("referer".to_string(), "https://www.reddit.com/".to_string());
    headers
}

async fn mux(video: &Path, audio: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let status = tokio::process::Command::new("ffmpeg")
        .arg("-y")
        .arg("-i")
        .arg(video)
        .arg("-i")
        .arg(audio)
        .arg("-c")
        .arg("copy")
        .arg("-shortest")
        .arg(destination)
        .status()
        .await
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                "ffmpeg não está instalado para juntar o áudio do Reddit.".to_string()
            } else {
                format!("ffmpeg falhou ao iniciar: {error}")
            }
        })?;
    if status.success() {
        Ok(())
    } else {
        Err("ffmpeg não conseguiu juntar o vídeo e o áudio do Reddit.".to_string())
    }
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
    let minutes = seconds / 60;
    let seconds = seconds % 60;
    format!("{minutes}:{seconds:02}")
}

/// One public video from a subreddit listing.
#[derive(Clone, Debug, PartialEq)]
pub struct ListedVideo {
    pub id: String,
    pub title: String,
    pub author: String,
    pub subreddit: String,
    pub page_url: String,
    pub media_url: String,
    pub thumbnail_url: Option<String>,
}

/// A page of public videos and the cursor for the next page.
#[derive(Clone, Debug, PartialEq)]
pub struct VideoListing {
    pub items: Vec<ListedVideo>,
    pub after: Option<String>,
}

/// Keeps community names that can be placed in a Reddit listing URL.
pub fn normalize_subreddits(names: &[String]) -> Result<Vec<String>, String> {
    let mut normalized = Vec::new();
    for name in names {
        let name = name
            .trim()
            .trim_start_matches("/r/")
            .trim_start_matches("r/");
        if name.is_empty() {
            continue;
        }
        let valid = name.len() <= 21
            && name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '_');
        if !valid {
            return Err(format!("Comunidade inválida: {name}"));
        }
        if normalized
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(name))
        {
            continue;
        }
        normalized.push(name.to_string());
    }
    if normalized.is_empty() {
        return Err("Informe uma comunidade do Reddit.".to_string());
    }
    Ok(normalized)
}

/// Reads videos out of a Reddit listing. Posts without `reddit_video` are skipped.
pub fn videos_from_listing(value: &Value) -> VideoListing {
    let after = value
        .pointer("/data/after")
        .and_then(Value::as_str)
        .filter(|cursor| cursor_is_valid(cursor))
        .map(str::to_string);
    let items = value
        .pointer("/data/children")
        .and_then(Value::as_array)
        .map(|children| {
            children
                .iter()
                .filter_map(|child| child.get("data").and_then(listed_video))
                .collect()
        })
        .unwrap_or_default();
    VideoListing { items, after }
}

/// Loads one page of public videos from the given communities.
pub async fn list_videos(
    subreddits: &[String],
    after: Option<&str>,
) -> Result<VideoListing, String> {
    let names = normalize_subreddits(subreddits)?;
    let after = after
        .map(str::trim)
        .filter(|cursor| !cursor.is_empty())
        .map(|cursor| {
            if cursor_is_valid(cursor) {
                Ok(cursor.to_string())
            } else {
                Err("Página do Reddit inválida.".to_string())
            }
        })
        .transpose()?;
    let mut url = format!("https://www.reddit.com/r/{}/hot/?limit=25", names.join("+"));
    if let Some(after) = after.as_deref() {
        url.push_str("&after=");
        url.push_str(after);
    }
    if let Ok(html) = fetch_feed(&url).await {
        let page = videos_from_feed_html(&html);
        if !page.items.is_empty() {
            return Ok(page);
        }
    }
    let json_page = fetch_json_listing(&names, after.as_deref()).await?;
    if json_page.items.is_empty() {
        return Err("O Reddit não entregou vídeos nesta página.".to_string());
    }
    Ok(json_page)
}

async fn fetch_json_listing(names: &[String], after: Option<&str>) -> Result<VideoListing, String> {
    let joined = names.join("+");
    let app_client = reqwest::Client::builder()
        .user_agent("linux:com.camacho.plasmahub:v0.1.0 (public video reader)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())?;

    let endpoints = [
        format!("https://api.reddit.com/r/{joined}/hot?limit=30&raw_json=1"),
        format!("https://www.reddit.com/r/{joined}/hot.json?limit=30&raw_json=1"),
        format!("https://old.reddit.com/r/{joined}/hot.json?limit=30&raw_json=1"),
    ];

    for mut url in endpoints {
        if let Some(after) = after {
            url.push_str("&after=");
            url.push_str(after);
        }
        let Ok(response) = app_client
            .get(&url)
            .header("accept", "application/json")
            .send()
            .await
        else {
            continue;
        };
        if !response.status().is_success() {
            continue;
        }
        let Ok(value) = response.json::<Value>().await else {
            continue;
        };
        let listing = videos_from_listing(&value);
        if !listing.items.is_empty() {
            return Ok(listing);
        }
    }
    Err("O Reddit recusou a listagem.".to_string())
}

/// Reads native videos from the public subreddit HTML.
///
/// Reddit answers `.json` with 403 for this client. The same feed is in the
/// page a browser receives, including the packaged MP4 and the next cursor.
pub fn videos_from_feed_html(html: &str) -> VideoListing {
    let mut items = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<shreddit-post") {
        let chunk = &rest[start..];
        let next = chunk[1..]
            .find("<shreddit-post")
            .map(|index| index + 1)
            .unwrap_or(chunk.len());
        if let Some(video) = listed_video_from_html(&chunk[..next]) {
            if !items.iter().any(|item: &ListedVideo| item.id == video.id) {
                items.push(video);
            }
        }
        if next == 0 {
            break;
        }
        rest = &chunk[next..];
    }
    let after = html
        .match_indices("more-posts-cursor=\"")
        .filter_map(|(index, marker)| {
            let value = attribute_value(&html[index + marker.len()..])?;
            decode_cursor(&value)
        })
        .last();
    VideoListing { items, after }
}

/// Interests the reel queue can fill without the user naming a community.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReelInterest {
    pub id: &'static str,
    pub label: &'static str,
    pub subreddits: &'static [&'static str],
}

pub const REEL_INTERESTS: &[ReelInterest] = &[
    ReelInterest {
        id: "variedades",
        label: "Variedades",
        subreddits: &["nextfuckinglevel", "oddlysatisfying", "BeAmazed"],
    },
    ReelInterest {
        id: "humor",
        label: "Humor",
        subreddits: &["ContagiousLaughter", "instant_regret", "nonononoyes"],
    },
    ReelInterest {
        id: "jogos",
        label: "Jogos",
        subreddits: &["gaming", "Games", "livestreamfail"],
    },
    ReelInterest {
        id: "esportes",
        label: "Esportes",
        subreddits: &["sports", "soccer", "nba"],
    },
    ReelInterest {
        id: "animais",
        label: "Animais",
        subreddits: &["AnimalsBeingDerps", "Zoomies", "aww"],
    },
    ReelInterest {
        id: "ciencia",
        label: "Ciência",
        subreddits: &["space", "NatureIsFuckingLit", "interestingasfuck"],
    },
    ReelInterest {
        id: "comida",
        label: "Comida",
        subreddits: &["GifRecipes", "cookingvideos"],
    },
];

pub fn default_reel_interests() -> Vec<String> {
    vec!["variedades".to_string()]
}

/// Communities behind the chosen interests. An empty choice uses the default.
pub fn subreddits_for_interests(ids: &[String]) -> Vec<String> {
    let chosen = if ids.is_empty() {
        default_reel_interests()
    } else {
        ids.to_vec()
    };
    let mut names = Vec::new();
    for interest in REEL_INTERESTS {
        if !chosen.iter().any(|id| id == interest.id) {
            continue;
        }
        for name in interest.subreddits {
            if names
                .iter()
                .any(|existing: &String| existing.eq_ignore_ascii_case(name))
            {
                continue;
            }
            names.push((*name).to_string());
            if names.len() == 8 {
                return names;
            }
        }
    }
    if names.is_empty() {
        return subreddits_for_interests(&default_reel_interests());
    }
    names
}

async fn fetch_feed(url: &str) -> Result<String, String> {
    let client = listing_client()?;
    let mut jar = Vec::new();
    let first = request_feed(&client, url, &jar).await?;
    absorb_cookies(&first, &mut jar);
    let body = first
        .text()
        .await
        .map_err(|error| format!("Falha ao ler o Reddit: {error}"))?;
    if body.contains("<shreddit-post") {
        return Ok(body);
    }
    let Some(challenge) = challenge_query(&body) else {
        return Err("O Reddit recusou a listagem.".to_string());
    };
    let next = append_query(url, &challenge);
    let second = request_feed(&client, &next, &jar).await?;
    let body = second
        .text()
        .await
        .map_err(|error| format!("Falha ao ler o Reddit: {error}"))?;
    if body.contains("<shreddit-post") {
        Ok(body)
    } else {
        Err("O Reddit recusou a listagem.".to_string())
    }
}

fn listing_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0")
        .build()
        .map_err(|error| error.to_string())
}

async fn request_feed(
    client: &reqwest::Client,
    url: &str,
    jar: &[(String, String)],
) -> Result<reqwest::Response, String> {
    let mut request = client
        .get(url)
        .header("accept", "text/html")
        .header("referer", "https://www.reddit.com/");
    if !jar.is_empty() {
        let cookie = jar
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ");
        request = request.header("cookie", cookie);
    }
    request
        .send()
        .await
        .map_err(|error| format!("Reddit indisponível: {error}"))
}

fn absorb_cookies(response: &reqwest::Response, jar: &mut Vec<(String, String)>) {
    for value in response.headers().get_all(reqwest::header::SET_COOKIE) {
        let Ok(value) = value.to_str() else {
            continue;
        };
        let Some((pair, _)) = value.split_once(';') else {
            continue;
        };
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        jar.retain(|(existing, _)| existing != name);
        jar.push((name.to_string(), value.trim().to_string()));
    }
}

fn challenge_query(html: &str) -> Option<String> {
    if html.contains("<shreddit-post") {
        return None;
    }
    let token = attr_in(html, "jsc_token")?;
    let nonce = Regex::new(r#"await\(async e=>e\+e\)\("([0-9a-f]+)"\)"#)
        .ok()?
        .captures(html)?
        .get(1)?
        .as_str()
        .to_string();
    Some(format!(
        "solution={nonce}{nonce}&js_challenge=1&jsc_token={token}"
    ))
}

fn append_query(url: &str, query: &str) -> String {
    if url.contains('?') {
        format!("{url}&{query}")
    } else {
        format!("{url}?{query}")
    }
}

fn listed_video_from_html(chunk: &str) -> Option<ListedVideo> {
    if attr_in(chunk, "post-type").as_deref() != Some("video") {
        return None;
    }
    let id = attr_in(chunk, "id")?;
    let id = id.strip_prefix("t3_").unwrap_or(&id).to_string();
    if id.is_empty() {
        return None;
    }
    let media_url = best_packaged_mp4(chunk)?;
    let title = attr_in(chunk, "post-title").unwrap_or_else(|| "Reddit".to_string());
    let author = attr_in(chunk, "author")
        .map(|author| format!("u/{author}"))
        .unwrap_or_else(|| "reddit".to_string());
    let subreddit =
        attr_in(chunk, "subreddit-prefixed-name").unwrap_or_else(|| "r/reddit".to_string());
    let page_url = attr_in(chunk, "permalink")
        .map(|path| {
            if path.starts_with("http") {
                path
            } else {
                format!("https://www.reddit.com{path}")
            }
        })
        .unwrap_or_else(|| format!("https://www.reddit.com/comments/{id}"));
    let thumbnail_url = chunk
        .find("https://preview.redd.it/")
        .or_else(|| chunk.find("https://external-preview.redd.it/"))
        .and_then(|index| attribute_or_url(&chunk[index..]));
    Some(ListedVideo {
        id,
        title,
        author,
        subreddit,
        page_url,
        media_url,
        thumbnail_url,
    })
}

fn best_packaged_mp4(chunk: &str) -> Option<String> {
    let raw = attr_in(chunk, "packaged-media-json")?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let permutations = value
        .pointer("/playbackMp4s/permutations")
        .and_then(Value::as_array)?;
    permutations
        .iter()
        .filter_map(|item| {
            let source = item.get("source")?;
            let url = source.get("url")?.as_str()?;
            if !url.starts_with("https://") {
                return None;
            }
            let height = source
                .pointer("/dimensions/height")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            Some((height, url.to_string()))
        })
        .max_by_key(|(height, _)| *height)
        .map(|(_, url)| url)
}

fn attr_in(html: &str, name: &str) -> Option<String> {
    let marker = format!("{name}=\"");
    let mut rest = html;
    while let Some(index) = rest.find(&marker) {
        let boundary = index == 0
            || rest.as_bytes()[index - 1].is_ascii_whitespace()
            || rest.as_bytes()[index - 1] == b'<';
        if boundary {
            return attribute_value(&rest[index + marker.len()..]);
        }
        rest = &rest[index + marker.len()..];
    }
    None
}

fn attribute_value(html: &str) -> Option<String> {
    let end = html.find('"')?;
    Some(unescape_html(&html[..end]))
}

fn attribute_or_url(html: &str) -> Option<String> {
    let end = html.find(['"', '&', ' ', '<']).unwrap_or(html.len());
    let url = unescape_html(&html[..end]);
    url.starts_with("https://").then_some(url)
}

fn unescape_html(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

fn decode_cursor(value: &str) -> Option<String> {
    if cursor_is_valid(value) {
        return Some(value.to_string());
    }
    let mut padded = value.to_string();
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    for engine in [
        base64::engine::general_purpose::STANDARD,
        base64::engine::general_purpose::URL_SAFE,
    ] {
        let Ok(bytes) = engine.decode(&padded) else {
            continue;
        };
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        if cursor_is_valid(&text) {
            return Some(text);
        }
    }
    None
}

fn cursor_is_valid(cursor: &str) -> bool {
    let Some(rest) = cursor.strip_prefix("t3_") else {
        return false;
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn listed_video(post: &Value) -> Option<ListedVideo> {
    let video = post
        .pointer("/media/reddit_video")
        .or_else(|| post.pointer("/secure_media/reddit_video"))
        .or_else(|| post.pointer("/crosspost_parent_list/0/media/reddit_video"))
        .or_else(|| post.pointer("/crosspost_parent_list/0/secure_media/reddit_video"))?;
    let fallback = video.get("fallback_url")?.as_str()?;
    let media_url = fallback.split('?').next().unwrap_or(fallback);
    if !media_url.starts_with("https://") {
        return None;
    }
    let id = post.get("id")?.as_str()?.to_string();
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
    let page_url = post
        .get("permalink")
        .and_then(Value::as_str)
        .map(|path| {
            if path.starts_with("http") {
                path.to_string()
            } else {
                format!("https://www.reddit.com{path}")
            }
        })
        .unwrap_or_else(|| format!("https://www.reddit.com/comments/{id}"));
    let thumbnail_url = post
        .get("thumbnail")
        .and_then(Value::as_str)
        .filter(|thumbnail| thumbnail.starts_with("https://"))
        .map(str::to_string);
    Some(ListedVideo {
        id,
        title,
        author,
        subreddit,
        page_url,
        media_url: media_url.to_string(),
        thumbnail_url,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_comment_id_and_the_audio_track() {
        assert_eq!(
            post_id("https://www.reddit.com/r/rust/comments/abc123/title/"),
            Some("abc123".to_string())
        );
        assert_eq!(
            audio_url("https://v.redd.it/clip/DASH_720.mp4"),
            Some("https://v.redd.it/clip/DASH_audio.mp4".to_string())
        );
    }

    #[test]
    fn reads_gallery_images() {
        let post = serde_json::json!({
            "title": "Álbum",
            "author": "ada",
            "subreddit_name_prefixed": "r/pics",
            "gallery_data": { "items": [{ "media_id": "a" }, { "media_id": "b" }] },
            "media_metadata": {
                "a": { "s": { "u": "https://i.redd.it/a.jpg" } },
                "b": { "s": { "u": "https://i.redd.it/b.jpg" } }
            }
        });
        let media = from_post(&post, "https://www.reddit.com/r/pics/comments/abc/album").unwrap();
        assert_eq!(media.video.len(), 2);
        assert_eq!(
            media.video[1].url.as_deref(),
            Some("https://i.redd.it/b.jpg")
        );
    }

    #[test]
    fn listing_keeps_videos_and_the_next_cursor() {
        let listing = serde_json::json!({
            "data": {
                "after": "t3_next1",
                "children": [
                    {
                        "data": {
                            "id": "vid1",
                            "title": "Clipe",
                            "author": "ada",
                            "subreddit_name_prefixed": "r/videos",
                            "permalink": "/r/videos/comments/vid1/clipe/",
                            "thumbnail": "https://preview.redd.it/thumb.jpg",
                            "media": {
                                "reddit_video": {
                                    "fallback_url": "https://v.redd.it/clip/DASH_720.mp4?source=fallback"
                                }
                            }
                        }
                    },
                    {
                        "data": {
                            "id": "pic1",
                            "title": "Foto",
                            "url": "https://i.redd.it/pic.jpg"
                        }
                    },
                    {
                        "data": {
                            "id": "cross1",
                            "title": "Cruzado",
                            "author": "grace",
                            "permalink": "/r/funny/comments/cross1/cruzado/",
                            "crosspost_parent_list": [{
                                "media": {
                                    "reddit_video": {
                                        "fallback_url": "https://v.redd.it/other/DASH_480.mp4"
                                    }
                                }
                            }]
                        }
                    }
                ]
            }
        });
        let page = videos_from_listing(&listing);
        assert_eq!(page.after.as_deref(), Some("t3_next1"));
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0].id, "vid1");
        assert_eq!(
            page.items[0].media_url,
            "https://v.redd.it/clip/DASH_720.mp4"
        );
        assert_eq!(
            page.items[0].page_url,
            "https://www.reddit.com/r/videos/comments/vid1/clipe/"
        );
        assert_eq!(page.items[1].id, "cross1");
        assert!(normalize_subreddits(&["r/videos".to_string(), " Videos ".to_string()]).is_ok());
        assert!(normalize_subreddits(&["../etc".to_string()]).is_err());
    }

    #[test]
    fn feed_html_keeps_the_largest_video_and_the_next_cursor() {
        let html = r#"
            <shreddit-post id="t3_vid1" post-type="video" post-title="Clipe" author="ada"
                permalink="/r/videos/comments/vid1/clipe/" subreddit-prefixed-name="r/videos"
                packaged-media-json="{&quot;playbackMp4s&quot;:{&quot;permutations&quot;:[{&quot;source&quot;:{&quot;dimensions&quot;:{&quot;height&quot;:240},&quot;url&quot;:&quot;https://packaged-media.redd.it/low.mp4&quot;}},{&quot;source&quot;:{&quot;dimensions&quot;:{&quot;height&quot;:720},&quot;url&quot;:&quot;https://packaged-media.redd.it/high.mp4&quot;}}]}}">
            </shreddit-post>
            <shreddit-post id="t3_pic" post-type="image" post-title="Foto" more-posts-cursor="dDNfbmV4dDE"></shreddit-post>
        "#;
        let page = videos_from_feed_html(html);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].id, "vid1");
        assert_eq!(
            page.items[0].media_url,
            "https://packaged-media.redd.it/high.mp4"
        );
        assert_eq!(
            page.items[0].page_url,
            "https://www.reddit.com/r/videos/comments/vid1/clipe/"
        );
        assert_eq!(page.after.as_deref(), Some("t3_next1"));
        assert_eq!(
            subreddits_for_interests(&["jogos".to_string()]),
            vec![
                "gaming".to_string(),
                "Games".to_string(),
                "livestreamfail".to_string()
            ]
        );
        assert!(!subreddits_for_interests(&[]).is_empty());
    }
}
