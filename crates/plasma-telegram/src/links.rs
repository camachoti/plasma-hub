//! Telegram `t.me` and `tg://` links that the shell can open inside the app.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MessageLinkFallback {
    pub chat_id: Option<i64>,
    pub username: Option<String>,
    pub topic_id: Option<i64>,
    pub topic_kind: Option<String>,
    pub message_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ParsedTelegramLink {
    Invite {
        link: String,
    },
    Message {
        canonical: String,
        fallback: MessageLinkFallback,
    },
}

/// True when the shell should handle the URL instead of the system browser.
pub fn looks_like_telegram_link(url: &str) -> bool {
    parse_telegram_link(url).is_some()
}

pub(crate) fn parse_telegram_link(raw: &str) -> Option<ParsedTelegramLink> {
    let trimmed = raw.trim();
    let trimmed = trimmed.trim_end_matches(|ch: char| {
        matches!(
            ch,
            ')' | ']' | '}' | '"' | '\'' | ',' | '.' | ';' | '!' | '?'
        )
    });
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("tg://") {
        return parse_tg_scheme(trimmed);
    }
    let body = strip_http_scheme(trimmed);
    let (host, path) = body.split_once('/')?;
    let host = host
        .split('@')
        .next_back()
        .unwrap_or(host)
        .split(':')
        .next()
        .unwrap_or(host);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if !matches!(
        host.to_ascii_lowercase().as_str(),
        "t.me" | "telegram.me" | "telegram.dog"
    ) {
        return None;
    }
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    parse_path(&segments)
}

fn strip_http_scheme(value: &str) -> &str {
    let lower = value.to_ascii_lowercase();
    for prefix in ["https://", "http://"] {
        if lower.starts_with(prefix) {
            return &value[prefix.len()..];
        }
    }
    value
}

fn parse_tg_scheme(value: &str) -> Option<ParsedTelegramLink> {
    let body = &value[5..];
    let (kind, query) = body.split_once('?').unwrap_or((body, ""));
    let params = query_pairs(query);
    match kind.to_ascii_lowercase().as_str() {
        "join" => {
            let invite = params.iter().find(|(key, _)| key == "invite")?.1.clone();
            let invite = percent_decode(&invite);
            if invite.is_empty() {
                return None;
            }
            Some(ParsedTelegramLink::Invite {
                link: format!("https://t.me/+{invite}"),
            })
        }
        "privatepost" => {
            let channel = params
                .iter()
                .find(|(key, _)| key == "channel")?
                .1
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)?;
            let message_id = params
                .iter()
                .find(|(key, _)| key == "post")?
                .1
                .parse::<i64>()
                .ok()
                .filter(|id| *id > 0)?;
            let topic_id = params
                .iter()
                .find(|(key, _)| key == "thread")
                .and_then(|(_, value)| value.parse::<i64>().ok())
                .filter(|id| *id > 0);
            private_message(channel, topic_id, message_id)
        }
        "resolve" => {
            let username = params.iter().find(|(key, _)| key == "domain")?.1.clone();
            let username = percent_decode(&username);
            let message_id = params
                .iter()
                .find(|(key, _)| key == "post")
                .and_then(|(_, value)| value.parse::<i64>().ok())
                .filter(|id| *id > 0);
            let topic_id = params
                .iter()
                .find(|(key, _)| key == "thread")
                .and_then(|(_, value)| value.parse::<i64>().ok())
                .filter(|id| *id > 0);
            public_message(&username, topic_id, message_id)
        }
        _ => None,
    }
}

fn parse_path(segments: &[&str]) -> Option<ParsedTelegramLink> {
    let first = segments.first()?.trim();
    if first.is_empty() {
        return None;
    }
    if let Some(hash) = first.strip_prefix('+') {
        if hash.is_empty() {
            return None;
        }
        return Some(ParsedTelegramLink::Invite {
            link: format!("https://t.me/+{hash}"),
        });
    }
    if first.eq_ignore_ascii_case("joinchat") {
        let hash = segments.get(1)?.trim();
        if hash.is_empty() {
            return None;
        }
        return Some(ParsedTelegramLink::Invite {
            link: format!("https://t.me/+{hash}"),
        });
    }
    if first.eq_ignore_ascii_case("c") {
        let channel = segments.get(1)?.parse::<i64>().ok().filter(|id| *id > 0)?;
        let second = segments.get(2)?.parse::<i64>().ok().filter(|id| *id > 0)?;
        let third = segments
            .get(3)
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|id| *id > 0);
        return match third {
            Some(message_id) => private_message(channel, Some(second), message_id),
            None => private_message(channel, None, second),
        };
    }
    if first.eq_ignore_ascii_case("s") {
        let username = segments.get(1)?;
        let message_id = segments
            .get(2)
            .and_then(|value| value.parse::<i64>().ok())
            .filter(|id| *id > 0);
        return public_message(username, None, message_id);
    }
    if is_reserved(first) || !is_username(first) {
        return None;
    }
    let second = segments
        .get(1)
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|id| *id > 0);
    let third = segments
        .get(2)
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|id| *id > 0);
    match (second, third) {
        (Some(topic_id), Some(message_id)) => {
            public_message(first, Some(topic_id), Some(message_id))
        }
        (Some(message_id), None) => public_message(first, None, Some(message_id)),
        _ => public_message(first, None, None),
    }
}

fn private_message(
    channel_id: i64,
    topic_id: Option<i64>,
    message_id: i64,
) -> Option<ParsedTelegramLink> {
    let chat_id = private_chat_id(channel_id)?;
    let canonical = match topic_id {
        Some(topic_id) => format!("https://t.me/c/{channel_id}/{topic_id}/{message_id}"),
        None => format!("https://t.me/c/{channel_id}/{message_id}"),
    };
    Some(ParsedTelegramLink::Message {
        canonical,
        fallback: MessageLinkFallback {
            chat_id: Some(chat_id),
            username: None,
            topic_id,
            topic_kind: topic_id.map(|_| "forum".to_string()),
            message_id: Some(message_id),
        },
    })
}

fn public_message(
    username: &str,
    topic_id: Option<i64>,
    message_id: Option<i64>,
) -> Option<ParsedTelegramLink> {
    if !is_username(username) {
        return None;
    }
    let mut canonical = format!("https://t.me/{username}");
    if let Some(topic_id) = topic_id {
        canonical.push('/');
        canonical.push_str(&topic_id.to_string());
    }
    if let Some(message_id) = message_id {
        canonical.push('/');
        canonical.push_str(&message_id.to_string());
    }
    Some(ParsedTelegramLink::Message {
        canonical,
        fallback: MessageLinkFallback {
            chat_id: None,
            username: Some(username.to_string()),
            topic_id,
            topic_kind: topic_id.map(|_| "forum".to_string()),
            message_id,
        },
    })
}

/// `t.me/c/<id>` drops the `-100` prefix of the supergroup id.
pub(crate) fn private_chat_id(channel_id: i64) -> Option<i64> {
    if channel_id <= 0 {
        return None;
    }
    1_000_000_000_000_i64.checked_add(channel_id).map(|id| -id)
}

fn is_reserved(segment: &str) -> bool {
    matches!(
        segment.to_ascii_lowercase().as_str(),
        "share"
            | "addstickers"
            | "addtheme"
            | "addemoji"
            | "proxy"
            | "socks"
            | "iv"
            | "login"
            | "bg"
            | "invoice"
            | "confirmphone"
            | "boost"
            | "giftcode"
            | "m"
            | "web"
            | "contact"
    )
}

fn is_username(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_alphabetic() {
        return false;
    }
    chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && (1..=32).contains(&value.chars().count())
}

fn query_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter_map(|part| {
            let (key, value) = part.split_once('=')?;
            Some((key.to_ascii_lowercase(), percent_decode(value)))
        })
        .collect()
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        if bytes[index] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[index]);
        }
        index += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_forum_link_carries_topic_and_message() {
        let parsed = parse_telegram_link("https://t.me/c/3977858280/18/601").unwrap();
        match parsed {
            ParsedTelegramLink::Message {
                canonical,
                fallback,
            } => {
                assert_eq!(canonical, "https://t.me/c/3977858280/18/601");
                assert_eq!(fallback.chat_id, Some(-1_003_977_858_280));
                assert_eq!(fallback.topic_id, Some(18));
                assert_eq!(fallback.topic_kind.as_deref(), Some("forum"));
                assert_eq!(fallback.message_id, Some(601));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn private_message_link_has_no_topic() {
        let parsed = parse_telegram_link("t.me/c/3977858280/601.").unwrap();
        match parsed {
            ParsedTelegramLink::Message { fallback, .. } => {
                assert_eq!(fallback.topic_id, None);
                assert_eq!(fallback.message_id, Some(601));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn invite_and_public_links() {
        assert!(matches!(
            parse_telegram_link("https://t.me/+AbCdEf"),
            Some(ParsedTelegramLink::Invite { .. })
        ));
        assert!(matches!(
            parse_telegram_link("https://t.me/joinchat/AbCdEf"),
            Some(ParsedTelegramLink::Invite { .. })
        ));
        let parsed = parse_telegram_link("https://t.me/durov/12/34").unwrap();
        match parsed {
            ParsedTelegramLink::Message { fallback, .. } => {
                assert_eq!(fallback.username.as_deref(), Some("durov"));
                assert_eq!(fallback.topic_id, Some(12));
                assert_eq!(fallback.message_id, Some(34));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn tg_scheme_private_post() {
        let parsed =
            parse_telegram_link("tg://privatepost?channel=3977858280&post=601&thread=18").unwrap();
        match parsed {
            ParsedTelegramLink::Message { fallback, .. } => {
                assert_eq!(fallback.chat_id, Some(-1_003_977_858_280));
                assert_eq!(fallback.topic_id, Some(18));
                assert_eq!(fallback.message_id, Some(601));
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
