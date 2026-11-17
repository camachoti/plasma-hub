//! Album grouping and height estimates for the virtual timeline.

use plasma_telegram::TdlibMessageInfo;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;
use time::OffsetDateTime;

const ALBUM_CELL_HEIGHT: f64 = 104.0;
const OVERSCAN: usize = 4;
const ALBUM_GAP: f64 = 4.0;

/// A row of the timeline: a single message or a grouped album.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum TimelineItem {
    Message(Box<TdlibMessageInfo>),
    Album(Vec<TdlibMessageInfo>),
}

impl TimelineItem {
    pub(super) fn key(&self) -> String {
        match self {
            TimelineItem::Message(message) => message.id.to_string(),
            TimelineItem::Album(messages) => format!("album-{}", messages[0].id),
        }
    }

    /// Whether this row contains the given Telegram message id.
    pub(super) fn contains(&self, message_id: i64) -> bool {
        match self {
            TimelineItem::Message(message) => message.id == message_id,
            TimelineItem::Album(messages) => messages.iter().any(|item| item.id == message_id),
        }
    }
}

/// Measurement keys are scoped by chat: Telegram message ids are only unique
/// inside a conversation, so the same id in two chats must not share a height.
pub(super) fn scoped_key(chat_id: i64, item: &TimelineItem) -> String {
    format!("{chat_id}:{}", item.key())
}

/// Collapses consecutive messages that share a `grouped_id` into albums.
pub(super) fn group_albums(messages: &[TdlibMessageInfo]) -> Vec<TimelineItem> {
    let mut items = Vec::new();
    let mut index = 0;
    while index < messages.len() {
        let message = &messages[index];
        let group = message
            .grouped_id
            .as_deref()
            .filter(|group| !group.is_empty());
        let Some(group) = group else {
            items.push(TimelineItem::Message(Box::new(message.clone())));
            index += 1;
            continue;
        };
        let mut end = index + 1;
        while end < messages.len() && messages[end].grouped_id.as_deref() == Some(group) {
            end += 1;
        }
        if end - index == 1 {
            items.push(TimelineItem::Message(Box::new(message.clone())));
        } else {
            items.push(TimelineItem::Album(messages[index..end].to_vec()));
        }
        index = end;
    }
    items
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct TimelineLayout {
    pub(super) offsets: Vec<f64>,
    pub(super) heights: Vec<f64>,
    pub(super) total_height: f64,
}

pub(super) fn build_layout(
    items: &[TimelineItem],
    measured: &HashMap<String, f64>,
    chat_id: i64,
) -> TimelineLayout {
    let mut offsets = Vec::with_capacity(items.len());
    let mut heights = Vec::with_capacity(items.len());
    let mut top = 0.0;
    for item in items {
        offsets.push(top);
        // Rendered rows are measured by the WebView; estimates only cover rows
        // that have never been on screen. This keeps long messages from being
        // clipped by the virtualized canvas.
        let height = measured
            .get(&scoped_key(chat_id, item))
            .copied()
            .unwrap_or_else(|| estimate_item_height(item));
        heights.push(height);
        top += height;
    }
    TimelineLayout {
        offsets,
        heights,
        total_height: top,
    }
}

/// Returns the canvas offset of the row that contains a message, if that
/// message is part of the loaded page.
pub(super) fn message_offset(
    items: &[TimelineItem],
    layout: &TimelineLayout,
    message_id: i64,
) -> Option<f64> {
    items
        .iter()
        .position(|item| item.contains(message_id))
        .map(|index| layout.offsets[index])
}

/// Finds the loaded message quoted by a reply, so the timeline can show a
/// Telegram-style preview instead of a bare "resposta" label.
pub(super) fn reply_preview(items: &[TimelineItem], message_id: i64) -> Option<&TdlibMessageInfo> {
    items.iter().find_map(|item| match item {
        TimelineItem::Message(message) => (message.id == message_id).then_some(message.as_ref()),
        TimelineItem::Album(messages) => messages.iter().find(|message| message.id == message_id),
    })
}

pub(crate) fn reply_sender(message: &TdlibMessageInfo) -> String {
    if message.out {
        return "Você".to_string();
    }
    let name = message.sender_name.trim();
    if name.is_empty() {
        "Mensagem".to_string()
    } else {
        name.to_string()
    }
}

pub(crate) fn reply_snippet(message: &TdlibMessageInfo) -> String {
    let text = message
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() {
        return if message.is_photo {
            "Foto".to_string()
        } else if message.is_video {
            "Vídeo".to_string()
        } else if message.has_media {
            "Mídia".to_string()
        } else {
            "Mensagem sem texto".to_string()
        };
    }
    let mut characters = text.chars();
    let mut snippet: String = characters.by_ref().take(110).collect();
    if characters.next().is_some() {
        snippet.push('…');
    }
    snippet
}

#[derive(Deserialize)]
pub(super) struct RowMeasurements {
    pub(super) heights: HashMap<String, f64>,
    #[serde(default, rename = "deltaAbove")]
    pub(super) delta_above: f64,
    #[serde(default, rename = "atBottom")]
    pub(super) at_bottom: bool,
}

/// Number of grid rows used by an album grid.
pub(super) fn album_rows(count: usize) -> usize {
    if count <= 2 {
        1
    } else {
        count.div_ceil(2)
    }
}

pub(super) fn estimate_item_height(item: &TimelineItem) -> f64 {
    match item {
        TimelineItem::Message(message) => estimate_height(message),
        TimelineItem::Album(messages) => {
            let characters = messages
                .iter()
                .find(|message| !message.text.trim().is_empty())
                .map(|message| message.text.chars().count())
                .unwrap_or(0);
            let lines = if characters == 0 {
                0.0
            } else {
                ((characters as f64) / 74.0).ceil()
            };
            let rows = album_rows(messages.len());
            let grid =
                rows as f64 * ALBUM_CELL_HEIGHT + (rows.saturating_sub(1)) as f64 * ALBUM_GAP;
            64.0 + lines * 19.0 + 8.0 + grid
        }
    }
}

pub(super) fn estimate_height(message: &TdlibMessageInfo) -> f64 {
    if message.is_deleted {
        return 60.0;
    }
    let characters = message.text.chars().count();
    let lines = ((characters as f64) / 74.0).ceil().max(1.0);
    let mut height = 64.0 + lines * 19.0;
    if message.has_media {
        height += if message.is_photo || message.is_video {
            136.0
        } else {
            24.0
        };
    }
    if message.reply_to_msg_id.is_some() {
        // The quoted reply block shows the author and a text snippet.
        height += 44.0;
    }
    if !message.reactions.is_empty() {
        height += 26.0;
    }
    height.max(64.0)
}

pub(super) fn visible_range(
    offsets: &[f64],
    scroll_top: f64,
    viewport_height: f64,
    row_count: usize,
) -> (usize, usize) {
    if row_count == 0 {
        return (0, 0);
    }
    let start = offsets
        .partition_point(|offset| *offset < scroll_top)
        .saturating_sub(1 + OVERSCAN);
    let bottom = scroll_top + viewport_height.max(1.0);
    let end = (offsets.partition_point(|offset| *offset < bottom) + OVERSCAN).min(row_count);
    (start.min(end), end)
}

/// Resolving the local offset reads the OS time zone configuration, which is
/// too expensive to repeat for every visible message. Cache it for the session.
pub(super) fn local_offset() -> time::UtcOffset {
    static OFFSET: OnceLock<time::UtcOffset> = OnceLock::new();
    *OFFSET.get_or_init(|| time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC))
}

pub(crate) fn format_timestamp(timestamp: i32) -> String {
    if timestamp <= 0 {
        return String::new();
    }
    let Ok(utc) = OffsetDateTime::from_unix_timestamp(timestamp as i64) else {
        return String::new();
    };
    let offset = local_offset();
    let local = utc.to_offset(offset);
    let now = OffsetDateTime::now_utc().to_offset(offset);
    if local.date() == now.date() {
        format!("{:02}:{:02}", local.hour(), local.minute())
    } else if local.year() == now.year() {
        format!("{:02}/{:02}", u8::from(local.month()), local.day())
    } else {
        format!(
            "{:02}/{:02}/{:04}",
            u8::from(local.month()),
            local.day(),
            local.year()
        )
    }
}

pub(super) fn format_size(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes = bytes.max(0) as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

pub(super) fn media_label(message: &TdlibMessageInfo) -> Option<String> {
    if !message.has_media {
        return None;
    }
    let kind = if message.is_photo {
        "Foto"
    } else if message.is_video {
        "Vídeo"
    } else {
        "Mídia"
    };
    let duration = message
        .video_duration
        .filter(|duration| *duration > 0)
        .map(|duration| format!(" · {}:{:02}", duration / 60, duration % 60))
        .unwrap_or_default();
    let size = message
        .media_size
        .filter(|size| *size > 0)
        .map(|size| format!(" · {}", format_size(size)))
        .unwrap_or_default();
    Some(format!("{kind}{duration}{size}"))
}
