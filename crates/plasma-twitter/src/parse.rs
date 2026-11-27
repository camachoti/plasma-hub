use super::*;

pub(crate) async fn analyze_twitter_tweet_v1(
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

pub(crate) async fn fetch_twitter_profile_graphql(
    client: &reqwest::Client,
    username: &str,
) -> Result<Value, String> {
    tracing::debug!(username = %username, "fetching Twitter profile GraphQL");
    let variables = json!({ "screenName": username });
    let url = format!(
        "https://x.com/i/api/graphql/SBM-5B0LqR_VAFawpkwGRQ/UserByScreenName?variables={}",
        encode_query_component(&variables.to_string())
    );
    fetch_twitter_graphql_json(client, url, "profile").await
}

pub(crate) async fn fetch_twitter_profile_media_graphql(
    client: &reqwest::Client,
    username: &str,
    cursor: Option<&str>,
) -> Result<Value, String> {
    if cursor.is_some() {
        tracing::debug!(
            username = %username,
            cursor = "next",
            "fetching Twitter profile media GraphQL"
        );
    } else {
        tracing::debug!(username = %username, "fetching Twitter profile media GraphQL");
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

pub(crate) async fn fetch_twitter_profile_media_pages(
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
            tracing::debug!(
                username = %username,
                "Twitter profile media pagination stopped at safety limit"
            );
        }
    }

    tracing::debug!(
        username = %username,
        items = collection.items.len(),
        "Twitter profile media collected"
    );
    Ok(collection)
}

pub(crate) async fn fetch_twitter_graphql_json(
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
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok());
    let body = response.text().await.map_err(|e| e.to_string())?;
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let wait_message = match retry_after {
            Some(seconds) if seconds > 0 => {
                let minutes = seconds.div_ceil(60);
                format!("Tente novamente em aproximadamente {minutes} min.")
            }
            _ => "Aguarde alguns minutos antes de tentar novamente.".to_string(),
        };
        return Err(format!(
            "O X limitou temporariamente as consultas ({label}, HTTP 429). {wait_message}"
        ));
    }
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

pub(crate) async fn analyze_twitter_tweet_graphql(
    client: &reqwest::Client,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    tracing::debug!(tweet_id = %tweet_id, "fetching authenticated GraphQL tweet");
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

pub(crate) async fn analyze_twitter_tweet_page(
    client: &reqwest::Client,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    tracing::debug!(tweet_id = %tweet_id, "fetching authenticated tweet page");
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

pub(crate) async fn analyze_twitter_tweet_syndication(
    client: &reqwest::Client,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    let token = syndication_token(tweet_id)?;
    tracing::debug!(tweet_id = %tweet_id, "fetching syndication tweet");
    let response = client
        .get(format!(
            "https://cdn.syndication.twimg.com/tweet-result?id={}&token={}&lang=pt",
            tweet_id, token
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        tracing::debug!(
            tweet_id = %tweet_id,
            status = %response.status(),
            "syndication returned a non-success status"
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

pub(crate) fn extract_twitter_id(url: &str) -> Result<String, String> {
    let re = Regex::new(r"(?:twitter\.com|x\.com)/(?:\w+/status|i/web/status)/(\d+)")
        .map_err(|e| e.to_string())?;
    re.captures(url)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .ok_or_else(|| "Nao foi possivel identificar o tweet".to_string())
}

pub(crate) fn extract_twitter_profile_username(url: &str) -> Result<String, String> {
    let trimmed = url.trim();
    let handle_re = Regex::new(r"^@([A-Za-z0-9_]{1,15})$").map_err(|e| e.to_string())?;
    if let Some(username) = handle_re
        .captures(trimmed)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().to_string()))
    {
        return Ok(username);
    }

    let username_re = Regex::new(r"^[A-Za-z0-9_]{1,15}$").map_err(|e| e.to_string())?;
    if username_re.is_match(trimmed) {
        return Ok(trimmed.to_string());
    }

    let re = Regex::new(r"(?:twitter\.com|x\.com)/([A-Za-z0-9_]{1,15})/?(?:\?.*)?$")
        .map_err(|e| e.to_string())?;
    re.captures(trimmed)
        .and_then(|captures| captures.get(1).map(|m| m.as_str().to_string()))
        .ok_or_else(|| "Nao foi possivel identificar o perfil".to_string())
}

pub(crate) fn build_twitter_client(cookie_header: Option<&str>) -> Result<reqwest::Client, String> {
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

pub fn normalize_cookie_input(input: Option<&str>) -> Result<Option<String>, String> {
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

pub(crate) fn looks_like_netscape_cookie_file(content: &str) -> bool {
    content
        .lines()
        .any(|line| line.split('\t').count() >= 7 && !line.trim_start().starts_with("# "))
}

pub(crate) fn netscape_cookie_header(content: &str) -> String {
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

pub(crate) fn raw_cookie_header(content: &str) -> String {
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

pub(crate) fn is_twitter_cookie_domain(domain: &str) -> bool {
    let domain = domain.trim_start_matches('.').to_ascii_lowercase();
    matches!(
        domain.as_str(),
        "twitter.com" | "x.com" | "twimg.com" | "api.twitter.com"
    ) || domain.ends_with(".twitter.com")
        || domain.ends_with(".x.com")
        || domain.ends_with(".twimg.com")
}

pub(crate) fn cookie_value(cookie_header: &str, name: &str) -> Option<String> {
    cookie_header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        if key.trim() == name {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

pub(crate) fn graphql_value_to_media_info(
    value: &Value,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    let mut variants = Vec::new();
    collect_graphql_video_variants(value, &mut variants);
    variants.sort_by_key(|b| std::cmp::Reverse(b.bitrate.unwrap_or(0)));
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

pub(crate) fn collect_graphql_video_variants(value: &Value, output: &mut Vec<TwitterVariant>) {
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

pub(crate) fn first_string_for_key(value: &Value, key: &str) -> Option<String> {
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

pub(crate) fn first_u64_for_key(value: &Value, key: &str) -> Option<u64> {
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

pub(crate) fn encode_query_component(value: &str) -> String {
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

pub(crate) fn extract_video_urls_from_text(text: &str) -> Vec<String> {
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
    urls.sort_by_key(|b| std::cmp::Reverse(video_url_height(b)));
    urls
}

pub(crate) fn extract_image_url_from_text(text: &str) -> Option<String> {
    let normalized = normalize_embedded_html(text);
    let re =
        Regex::new(r#"https://pbs\.twimg\.com/[^"'<>\s]+?(?:\.jpg|\.png|\.webp)(?:\?[^"'<>\s]+)?"#)
            .expect("valid image url regex");
    re.find(&normalized).map(|hit| hit.as_str().to_string())
}

pub(crate) fn collect_graphql_profile_media_urls(
    value: &Value,
    output: &mut ProfileMediaCollection,
) {
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

pub(crate) fn best_video_variant_url(
    video_info: &serde_json::Map<String, Value>,
) -> Option<String> {
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

pub(crate) fn push_profile_media_item(
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

pub(crate) fn normalize_twitter_photo_url(url: &str) -> String {
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

pub(crate) fn profile_media_bottom_cursor(value: &Value) -> Option<String> {
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
                .or_else(|| map.values().find_map(profile_media_bottom_cursor))
        }
        Value::Array(items) => items.iter().find_map(profile_media_bottom_cursor),
        _ => None,
    }
}

pub(crate) fn is_profile_media_timeline_terminated(value: &Value) -> bool {
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

pub(crate) fn first_string_for_pointer(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    current.as_str().map(ToString::to_string)
}

pub(crate) fn normalize_embedded_html(text: &str) -> String {
    text.replace("\\/", "/")
        .replace("&amp;", "&")
        .replace("\\u0026", "&")
        .replace("\\u003d", "=")
}

pub(crate) fn extract_meta_content(html: &str, name: &str) -> Option<String> {
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

pub(crate) fn extract_author_from_url(url: &str) -> Option<String> {
    Regex::new(r"(?:twitter\.com|x\.com)/([A-Za-z0-9_]{1,15})/status/")
        .ok()?
        .captures(url)
        .and_then(|captures| captures.get(1))
        .map(|value| value.as_str().to_string())
}

pub(crate) fn video_url_height(url: &str) -> u32 {
    Regex::new(r"/\d+x(\d+)/")
        .ok()
        .and_then(|re| re.captures(url))
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse::<u32>().ok())
        .unwrap_or(0)
}

pub(crate) fn media_extension(url: &str) -> &'static str {
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

pub(crate) fn sanitize_filename(value: &str) -> String {
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

pub(crate) fn media_info_from_video_urls(
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

pub(crate) fn syndication_token(tweet_id: &str) -> Result<String, String> {
    let id = tweet_id.parse::<f64>().map_err(|e| e.to_string())?;
    let value = (id / 1_000_000_000_000_000.0) * std::f64::consts::PI;
    let base36 = to_base36_float(value, 8);
    Ok(base36
        .chars()
        .filter(|ch| *ch != '0' && *ch != '.')
        .collect())
}

pub(crate) fn to_base36_float(value: f64, precision: usize) -> String {
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

pub(crate) fn tweet_to_media_info(tweet: TwitterTweet, original_url: String) -> NativeMediaInfo {
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
                variants.sort_by_key(|b| std::cmp::Reverse(b.bitrate.unwrap_or(0)));

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
            .map(|ms| format_duration(ms / 1000))
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

pub(crate) fn strip_tco_links(text: &str) -> String {
    let re = Regex::new(r"https?://t\.co/\S+").expect("valid t.co regex");
    re.replace_all(text, "").trim().to_string()
}

pub(crate) fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{}:{:02}", minutes, secs)
    }
}

pub(crate) async fn fetch_syndication_timeline_videos(
    client: &reqwest::Client,
    username: &str,
) -> Result<Vec<PublicTimelineVideo>, String> {
    let url =
        format!("https://syndication.twitter.com/srv/timeline-profile/screen-name/{username}");
    let response = client
        .get(&url)
        .header("accept", "text/html,application/xhtml+xml")
        .send()
        .await
        .map_err(|e| format!("X syndication indisponível: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("X syndication HTTP {}", response.status()));
    }
    let html = response
        .text()
        .await
        .map_err(|e| format!("Falha ao ler timeline pública do X: {e}"))?;
    Ok(parse_syndication_timeline_videos(&html, username))
}

pub(crate) async fn fetch_syndication_timeline_profile(
    client: &reqwest::Client,
    username: &str,
    original_url: &str,
) -> Result<NativeProfileInfo, String> {
    let videos = fetch_syndication_timeline_videos(client, username).await?;
    let thumbnail_url = videos.first().and_then(|v| v.thumbnail_url.clone());
    let media_items: Vec<NativeProfileMediaItem> = videos
        .into_iter()
        .map(|v| NativeProfileMediaItem {
            url: v.video_url,
            thumbnail_url: v.thumbnail_url,
            is_video: true,
        })
        .collect();
    let media_urls = media_items.iter().map(|i| i.url.clone()).collect();
    Ok(NativeProfileInfo {
        platform: "twitter".to_string(),
        username: username.to_string(),
        display_name: None,
        avatar_url: None,
        thumbnail_url,
        media_count: media_items.len(),
        media_urls,
        media_items,
        original_url: original_url.to_string(),
    })
}

pub(crate) fn parse_syndication_timeline_videos(
    html: &str,
    fallback_username: &str,
) -> Vec<PublicTimelineVideo> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(next_data) = extract_next_data_json(html) {
        if let Some(entries) = next_data
            .pointer("/props/pageProps/timeline/entries")
            .and_then(Value::as_array)
        {
            for entry in entries {
                let Some(tweet) = entry.pointer("/content/tweet") else {
                    continue;
                };
                if let Some(video) = video_from_syndication_entry(tweet, fallback_username) {
                    if seen.insert(video.video_url.clone()) {
                        out.push(video);
                    }
                }
                if out.len() >= 12 {
                    break;
                }
            }
        }
    }

    if out.is_empty() {
        for (idx, url) in extract_video_urls_from_text(html)
            .into_iter()
            .take(8)
            .enumerate()
        {
            if seen.insert(url.clone()) {
                out.push(PublicTimelineVideo {
                    id: format!("{fallback_username}-embed-{idx}"),
                    title: format!("@{fallback_username}"),
                    author: format!("@{fallback_username}"),
                    page_url: format!("https://x.com/{fallback_username}"),
                    video_url: url,
                    thumbnail_url: None,
                });
            }
        }
    }

    out
}

fn extract_next_data_json(html: &str) -> Option<Value> {
    let marker = r#"<script id="__NEXT_DATA__" type="application/json">"#;
    let start = html.find(marker)? + marker.len();
    let rest = &html[start..];
    let end = rest.find("</script>")?;
    serde_json::from_str(&rest[..end]).ok()
}

fn video_from_syndication_entry(
    tweet: &Value,
    fallback_username: &str,
) -> Option<PublicTimelineVideo> {
    let id = tweet
        .get("id_str")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())?
        .to_string();
    let screen_name = tweet
        .pointer("/user/screen_name")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback_username);
    let raw_text = tweet
        .get("full_text")
        .or_else(|| tweet.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let cleaned = strip_tco_links(raw_text);
    let title = if cleaned.is_empty() {
        format!("@{screen_name}")
    } else {
        cleaned.chars().take(180).collect()
    };

    let media_list = tweet
        .pointer("/extended_entities/media")
        .or_else(|| tweet.pointer("/entities/media"))
        .and_then(Value::as_array)?;

    for media in media_list {
        let variants = media
            .pointer("/video_info/variants")
            .and_then(Value::as_array);
        let Some(variants) = variants else {
            continue;
        };
        let best_mp4 = variants
            .iter()
            .filter_map(|variant| {
                let ctype = variant.get("content_type")?.as_str()?;
                if ctype != "video/mp4" {
                    return None;
                }
                let url = variant.get("url")?.as_str()?;
                if !url.starts_with("https://") {
                    return None;
                }
                let bitrate = variant.get("bitrate").and_then(Value::as_u64).unwrap_or(0);
                Some((bitrate, url.to_string()))
            })
            .max_by_key(|(bitrate, _)| *bitrate)
            .map(|(_, url)| url);

        if let Some(video_url) = best_mp4 {
            let thumbnail_url = media
                .get("media_url_https")
                .and_then(Value::as_str)
                .filter(|u| u.starts_with("https://"))
                .map(str::to_string);
            return Some(PublicTimelineVideo {
                id,
                title,
                author: format!("@{screen_name}"),
                page_url: format!(
                    "https://x.com/{screen_name}/status/{}",
                    tweet.get("id_str").and_then(Value::as_str).unwrap_or("")
                ),
                video_url,
                thumbnail_url,
            });
        }
    }
    None
}

pub(crate) async fn analyze_twitter_tweet_fx(
    client: &reqwest::Client,
    tweet_id: &str,
    original_url: &str,
) -> Result<NativeMediaInfo, String> {
    let response = client
        .get(format!("https://api.fxtwitter.com/status/{tweet_id}"))
        .header("accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("FxTwitter HTTP {}", response.status()));
    }
    let data: Value = response.json().await.map_err(|e| e.to_string())?;
    let tweet = data
        .get("tweet")
        .ok_or_else(|| "FxTwitter sem dados de tweet".to_string())?;
    let screen_name = tweet
        .pointer("/author/screen_name")
        .and_then(Value::as_str)
        .unwrap_or("x");
    let author_name = tweet
        .pointer("/author/name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let text = tweet
        .get("text")
        .and_then(Value::as_str)
        .map(strip_tco_links)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| format!("@{screen_name}"));
    let videos = tweet
        .pointer("/media/videos")
        .and_then(Value::as_array)
        .ok_or_else(|| "Tweet público sem vídeo direto".to_string())?;
    let first = videos
        .first()
        .ok_or_else(|| "Tweet público sem vídeo".to_string())?;
    let video_url = first
        .get("url")
        .and_then(Value::as_str)
        .filter(|u| u.starts_with("https://"))
        .ok_or_else(|| "URL de vídeo ausente".to_string())?;
    let thumbnail_url = first
        .get("thumbnail_url")
        .and_then(Value::as_str)
        .filter(|u| u.starts_with("https://"))
        .map(str::to_string);
    let duration = first
        .get("duration")
        .and_then(Value::as_f64)
        .map(|d| format_duration(d.round() as u64))
        .unwrap_or_else(|| "—".to_string());

    Ok(NativeMediaInfo {
        platform: "twitter".to_string(),
        title: text,
        author: format!("@{screen_name}"),
        author_full: author_name,
        duration,
        thumb_hue: 205,
        thumb_hue2: 225,
        thumbnail_url,
        original_url: original_url.to_string(),
        formats: NativeFormats {
            video: vec![NativeFormatOption {
                id: "twitter-hd".to_string(),
                label: "HD".to_string(),
                sub: "MP4 · X/Twitter".to_string(),
                size: "—".to_string(),
                best: Some(true),
                url: Some(video_url.to_string()),
            }],
            audio: Vec::new(),
        },
    })
}
