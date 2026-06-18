use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, io::Write, path::Path};
use tauri::{Emitter, Manager};

const TWITTER_BEARER: &str = "AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMediaInfo {
    platform: String,
    title: String,
    author: String,
    author_full: Option<String>,
    duration: String,
    thumb_hue: u16,
    thumb_hue2: u16,
    thumbnail_url: Option<String>,
    original_url: String,
    formats: NativeFormats,
}

#[derive(Debug, Serialize)]
pub struct NativeFormats {
    video: Vec<NativeFormatOption>,
    audio: Vec<NativeFormatOption>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeFormatOption {
    id: String,
    label: String,
    sub: String,
    size: String,
    best: Option<bool>,
    url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeProfileInfo {
    platform: String,
    username: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    thumbnail_url: Option<String>,
    media_count: usize,
    media_urls: Vec<String>,
    media_items: Vec<NativeProfileMediaItem>,
    original_url: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeProfileMediaItem {
    url: String,
    thumbnail_url: Option<String>,
    is_video: bool,
}

#[derive(Debug, Default)]
struct ProfileMediaCollection {
    items: Vec<NativeProfileMediaItem>,
    thumbnail_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GuestTokenResponse {
    guest_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TwitterTweet {
    id_str: String,
    full_text: Option<String>,
    text: Option<String>,
    user: Option<TwitterUser>,
    extended_entities: Option<TwitterEntities>,
    entities: Option<TwitterEntities>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SyndicationTweet {
    id_str: Option<String>,
    text: Option<String>,
    user: Option<TwitterUser>,
    media_details: Option<Vec<TwitterMedia>>,
}

#[derive(Debug, Deserialize)]
struct TwitterUser {
    screen_name: Option<String>,
    name: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct TwitterEntities {
    media: Option<Vec<TwitterMedia>>,
}

#[derive(Clone, Debug, Deserialize)]
struct TwitterMedia {
    #[serde(rename = "type")]
    media_type: String,
    video_info: Option<TwitterVideoInfo>,
    media_url_https: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct TwitterVideoInfo {
    duration_millis: Option<u64>,
    variants: Vec<TwitterVariant>,
}

#[derive(Clone, Debug, Deserialize)]
struct TwitterVariant {
    content_type: String,
    bitrate: Option<u64>,
    url: String,
}

#[derive(Clone, Serialize)]
struct TwitterDownloadProgressEvent {
    id: String,
    progress: f64,
}

#[derive(Clone, Serialize)]
struct TwitterDownloadErrorEvent {
    id: String,
    error: String,
}

#[derive(Clone, Serialize)]
struct TwitterDownloadDoneEvent {
    id: String,
}

#[tauri::command]
pub async fn analyze_twitter_profile_native(
    url: String,
    cookies: Option<String>,
) -> Result<NativeProfileInfo, String> {
    let username = extract_twitter_profile_username(&url)?;
    eprintln!("[twitter_native] analyzing profile username={}", username);
    let cookie_header = normalize_cookie_input(cookies.as_deref())?;
    if cookie_header.is_some() {
        eprintln!("[twitter_native] using Twitter cookies for profile analyzer");
    }
    let client = build_twitter_client(cookie_header.as_deref())?;
    let profile = fetch_twitter_profile_graphql(&client, &username).await?;
    if let Some(reason) = first_string_for_key(&profile, "unavailable_reason") {
        return Err(format!("Perfil indisponivel no GraphQL: {}", reason));
    }

    let profile_result = profile
        .pointer("/data/user_result_by_screen_name/result")
        .unwrap_or(&profile);
    let display_name = first_string_for_pointer(profile_result, &["user", "core", "name"])
        .or_else(|| first_string_for_pointer(profile_result, &["core", "name"]));
    let resolved_username =
        first_string_for_pointer(profile_result, &["user", "core", "screen_name"])
            .or_else(|| first_string_for_pointer(profile_result, &["core", "screen_name"]))
            .unwrap_or_else(|| username.clone());
    let avatar_url = first_string_for_pointer(profile_result, &["user", "avatar", "image_url"])
        .or_else(|| first_string_for_pointer(profile_result, &["avatar", "image_url"]));

    let media = fetch_twitter_profile_media_pages(&client, &username).await?;
    let media_count = media.items.len();
    let media_urls = media
        .items
        .iter()
        .map(|item| item.url.clone())
        .collect::<Vec<_>>();

    Ok(NativeProfileInfo {
        platform: "twitter".to_string(),
        username: resolved_username,
        display_name,
        avatar_url,
        thumbnail_url: media.thumbnail_url.clone(),
        media_count,
        media_urls,
        media_items: media.items,
        original_url: url,
    })
}

#[tauri::command]
pub async fn analyze_twitter_tweet_native(
    url: String,
    cookies: Option<String>,
) -> Result<NativeMediaInfo, String> {
    let tweet_id = extract_twitter_id(&url)?;
    eprintln!("[twitter_native] analyzing tweet id={}", tweet_id);
    let cookie_header = normalize_cookie_input(cookies.as_deref())?;
    if cookie_header.is_some() {
        eprintln!("[twitter_native] using Twitter cookies for analyzer");
    }
    let client = build_twitter_client(cookie_header.as_deref())?;

    if cookie_header.is_some() {
        match analyze_twitter_tweet_graphql(&client, &tweet_id, &url).await {
            Ok(info) => return Ok(info),
            Err(graphql_error) => {
                eprintln!(
                    "[twitter_native] authenticated GraphQL analyzer failed for id={}: {}",
                    tweet_id, graphql_error
                );
            }
        }

        match analyze_twitter_tweet_page(&client, &tweet_id, &url).await {
            Ok(info) => return Ok(info),
            Err(page_error) => {
                eprintln!(
                    "[twitter_native] authenticated page analyzer failed for id={}: {}",
                    tweet_id, page_error
                );
            }
        }
    }

    match analyze_twitter_tweet_v1(&client, &tweet_id, &url).await {
        Ok(info) => Ok(info),
        Err(v1_error) => {
            eprintln!(
                "[twitter_native] v1 analyzer failed for id={}: {}. Trying syndication.",
                tweet_id, v1_error
            );
            analyze_twitter_tweet_syndication(&client, &tweet_id, &url)
                .await
                .map_err(|syndication_error| {
                    eprintln!(
                        "[twitter_native] syndication analyzer failed for id={}: {}",
                        tweet_id, syndication_error
                    );
                    format!(
                        "Twitter API falhou: {}. Syndication falhou: {}",
                        v1_error, syndication_error
                    )
                })
        }
    }
}

async fn analyze_twitter_tweet_v1(
    client: &reqwest::Client,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    let token_response = client
        .post("https://api.twitter.com/1.1/guest/activate.json")
        .header("Authorization", format!("Bearer {}", TWITTER_BEARER))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !token_response.status().is_success() {
        return Err(format!(
            "Twitter guest token HTTP {}",
            token_response.status()
        ));
    }

    let guest = token_response
        .json::<GuestTokenResponse>()
        .await
        .map_err(|e| e.to_string())?;
    let guest_token = guest
        .guest_token
        .ok_or_else(|| "Twitter guest token ausente".to_string())?;

    let tweet_response = client
        .get(format!(
            "https://api.twitter.com/1.1/statuses/show/{}.json?tweet_mode=extended",
            tweet_id
        ))
        .header("Authorization", format!("Bearer {}", TWITTER_BEARER))
        .header("x-guest-token", guest_token)
        .header("x-twitter-client-language", "en")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !tweet_response.status().is_success() {
        return Err(format!("Twitter API HTTP {}", tweet_response.status()));
    }

    let tweet = tweet_response
        .json::<TwitterTweet>()
        .await
        .map_err(|e| e.to_string())?;

    Ok(tweet_to_media_info(tweet, original_url.to_string()))
}

async fn fetch_twitter_profile_graphql(
    client: &reqwest::Client,
    username: &str,
) -> Result<Value, String> {
    eprintln!(
        "[twitter_native] fetching profile GraphQL username={}",
        username
    );
    let variables = json!({ "screenName": username });
    let url = format!(
        "https://x.com/i/api/graphql/SBM-5B0LqR_VAFawpkwGRQ/UserByScreenName?variables={}",
        encode_query_component(&variables.to_string())
    );
    fetch_twitter_graphql_json(client, url, "profile").await
}

async fn fetch_twitter_profile_media_graphql(
    client: &reqwest::Client,
    username: &str,
    cursor: Option<&str>,
) -> Result<Value, String> {
    if cursor.is_some() {
        eprintln!(
            "[twitter_native] fetching profile media GraphQL username={} cursor=next",
            username
        );
    } else {
        eprintln!(
            "[twitter_native] fetching profile media GraphQL username={}",
            username
        );
    }
    let variables = json!({
        "screenName": username,
        "count": 40,
        "cursor": cursor,
    });
    let url = format!(
        "https://x.com/i/api/graphql/Dq-6me1NYZkmsV_LjH6Pmg/mediaQuery?variables={}",
        encode_query_component(&variables.to_string())
    );
    fetch_twitter_graphql_json(client, url, "profile media").await
}

async fn fetch_twitter_profile_media_pages(
    client: &reqwest::Client,
    username: &str,
) -> Result<ProfileMediaCollection, String> {
    let mut collection = ProfileMediaCollection::default();
    let mut cursor = None;
    let mut seen_cursors = Vec::new();

    for page in 0..25 {
        let value =
            fetch_twitter_profile_media_graphql(client, username, cursor.as_deref()).await?;
        collect_graphql_profile_media_urls(&value, &mut collection);
        if is_profile_media_timeline_terminated(&value) {
            break;
        }

        let next_cursor = profile_media_bottom_cursor(&value);
        let Some(next_cursor) = next_cursor else {
            break;
        };
        if next_cursor.is_empty() || seen_cursors.contains(&next_cursor) {
            break;
        }

        seen_cursors.push(next_cursor.clone());
        cursor = Some(next_cursor);

        if page == 24 {
            eprintln!(
                "[twitter_native] profile media pagination stopped at safety limit username={}",
                username
            );
        }
    }

    eprintln!(
        "[twitter_native] profile media collected username={} items={}",
        username,
        collection.items.len()
    );
    Ok(collection)
}

async fn fetch_twitter_graphql_json(
    client: &reqwest::Client,
    url: String,
    label: &str,
) -> Result<Value, String> {
    let response = client
        .get(url)
        .header("Authorization", format!("Bearer {}", TWITTER_BEARER))
        .header("x-twitter-active-user", "yes")
        .header("x-twitter-client-language", "en")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "Twitter GraphQL {} HTTP {}: {}",
            label,
            status,
            body.chars().take(240).collect::<String>()
        ));
    }

    serde_json::from_str(&body).map_err(|e| {
        format!(
            "Twitter GraphQL {} JSON invalido: {}. Corpo: {}",
            label,
            e,
            body.chars().take(240).collect::<String>()
        )
    })
}

async fn analyze_twitter_tweet_graphql(
    client: &reqwest::Client,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    eprintln!(
        "[twitter_native] fetching authenticated GraphQL tweet id={}",
        tweet_id
    );
    let variables = json!({ "restId": tweet_id });
    let url = format!(
        "https://x.com/i/api/graphql/qJh6Id-hd5uVVGXofFCT4w/TweetResultByRestId?variables={}",
        encode_query_component(&variables.to_string())
    );
    let response = client
        .get(url)
        .header("Authorization", format!("Bearer {}", TWITTER_BEARER))
        .header("x-twitter-active-user", "yes")
        .header("x-twitter-client-language", "en")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    let body = response.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        return Err(format!(
            "Twitter GraphQL HTTP {}: {}",
            status,
            body.chars().take(240).collect::<String>()
        ));
    }

    let value: Value = serde_json::from_str(&body).map_err(|e| {
        format!(
            "Twitter GraphQL JSON invalido: {}. Corpo: {}",
            e,
            body.chars().take(240).collect::<String>()
        )
    })?;

    if let Some(reason) = first_string_for_key(&value, "unavailable_reason") {
        return Err(format!("Tweet indisponivel no GraphQL: {}", reason));
    }

    graphql_value_to_media_info(&value, tweet_id, original_url)
}

async fn analyze_twitter_tweet_page(
    client: &reqwest::Client,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    eprintln!(
        "[twitter_native] fetching authenticated tweet page id={}",
        tweet_id
    );
    let response = client
        .get(original_url)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Twitter page HTTP {}", response.status()));
    }

    let html = response.text().await.map_err(|e| e.to_string())?;
    let urls = extract_video_urls_from_text(&html);
    if urls.is_empty() {
        if html.contains("TweetUnavailable")
            && (html.contains("Protected") || html.contains("protected account"))
        {
            return Err(
                "Post protegido; os cookies nao parecem ter acesso a essa conta".to_string(),
            );
        }
        return Err("Nenhuma URL de video encontrada na pagina autenticada".to_string());
    }

    let title = extract_meta_content(&html, "og:description")
        .or_else(|| extract_meta_content(&html, "description"))
        .filter(|value| !value.eq_ignore_ascii_case("post"))
        .unwrap_or_else(|| "Twitter Video".to_string());
    let author = extract_author_from_url(original_url).unwrap_or_else(|| "twitter".to_string());
    let thumbnail_url = extract_image_url_from_text(&html);

    Ok(media_info_from_video_urls(
        urls,
        title,
        format!("@{}", author),
        None,
        None,
        thumbnail_url,
        original_url.to_string(),
    ))
}

async fn analyze_twitter_tweet_syndication(
    client: &reqwest::Client,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    let token = syndication_token(tweet_id)?;
    eprintln!(
        "[twitter_native] fetching syndication tweet id={} token={}",
        tweet_id, token
    );
    let response = client
        .get(format!(
            "https://cdn.syndication.twimg.com/tweet-result?id={}&token={}&lang=pt",
            tweet_id, token
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        eprintln!(
            "[twitter_native] syndication HTTP status for id={}: {}",
            tweet_id,
            response.status()
        );
        return Err(format!("Twitter syndication HTTP {}", response.status()));
    }

    let body = response.text().await.map_err(|e| e.to_string())?;
    if body.contains("\"__typename\":\"TweetTombstone\"") {
        return Err("Post indisponivel no syndication; pode ser protegido ou restrito".to_string());
    }

    let tweet: SyndicationTweet = serde_json::from_str(&body).map_err(|e| {
        let snippet: String = body.chars().take(240).collect();
        format!("JSON syndication invalido: {}. Corpo: {}", e, snippet)
    })?;

    let media = tweet.media_details.unwrap_or_default();
    let converted = TwitterTweet {
        id_str: tweet.id_str.unwrap_or_else(|| tweet_id.to_string()),
        full_text: tweet.text,
        text: None,
        user: tweet.user,
        extended_entities: Some(TwitterEntities { media: Some(media) }),
        entities: None,
    };

    Ok(tweet_to_media_info(converted, original_url.to_string()))
}

#[tauri::command]
pub async fn download_twitter_native(
    app_handle: tauri::AppHandle,
    id: String,
    url: String,
    filename: String,
    cookies: Option<String>,
) -> Result<(), String> {
    let result = download_twitter_native_inner(&app_handle, &id, &url, &filename, cookies).await;
    if let Err(error) = &result {
        let _ = app_handle.emit(
            "twitter-download-error",
            TwitterDownloadErrorEvent {
                id: id.clone(),
                error: error.clone(),
            },
        );
    }
    result
}

#[tauri::command]
pub async fn download_twitter_profile_native(
    app_handle: tauri::AppHandle,
    id: String,
    username: String,
    media_urls: Vec<String>,
    cookies: Option<String>,
) -> Result<(), String> {
    let result =
        download_twitter_profile_native_inner(&app_handle, &id, &username, media_urls, cookies)
            .await;
    if let Err(error) = &result {
        let _ = app_handle.emit(
            "twitter-download-error",
            TwitterDownloadErrorEvent {
                id: id.clone(),
                error: error.clone(),
            },
        );
    }
    result
}

async fn download_twitter_profile_native_inner(
    app_handle: &tauri::AppHandle,
    id: &str,
    username: &str,
    media_urls: Vec<String>,
    cookies: Option<String>,
) -> Result<(), String> {
    if media_urls.is_empty() {
        return Err("Nenhuma mídia encontrada para baixar".to_string());
    }

    let download_dir = app_handle
        .path()
        .download_dir()
        .map_err(|e| e.to_string())?
        .join(format!("plasma_twitter_{}", sanitize_filename(username)));
    fs::create_dir_all(&download_dir).map_err(|e| e.to_string())?;

    let cookie_header = normalize_cookie_input(cookies.as_deref())?;
    let client = build_twitter_client(cookie_header.as_deref())?;
    let total_items = media_urls.len() as f64;

    for (idx, media_url) in media_urls.iter().enumerate() {
        let ext = media_extension(media_url);
        let output_path = download_dir.join(format!("{:04}.{}", idx + 1, ext));
        let mut response = client
            .get(media_url)
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !response.status().is_success() {
            return Err(format!(
                "Twitter profile media HTTP {} em {}",
                response.status(),
                media_url
            ));
        }

        let mut file = fs::File::create(output_path).map_err(|e| e.to_string())?;
        while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
            file.write_all(&chunk).map_err(|e| e.to_string())?;
        }

        let progress = (((idx + 1) as f64 / total_items) * 100.0).min(99.0);
        let _ = app_handle.emit(
            "twitter-download-progress",
            TwitterDownloadProgressEvent {
                id: id.to_string(),
                progress,
            },
        );
    }

    let _ = app_handle.emit(
        "twitter-download-done",
        TwitterDownloadDoneEvent { id: id.to_string() },
    );
    Ok(())
}

async fn download_twitter_native_inner(
    app_handle: &tauri::AppHandle,
    id: &str,
    url: &str,
    filename: &str,
    cookies: Option<String>,
) -> Result<(), String> {
    let download_dir = app_handle
        .path()
        .download_dir()
        .map_err(|e| e.to_string())?;
    fs::create_dir_all(&download_dir).map_err(|e| e.to_string())?;
    let output_path = download_dir.join(filename);

    let cookie_header = normalize_cookie_input(cookies.as_deref())?;
    if cookie_header.is_some() {
        eprintln!("[twitter_native] using Twitter cookies for media download");
    }
    let client = build_twitter_client(cookie_header.as_deref())?;
    let mut response = client.get(url).send().await.map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("Twitter media HTTP {}", response.status()));
    }

    let total = response.content_length().unwrap_or(0);
    let mut received = 0_u64;
    let mut file = fs::File::create(output_path).map_err(|e| e.to_string())?;

    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        received += chunk.len() as u64;
        let progress = if total > 0 {
            ((received as f64 / total as f64) * 100.0).min(99.0)
        } else {
            50.0
        };
        let _ = app_handle.emit(
            "twitter-download-progress",
            TwitterDownloadProgressEvent {
                id: id.to_string(),
                progress,
            },
        );
    }

    let _ = app_handle.emit(
        "twitter-download-done",
        TwitterDownloadDoneEvent { id: id.to_string() },
    );
    Ok(())
}

fn extract_twitter_id(url: &str) -> Result<String, String> {
    let re = Regex::new(r"(?:twitter\.com|x\.com)/(?:\w+/status|i/web/status)/(\d+)")
        .map_err(|e| e.to_string())?;
    re.captures(url)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .ok_or_else(|| "Nao foi possivel identificar o tweet".to_string())
}

fn extract_twitter_profile_username(url: &str) -> Result<String, String> {
    let re = Regex::new(r"(?:twitter\.com|x\.com)/([A-Za-z0-9_]{1,15})/?(?:\?.*)?$")
        .map_err(|e| e.to_string())?;
    re.captures(url)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .ok_or_else(|| "Nao foi possivel identificar o perfil".to_string())
}

fn build_twitter_client(cookie_header: Option<&str>) -> Result<reqwest::Client, String> {
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static(
            "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 Chrome/120 Safari/537.36",
        ),
    );
    headers.insert(REFERER, HeaderValue::from_static("https://x.com/"));
    headers.insert("accept", HeaderValue::from_static("application/json"));

    if let Some(cookie_header) = cookie_header {
        headers.insert(
            COOKIE,
            HeaderValue::from_str(cookie_header).map_err(|e| e.to_string())?,
        );
        headers.insert(
            "x-twitter-auth-type",
            HeaderValue::from_static("OAuth2Session"),
        );

        if let Some(ct0) = cookie_value(cookie_header, "ct0") {
            headers.insert(
                "x-csrf-token",
                HeaderValue::from_str(&ct0).map_err(|e| e.to_string())?,
            );
        }
    }

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| e.to_string())
}

fn normalize_cookie_input(input: Option<&str>) -> Result<Option<String>, String> {
    let Some(raw_input) = input else {
        return Ok(None);
    };
    let trimmed = raw_input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let content = if Path::new(trimmed).is_file() {
        fs::read_to_string(trimmed).map_err(|e| format!("Nao foi possivel ler cookies: {}", e))?
    } else {
        trimmed.to_string()
    };

    let normalized = if looks_like_netscape_cookie_file(&content) {
        netscape_cookie_header(&content)
    } else {
        raw_cookie_header(&content)
    };

    if normalized.is_empty() {
        Ok(None)
    } else {
        Ok(Some(normalized))
    }
}

fn looks_like_netscape_cookie_file(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.split('\t').count() >= 7 && !line.trim_start().starts_with("# "))
}

fn netscape_cookie_header(content: &str) -> String {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("# Netscape") {
                return None;
            }

            let parts: Vec<&str> = trimmed.split('\t').collect();
            if parts.len() < 7 {
                return None;
            }

            let domain = parts[0].trim_start_matches("#HttpOnly_");
            if !is_twitter_cookie_domain(domain) {
                return None;
            }

            let name = parts[5].trim();
            let value = parts[6].trim();
            if name.is_empty() || value.is_empty() {
                None
            } else {
                Some(format!("{}={}", name, value))
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn raw_cookie_header(content: &str) -> String {
    content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.strip_prefix("Cookie:")
                .or_else(|| line.strip_prefix("cookie:"))
                .unwrap_or(line)
                .trim()
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn is_twitter_cookie_domain(domain: &str) -> bool {
    let domain = domain.trim_start_matches('.').to_ascii_lowercase();
    matches!(
        domain.as_str(),
        "twitter.com" | "x.com" | "twimg.com" | "api.twitter.com"
    ) || domain.ends_with(".twitter.com")
        || domain.ends_with(".x.com")
        || domain.ends_with(".twimg.com")
}

fn cookie_value(cookie_header: &str, name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        if key.trim() == name {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn graphql_value_to_media_info(
    value: &Value,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    let mut variants = Vec::new();
    collect_graphql_video_variants(value, &mut variants);
    variants.sort_by(|a, b| b.bitrate.unwrap_or(0).cmp(&a.bitrate.unwrap_or(0)));
    variants.dedup_by(|a, b| a.url == b.url);

    if variants.is_empty() {
        return Err("GraphQL nao retornou variantes MP4 para esse tweet".to_string());
    }

    let title = first_string_for_key(value, "full_text")
        .map(|text| strip_tco_links(&text))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| format!("Twitter/X {}", tweet_id));
    let author = first_string_for_key(value, "screen_name")
        .or_else(|| extract_author_from_url(original_url))
        .unwrap_or_else(|| "twitter".to_string());
    let author_full = first_string_for_key(value, "name");
    let duration_ms = first_u64_for_key(value, "duration_millis");
    let thumbnail_url = first_string_for_key(value, "media_url_https")
        .map(|url| format!("{}?format=jpg&name=small", url));

    let video_formats = variants
        .iter()
        .enumerate()
        .map(|(idx, variant)| {
            let kbps = variant.bitrate.unwrap_or(0) / 1000;
            let height = video_url_height(&variant.url);
            let label = if height > 0 {
                format!("{}p", height)
            } else if kbps > 1500 {
                "1080p".to_string()
            } else if kbps > 800 {
                "720p".to_string()
            } else if kbps > 0 {
                "360p".to_string()
            } else {
                "MP4".to_string()
            };

            NativeFormatOption {
                id: format!("tw-graphql-video-{}", idx),
                label,
                sub: if kbps > 0 {
                    format!("MP4 · {}kbps", kbps)
                } else {
                    "MP4 · Twitter".to_string()
                },
                size: "—".to_string(),
                best: Some(idx == 0),
                url: Some(variant.url.clone()),
            }
        })
        .collect::<Vec<_>>();

    let audio_formats = video_formats
        .first()
        .map(|best| {
            vec![NativeFormatOption {
                id: "tw-audio".to_string(),
                label: "Áudio".to_string(),
                sub: "MP4 · Twitter".to_string(),
                size: "—".to_string(),
                best: Some(true),
                url: best.url.clone(),
            }]
        })
        .unwrap_or_default();

    Ok(NativeMediaInfo {
        platform: "twitter".to_string(),
        title: title.chars().take(120).collect(),
        author: format!("@{}", author),
        author_full,
        duration: duration_ms
            .map(|ms| format_duration(ms / 1000))
            .unwrap_or_else(|| "—".to_string()),
        thumb_hue: 200,
        thumb_hue2: 220,
        thumbnail_url,
        original_url: original_url.to_string(),
        formats: NativeFormats {
            video: video_formats,
            audio: audio_formats,
        },
    })
}

fn collect_graphql_video_variants(value: &Value, output: &mut Vec<TwitterVariant>) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(variants)) = map.get("variants") {
                for variant in variants {
                    if variant
                        .get("content_type")
                        .and_then(Value::as_str)
                        .is_some_and(|content_type| content_type == "video/mp4")
                    {
                        if let Some(url) = variant.get("url").and_then(Value::as_str) {
                            output.push(TwitterVariant {
                                content_type: "video/mp4".to_string(),
                                bitrate: variant.get("bitrate").and_then(Value::as_u64),
                                url: url.to_string(),
                            });
                        }
                    }
                }
            }

            for child in map.values() {
                collect_graphql_video_variants(child, output);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_graphql_video_variants(child, output);
            }
        }
        _ => {}
    }
}

fn first_string_for_key(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(Value::as_str) {
                return Some(found.to_string());
            }
            map.values()
                .find_map(|child| first_string_for_key(child, key))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| first_string_for_key(child, key)),
        _ => None,
    }
}

fn first_u64_for_key(value: &Value, key: &str) -> Option<u64> {
    match value {
        Value::Object(map) => {
            if let Some(found) = map.get(key).and_then(Value::as_u64) {
                return Some(found);
            }
            map.values().find_map(|child| first_u64_for_key(child, key))
        }
        Value::Array(items) => items.iter().find_map(|child| first_u64_for_key(child, key)),
        _ => None,
    }
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            _ => encoded.push_str(&format!("%{:02X}", byte)),
        }
    }
    encoded
}

fn extract_video_urls_from_text(text: &str) -> Vec<String> {
    let normalized = normalize_embedded_html(text);
    let re = Regex::new(r#"https://video\.twimg\.com/[^"'<>\s]+?\.mp4(?:\?[^"'<>\s]+)?"#)
        .expect("valid video url regex");
    let mut urls = Vec::new();
    for hit in re.find_iter(&normalized) {
        let url = hit.as_str().to_string();
        if !urls.contains(&url) {
            urls.push(url);
        }
    }
    urls.sort_by(|a, b| video_url_height(b).cmp(&video_url_height(a)));
    urls
}

fn extract_image_url_from_text(text: &str) -> Option<String> {
    let normalized = normalize_embedded_html(text);
    let re =
        Regex::new(r#"https://pbs\.twimg\.com/[^"'<>\s]+?(?:\.jpg|\.png|\.webp)(?:\?[^"'<>\s]+)?"#)
            .expect("valid image url regex");
    re.find(&normalized).map(|hit| hit.as_str().to_string())
}

fn collect_graphql_profile_media_urls(value: &Value, output: &mut ProfileMediaCollection) {
    match value {
        Value::Object(map) => {
            let thumbnail_url = map
                .get("media_url_https")
                .and_then(Value::as_str)
                .or_else(|| map.get("original_img_url").and_then(Value::as_str))
                .filter(|url| url.contains("pbs.twimg.com/media/"))
                .map(normalize_twitter_photo_url);

            if let Some(Value::Object(video_info)) = map.get("video_info") {
                if let Some(url) = best_video_variant_url(video_info) {
                    push_profile_media_item(&url, thumbnail_url.clone(), true, output);
                    if output.thumbnail_url.is_none() {
                        output.thumbnail_url = thumbnail_url;
                    }
                }
            } else if let Some(url) = thumbnail_url {
                push_profile_media_item(&url, Some(url.clone()), false, output);
                if output.thumbnail_url.is_none() {
                    output.thumbnail_url = Some(url);
                }
            }

            for child in map.values() {
                collect_graphql_profile_media_urls(child, output);
            }
        }
        Value::Array(items) => {
            for child in items {
                collect_graphql_profile_media_urls(child, output);
            }
        }
        _ => {}
    }
}

fn best_video_variant_url(video_info: &serde_json::Map<String, Value>) -> Option<String> {
    let variants = video_info.get("variants")?.as_array()?;
    variants
        .iter()
        .filter_map(|variant| {
            let content_type = variant.get("content_type").and_then(Value::as_str)?;
            if content_type != "video/mp4" {
                return None;
            }
            let url = variant.get("url").and_then(Value::as_str)?.to_string();
            let bitrate = variant.get("bitrate").and_then(Value::as_u64).unwrap_or(0);
            Some((bitrate, url))
        })
        .max_by_key(|(bitrate, _)| *bitrate)
        .map(|(_, url)| url)
}

fn push_profile_media_item(
    url: &str,
    thumbnail_url: Option<String>,
    is_video: bool,
    output: &mut ProfileMediaCollection,
) {
    if !url.contains("pbs.twimg.com/media/") && !url.contains("video.twimg.com/") {
        return;
    }

    let normalized = if url.contains("pbs.twimg.com/media/") {
        normalize_twitter_photo_url(url)
    } else {
        url.to_string()
    };

    if !output.items.iter().any(|item| item.url == normalized) {
        output.items.push(NativeProfileMediaItem {
            url: normalized,
            thumbnail_url,
            is_video,
        });
    }
}

fn normalize_twitter_photo_url(url: &str) -> String {
    let mut normalized = url.to_string();
    if !normalized.contains("name=") {
        normalized.push_str(if normalized.contains('?') {
            "&name=orig"
        } else {
            "?name=orig"
        });
    }
    normalized
}

fn profile_media_bottom_cursor(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if map
                .get("cursor_type")
                .and_then(Value::as_str)
                .is_some_and(|cursor_type| cursor_type.eq_ignore_ascii_case("Bottom"))
            {
                if let Some(cursor) = map.get("value").and_then(Value::as_str) {
                    return Some(cursor.to_string());
                }
            }

            map.get("bottom_cursor")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.values()
                        .find_map(|child| profile_media_bottom_cursor(child))
                })
        }
        Value::Array(items) => items
            .iter()
            .find_map(|child| profile_media_bottom_cursor(child)),
        _ => None,
    }
}

fn is_profile_media_timeline_terminated(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map
                .get("is_terminated_at_bottom")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return true;
            }
            map.values().any(is_profile_media_timeline_terminated)
        }
        Value::Array(items) => items.iter().any(is_profile_media_timeline_terminated),
        _ => false,
    }
}

fn first_string_for_pointer(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str().map(ToString::to_string)
}

fn normalize_embedded_html(text: &str) -> String {
    text.replace("\\/", "/")
        .replace("&amp;", "&")
        .replace("\\u0026", "&")
        .replace("\\u003d", "=")
}

fn extract_meta_content(html: &str, name: &str) -> Option<String> {
    let pattern = format!(
        r#"<meta\s+(?:property|name)=["']{}["']\s+content=["']([^"']*)["']"#,
        regex::escape(name)
    );
    Regex::new(&pattern)
        .ok()?
        .captures(html)
        .and_then(|captures| captures.get(1))
        .map(|value| {
            value
                .as_str()
                .replace("&amp;", "&")
                .replace("&#x27;", "'")
                .replace("&quot;", "\"")
        })
}

fn extract_author_from_url(url: &str) -> Option<String> {
    Regex::new(r"(?:twitter\.com|x\.com)/([A-Za-z0-9_]{1,15})/status/")
        .ok()?
        .captures(url)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
}

fn video_url_height(url: &str) -> u32 {
    Regex::new(r"/\d+x(\d+)/")
        .ok()
        .and_then(|re| re.captures(url))
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<u32>().ok())
        .unwrap_or(0)
}

fn media_extension(url: &str) -> &'static str {
    let clean = url.split('?').next().unwrap_or(url).to_ascii_lowercase();
    if clean.ends_with(".png") {
        "png"
    } else if clean.ends_with(".webp") {
        "webp"
    } else if clean.ends_with(".mp4") {
        "mp4"
    } else {
        "jpg"
    }
}

fn sanitize_filename(value: &str) -> String {
    let safe: String = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        "twitter".to_string()
    } else {
        safe
    }
}

fn media_info_from_video_urls(
    urls: Vec<String>,
    title: String,
    author: String,
    author_full: Option<String>,
    duration: Option<String>,
    thumbnail_url: Option<String>,
    original_url: String,
) -> NativeMediaInfo {
    let mut video_formats = Vec::new();
    for (idx, url) in urls.into_iter().enumerate() {
        let height = video_url_height(&url);
        let label = if height > 0 {
            format!("{}p", height)
        } else {
            "MP4".to_string()
        };
        video_formats.push(NativeFormatOption {
            id: format!("tw-page-video-{}", idx),
            label,
            sub: "MP4 · Twitter".to_string(),
            size: "—".to_string(),
            best: Some(idx == 0),
            url: Some(url),
        });
    }

    let audio_formats = video_formats
        .first()
        .map(|best| {
            vec![NativeFormatOption {
                id: "tw-audio".to_string(),
                label: "Áudio".to_string(),
                sub: "MP4 · Twitter".to_string(),
                size: "—".to_string(),
                best: Some(true),
                url: best.url.clone(),
            }]
        })
        .unwrap_or_else(|| {
            vec![NativeFormatOption {
                id: "na".to_string(),
                label: "Sem áudio".to_string(),
                sub: "—".to_string(),
                size: "—".to_string(),
                best: None,
                url: None,
            }]
        });

    NativeMediaInfo {
        platform: "twitter".to_string(),
        title: title.chars().take(120).collect(),
        author,
        author_full,
        duration: duration.unwrap_or_else(|| "—".to_string()),
        thumb_hue: 200,
        thumb_hue2: 220,
        thumbnail_url,
        original_url,
        formats: NativeFormats {
            video: video_formats,
            audio: audio_formats,
        },
    }
}

fn syndication_token(tweet_id: &str) -> Result<String, String> {
    let id = tweet_id.parse::<f64>().map_err(|e| e.to_string())?;
    let value = (id / 1_000_000_000_000_000.0) * std::f64::consts::PI;
    let base36 = to_base36_float(value, 8);
    Ok(base36
        .chars()
        .filter(|ch| *ch != '0' && *ch != '.')
        .collect())
}

fn to_base36_float(value: f64, precision: usize) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let integer = value.trunc() as u64;
    let mut int = integer;
    let mut int_digits = Vec::new();

    if int == 0 {
        int_digits.push('0');
    } else {
        while int > 0 {
            int_digits.push(DIGITS[(int % 36) as usize] as char);
            int /= 36;
        }
        int_digits.reverse();
    }

    let mut output: String = int_digits.into_iter().collect();
    let mut fraction = value.fract();
    if fraction > 0.0 && precision > 0 {
        output.push('.');
        for _ in 0..precision {
            fraction *= 36.0;
            let digit = fraction.floor() as usize;
            output.push(DIGITS[digit.min(35)] as char);
            fraction -= digit as f64;
            if fraction.abs() < f64::EPSILON {
                break;
            }
        }
    }

    output
}

fn tweet_to_media_info(tweet: TwitterTweet, original_url: String) -> NativeMediaInfo {
    let media = tweet
        .extended_entities
        .as_ref()
        .and_then(|entities| entities.media.as_ref())
        .or_else(|| {
            tweet
                .entities
                .as_ref()
                .and_then(|entities| entities.media.as_ref())
        })
        .cloned()
        .unwrap_or_default();

    let text = tweet
        .full_text
        .or(tweet.text)
        .unwrap_or_else(|| "Twitter/X Media".to_string());
    let display_text = strip_tco_links(&text);

    let mut video_formats = Vec::new();
    let mut audio_formats = Vec::new();
    let mut duration_ms = None;

    for (media_idx, item) in media.iter().enumerate() {
        if matches!(item.media_type.as_str(), "video" | "animated_gif") {
            if let Some(video_info) = &item.video_info {
                duration_ms = duration_ms.or(video_info.duration_millis);
                let mut variants: Vec<&TwitterVariant> = video_info
                    .variants
                    .iter()
                    .filter(|variant| variant.content_type == "video/mp4")
                    .collect();
                variants.sort_by(|a, b| b.bitrate.unwrap_or(0).cmp(&a.bitrate.unwrap_or(0)));

                for (variant_idx, variant) in variants.iter().enumerate() {
                    let kbps = variant.bitrate.unwrap_or(0) / 1000;
                    let label = if kbps > 1500 {
                        "1080p"
                    } else if kbps > 800 {
                        "720p"
                    } else if kbps > 0 {
                        "360p"
                    } else {
                        "MP4"
                    };
                    let dur_sec = video_info.duration_millis.unwrap_or(30_000) as f64 / 1000.0;
                    let size = if kbps > 0 {
                        format!("~{:.1} MB", dur_sec * kbps as f64 / 8.0 / 1024.0)
                    } else {
                        "—".to_string()
                    };

                    video_formats.push(NativeFormatOption {
                        id: format!("tw-video-{}-{}", media_idx, variant_idx),
                        label: label.to_string(),
                        sub: format!("MP4 · {}kbps", kbps),
                        size,
                        best: Some(video_formats.is_empty()),
                        url: Some(variant.url.clone()),
                    });
                }
            }
        } else if item.media_type == "photo" {
            if let Some(url) = &item.media_url_https {
                video_formats.push(NativeFormatOption {
                    id: format!("tw-photo-{}", media_idx),
                    label: "Imagem".to_string(),
                    sub: "JPG · Twitter".to_string(),
                    size: "—".to_string(),
                    best: Some(video_formats.is_empty()),
                    url: Some(format!("{}?format=jpg&name=orig", url)),
                });
            }
        }
    }

    if let Some(best) = video_formats.first() {
        audio_formats.push(NativeFormatOption {
            id: "tw-audio".to_string(),
            label: "Áudio".to_string(),
            sub: "MP4 · Twitter".to_string(),
            size: "—".to_string(),
            best: Some(true),
            url: best.url.clone(),
        });
    }

    let thumbnail_url = media
        .iter()
        .find_map(|item| item.media_url_https.clone())
        .map(|url| format!("{}?format=jpg&name=small", url));

    NativeMediaInfo {
        platform: "twitter".to_string(),
        title: if display_text.is_empty() {
            format!("Twitter/X {}", tweet.id_str)
        } else {
            display_text.chars().take(120).collect()
        },
        author: format!(
            "@{}",
            tweet
                .user
                .as_ref()
                .and_then(|user| user.screen_name.as_ref())
                .map(String::as_str)
                .unwrap_or("twitter")
        ),
        author_full: tweet.user.and_then(|user| user.name),
        duration: duration_ms
            .map(|ms| format_duration((ms / 1000) as u64))
            .unwrap_or_else(|| "—".to_string()),
        thumb_hue: 200,
        thumb_hue2: 220,
        thumbnail_url,
        original_url,
        formats: NativeFormats {
            video: if video_formats.is_empty() {
                vec![NativeFormatOption {
                    id: "na".to_string(),
                    label: "Sem mídia".to_string(),
                    sub: "Nenhuma mídia encontrada".to_string(),
                    size: "—".to_string(),
                    best: None,
                    url: None,
                }]
            } else {
                video_formats
            },
            audio: if audio_formats.is_empty() {
                vec![NativeFormatOption {
                    id: "na".to_string(),
                    label: "Sem áudio".to_string(),
                    sub: "—".to_string(),
                    size: "—".to_string(),
                    best: None,
                    url: None,
                }]
            } else {
                audio_formats
            },
        },
    }
}

fn strip_tco_links(text: &str) -> String {
    let re = Regex::new(r"https?://t\.co/\S+").expect("valid t.co regex");
    re.replace_all(text, "").trim().to_string()
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{}:{:02}", minutes, secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_syndication_token_for_known_tweet() {
        assert_eq!(
            syndication_token("2067309575573840036").unwrap(),
            "5en7dbnrp"
        );
    }

    #[test]
    fn normalizes_raw_cookie_header() {
        assert_eq!(
            normalize_cookie_input(Some("Cookie: auth_token=abc; ct0=xyz"))
                .unwrap()
                .unwrap(),
            "auth_token=abc; ct0=xyz"
        );
    }

    #[test]
    fn normalizes_netscape_cookie_file() {
        let cookies = "\
# Netscape HTTP Cookie File
#HttpOnly_.twitter.com\tTRUE\t/\tTRUE\t1893456000\tauth_token\tabc
.x.com\tTRUE\t/\tTRUE\t1893456000\tct0\txyz
.example.com\tTRUE\t/\tTRUE\t1893456000\tignored\tnope
";

        assert_eq!(
            normalize_cookie_input(Some(cookies)).unwrap().unwrap(),
            "auth_token=abc; ct0=xyz"
        );
    }

    #[test]
    fn extracts_cookie_value() {
        assert_eq!(
            cookie_value("auth_token=abc; ct0=xyz", "ct0"),
            Some("xyz".to_string())
        );
    }

    #[test]
    fn extracts_embedded_video_urls() {
        let html = r#"{"url":"https:\/\/video.twimg.com\/ext_tw_video\/1\/vid\/avc1\/320x180\/low.mp4?tag=12"},{"url":"https:\/\/video.twimg.com\/ext_tw_video\/1\/vid\/avc1\/1280x720\/high.mp4?tag=12"}"#;
        let urls = extract_video_urls_from_text(html);

        assert_eq!(urls.len(), 2);
        assert!(urls[0].contains("1280x720"));
        assert!(urls[1].contains("320x180"));
    }

    #[test]
    fn extracts_profile_username() {
        assert_eq!(
            extract_twitter_profile_username("https://x.com/nubiwubix").unwrap(),
            "nubiwubix"
        );
    }

    #[test]
    fn collects_graphql_profile_media_without_avatar() {
        let value = json!({
            "data": {
                "user_result_by_screen_name": {
                    "result": {
                        "media_timeline_v2": {
                            "timeline": {
                                "instructions": [{
                                    "entries": [{
                                        "content": {
                                            "tweet_results": {
                                                "result": [{
                                                    "core": {
                                                        "user_results": {
                                                            "result": {
                                                                "avatar": {
                                                                    "image_url": "https://pbs.twimg.com/profile_images/avatar.jpg"
                                                                }
                                                            }
                                                        }
                                                    },
                                                    "media_url_https": "https://pbs.twimg.com/media/photo.jpg?format=jpg"
                                                }, {
                                                    "media_url_https": "https://pbs.twimg.com/media/video_thumb.jpg?format=jpg",
                                                    "video_info": {
                                                        "variants": [{
                                                            "content_type": "video/mp4",
                                                            "bitrate": 832000,
                                                            "url": "https://video.twimg.com/ext_tw_video/1/vid/avc1/640x360/clip.mp4"
                                                        }, {
                                                            "content_type": "video/mp4",
                                                            "bitrate": 256000,
                                                            "url": "https://video.twimg.com/ext_tw_video/1/vid/avc1/320x180/clip.mp4"
                                                        }]
                                                    }
                                                }]
                                            }
                                        }
                                    }]
                                }]
                            }
                        }
                    }
                }
            }
        });

        let mut collection = ProfileMediaCollection::default();
        collect_graphql_profile_media_urls(&value, &mut collection);

        assert_eq!(collection.items.len(), 2);
        assert!(collection
            .items
            .iter()
            .any(|item| item.is_video && item.url.contains("video.twimg.com/ext_tw_video")));
        assert!(collection
            .items
            .iter()
            .any(|item| !item.is_video && item.url.contains("pbs.twimg.com/media/photo.jpg")));
        assert!(collection
            .items
            .iter()
            .all(|item| !item.url.contains("profile_images")));
        assert!(collection.items.iter().any(|item| item.is_video
            && item
                .thumbnail_url
                .as_deref()
                .is_some_and(|url| url.contains("pbs.twimg.com/media/video_thumb.jpg"))));
        assert!(collection
            .thumbnail_url
            .as_deref()
            .is_some_and(|url| url.contains("pbs.twimg.com/media/photo.jpg")));
    }
}
