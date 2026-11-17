mod cookies;
mod download;
mod parse;

pub use download::{download_twitter_native, download_twitter_profile_native};
use parse::*;

pub use cookies::{load_twitter_cookies, save_twitter_cookies, TWITTER_COOKIES_FILE};
pub use parse::normalize_cookie_input;

use plasma_core::AppEnv;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, COOKIE, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use plasma_download::{is_cancellation_error, DownloadCancellation, DOWNLOAD_CANCELED_ERROR};

const TWITTER_BEARER: &str = "AAAAAAAAAAAAAAAAAAAAANRILgAAAAAAnNwIzUejRCOuH5E6I8xnZz4puTs%3D1Zv7ttfk8LF81IUq16cHjhLTvJu4FA33AGWWjCpTnA";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeMediaInfo {
    pub platform: String,
    pub title: String,
    pub author: String,
    pub author_full: Option<String>,
    pub duration: String,
    pub thumb_hue: u16,
    pub thumb_hue2: u16,
    pub thumbnail_url: Option<String>,
    pub original_url: String,
    pub formats: NativeFormats,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NativeFormats {
    pub video: Vec<NativeFormatOption>,
    pub audio: Vec<NativeFormatOption>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeFormatOption {
    pub id: String,
    pub label: String,
    pub sub: String,
    pub size: String,
    pub best: Option<bool>,
    pub url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeProfileInfo {
    pub platform: String,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub media_count: usize,
    pub media_urls: Vec<String>,
    pub media_items: Vec<NativeProfileMediaItem>,
    pub original_url: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeProfileMediaItem {
    pub url: String,
    pub thumbnail_url: Option<String>,
    pub is_video: bool,
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

pub async fn analyze_twitter_profile_native(
    url: String,
    cookies: Option<String>,
) -> Result<NativeProfileInfo, String> {
    let username = extract_twitter_profile_username(&url)?;
    tracing::debug!(username = %username, "analyzing Twitter profile");
    let cookie_header = normalize_cookie_input(cookies.as_deref())?;
    if cookie_header.is_some() {
        tracing::debug!("using Twitter cookies for profile analyzer");
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

pub async fn analyze_twitter_tweet_native(
    url: String,
    cookies: Option<String>,
) -> Result<NativeMediaInfo, String> {
    let tweet_id = extract_twitter_id(&url)?;
    tracing::debug!(tweet_id = %tweet_id, "analyzing Twitter tweet");
    let cookie_header = normalize_cookie_input(cookies.as_deref())?;
    if cookie_header.is_some() {
        tracing::debug!("using Twitter cookies for tweet analyzer");
    }
    let client = build_twitter_client(cookie_header.as_deref())?;

    if cookie_header.is_some() {
        match analyze_twitter_tweet_graphql(&client, &tweet_id, &url).await {
            Ok(info) => return Ok(info),
            Err(graphql_error) => {
                tracing::debug!(
                    tweet_id = %tweet_id,
                    error = %graphql_error,
                    "authenticated GraphQL analyzer failed"
                );
            }
        }

        match analyze_twitter_tweet_page(&client, &tweet_id, &url).await {
            Ok(info) => return Ok(info),
            Err(page_error) => {
                tracing::debug!(
                    tweet_id = %tweet_id,
                    error = %page_error,
                    "authenticated page analyzer failed"
                );
            }
        }
    }

    match analyze_twitter_tweet_v1(&client, &tweet_id, &url).await {
        Ok(info) => Ok(info),
        Err(v1_error) => {
            tracing::debug!(
                tweet_id = %tweet_id,
                error = %v1_error,
                "v1 analyzer failed; trying syndication"
            );
            analyze_twitter_tweet_syndication(&client, &tweet_id, &url)
                .await
                .map_err(|syndication_error| {
                    tracing::debug!(
                        tweet_id = %tweet_id,
                        error = %syndication_error,
                        "syndication analyzer failed"
                    );
                    format!(
                        "Twitter API falhou: {}. Syndication falhou: {}",
                        v1_error, syndication_error
                    )
                })
        }
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
