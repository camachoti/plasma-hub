//! Variable-height virtual timeline for real Telegram messages.

use crate::state::AppServices;
use dioxus::prelude::*;
use plasma_telegram::TdlibMessageInfo;
use time::OffsetDateTime;

const OVERSCAN: usize = 6;
const DEFAULT_VIEWPORT: f64 = 620.0;
const TIMELINE_ID: &str = "plasma-timeline-scroll";

#[derive(Clone, Debug, PartialEq)]
struct TimelineLayout {
    offsets: Vec<f64>,
    heights: Vec<f64>,
    total_height: f64,
}

fn build_layout(messages: &[TdlibMessageInfo]) -> TimelineLayout {
    let mut offsets = Vec::with_capacity(messages.len());
    let mut heights = Vec::with_capacity(messages.len());
    let mut top = 0.0;
    for message in messages {
        offsets.push(top);
        let height = estimate_height(message);
        heights.push(height);
        top += height;
    }
    TimelineLayout {
        offsets,
        heights,
        total_height: top,
    }
}

fn estimate_height(message: &TdlibMessageInfo) -> f64 {
    if message.is_deleted {
        return 58.0;
    }
    let characters = message.text.chars().count();
    let lines = ((characters as f64) / 74.0).ceil().max(1.0);
    let mut height = 58.0 + lines * 18.0;
    if message.has_media {
        height += if message.is_photo || message.is_video {
            132.0
        } else {
            24.0
        };
    }
    if message.reply_to_msg_id.is_some() {
        height += 18.0;
    }
    height.max(58.0)
}

fn visible_range(
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

pub(crate) fn format_timestamp(timestamp: i32) -> String {
    if timestamp <= 0 {
        return String::new();
    }
    let Ok(utc) = OffsetDateTime::from_unix_timestamp(timestamp as i64) else {
        return String::new();
    };
    let offset = time::UtcOffset::current_local_offset().unwrap_or(time::UtcOffset::UTC);
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

fn format_size(bytes: i64) -> String {
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

fn media_label(message: &TdlibMessageInfo) -> Option<String> {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageAction {
    Reply,
    OpenMedia,
    Forward,
}

#[component]
pub(crate) fn MessageThumbnail(
    chat_id: i64,
    message_id: i64,
    is_video: bool,
    on_open: EventHandler<()>,
) -> Element {
    let services = use_context::<AppServices>();
    let resource = use_resource(move || {
        let services = services.clone();
        async move {
            services
                .telegram
                .thumbnail_data_url(chat_id, message_id)
                .await
        }
    });

    rsx! {
        button {
            r#type: "button",
            class: "message-thumb",
            title: "Abrir mídia",
            onclick: move |_| on_open.call(()),
            match &*resource.read_unchecked() {
                Some(Ok(Some(url))) => rsx! { img { src: "{url}", alt: "" } },
                Some(Ok(None)) => rsx! { span { class: "thumb-placeholder", "▣" } },
                Some(Err(error)) => rsx! { span { class: "thumb-placeholder", title: "{error}", "!" } },
                None => rsx! { span { class: "thumb-placeholder", "…" } },
            }
            if is_video {
                span { class: "thumb-play", "▶" }
            }
        }
    }
}

#[component]
pub fn MessageTimeline(
    chat_id: i64,
    messages: Vec<TdlibMessageInfo>,
    empty_label: String,
    on_action: EventHandler<(TdlibMessageInfo, MessageAction)>,
) -> Element {
    let layout = build_layout(&messages);
    let mut scroll_top = use_signal(|| 0.0);
    let mut viewport_height = use_signal(|| DEFAULT_VIEWPORT);
    let mut last_marker = use_signal(|| None::<i64>);

    let range = visible_range(
        &layout.offsets,
        scroll_top(),
        viewport_height(),
        messages.len(),
    );
    let (start, end) = range;
    let rows: Vec<(TdlibMessageInfo, f64, f64)> = (start..end)
        .map(|index| {
            (
                messages[index].clone(),
                layout.offsets[index],
                layout.heights[index],
            )
        })
        .collect();

    // Only stick to the bottom when the newest message changes. Prepending
    // older history keeps the current scroll position intact.
    let marker = messages.last().map(|message| message.id);
    use_effect(move || {
        if marker.is_none() || marker == last_marker() {
            return;
        }
        last_marker.set(marker);
        spawn(async move {
            let _ = document::eval(&format!(
                r#"const element = document.getElementById("{TIMELINE_ID}");
                if (element) {{ element.scrollTop = element.scrollHeight; }}"#
            ))
            .await;
        });
    });

    rsx! {
        if messages.is_empty() {
            div { class: "empty-state",
                div { class: "empty-card",
                    div { class: "empty-icon", "…" }
                    h2 { "{empty_label}" }
                    p { "Nenhuma mensagem disponível nesta conversa." }
                }
            }
        } else {
            div {
                id: TIMELINE_ID,
                class: "message-scroll",
                onscroll: move |event| {
                    scroll_top.set(event.scroll_top());
                    viewport_height.set(event.client_height() as f64);
                },
                div {
                    class: "message-canvas",
                    style: "height: {layout.total_height}px;",
                    for (message, top, height) in rows {
                        article {
                            key: "{message.id}",
                            class: if message.is_deleted {
                                "message-row deleted"
                            } else if message.out {
                                "message-row own"
                            } else {
                                "message-row"
                            },
                            style: "top: {top}px; height: {height}px;",
                            div { class: "avatar",
                                "{message.sender_name.chars().next().unwrap_or('?')}"
                            }
                            div { class: "message-bubble",
                                div { class: "message-meta",
                                    strong {
                                        if message.sender_name.trim().is_empty() {
                                            "Mensagem"
                                        } else {
                                            "{message.sender_name}"
                                        }
                                    }
                                    span { "{format_timestamp(message.date)}" }
                                    if message.out {
                                        span { class: "pill", "enviada" }
                                    }
                                }
                                if message.reply_to_msg_id.is_some() {
                                    span { class: "message-reply", "↳ resposta" }
                                }
                                if message.is_deleted {
                                    p { class: "message-text message-deleted", "Mensagem apagada" }
                                } else {
                                    if !message.text.trim().is_empty() {
                                        p { class: "message-text", "{message.text}" }
                                    }
                                    if message.has_media && (message.is_photo || message.is_video) {
                                        MessageThumbnail {
                                            chat_id,
                                            message_id: message.id,
                                            is_video: message.is_video,
                                            on_open: {
                                                let message = message.clone();
                                                move |_| on_action.call((message.clone(), MessageAction::OpenMedia))
                                            },
                                        }
                                    }
                                    if let Some(media) = media_label(&message) {
                                        div { class: "message-media",
                                            span { "▣" }
                                            span { "{media}" }
                                            if message.thumbnail_path.is_some() {
                                                span { class: "pill", "miniatura" }
                                            }
                                        }
                                    }
                                }
                                div { class: "message-actions",
                                    button {
                                        class: "msg-action",
                                        title: "Responder",
                                        onclick: {
                                            let message = message.clone();
                                            move |_| on_action.call((message.clone(), MessageAction::Reply))
                                        },
                                        "Responder"
                                    }
                                    if message.has_media {
                                        button {
                                            class: "msg-action",
                                            onclick: {
                                                let message = message.clone();
                                                move |_| on_action.call((message.clone(), MessageAction::OpenMedia))
                                            },
                                            "Abrir"
                                        }
                                    }
                                    button {
                                        class: "msg-action",
                                        onclick: {
                                            let message = message.clone();
                                            move |_| on_action.call((message.clone(), MessageAction::Forward))
                                        },
                                        "Encaminhar"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id: i64, date: i32, text: &str) -> TdlibMessageInfo {
        TdlibMessageInfo {
            id,
            message: String::new(),
            date,
            out: false,
            sender_id: String::new(),
            sender_name: "Ana".to_string(),
            reply_to_msg_id: None,
            topic_id: None,
            topic_kind: None,
            media: false,
            text: text.to_string(),
            has_media: false,
            is_photo: false,
            is_video: false,
            grouped_id: None,
            video_duration: None,
            media_size: None,
            thumbnail_path: None,
            is_deleted: false,
        }
    }

    #[test]
    fn offsets_are_cumulative_and_total_matches() {
        let messages = vec![message(1, 1, "curta"), message(2, 2, &"a".repeat(300))];
        let layout = build_layout(&messages);
        assert_eq!(layout.offsets[0], 0.0);
        assert_eq!(layout.offsets[1], layout.heights[0]);
        assert_eq!(layout.total_height, layout.heights.iter().sum::<f64>());
    }

    #[test]
    fn visible_range_is_bounded_at_both_ends() {
        let messages: Vec<_> = (0..100)
            .map(|index| message(index, index as i32, "mensagem"))
            .collect();
        let layout = build_layout(&messages);
        let (start, end) = visible_range(&layout.offsets, 0.0, 620.0, messages.len());
        assert_eq!(start, 0);
        assert!(end > start);

        let (start, end) = visible_range(
            &layout.offsets,
            layout.total_height - 620.0,
            620.0,
            messages.len(),
        );
        assert!(start < messages.len());
        assert_eq!(end, messages.len());
    }

    #[test]
    fn empty_range_is_safe() {
        assert_eq!(visible_range(&[], 0.0, 620.0, 0), (0, 0));
    }

    #[test]
    fn media_and_replies_add_height() {
        let mut base = message(1, 1, "oi");
        let base_height = estimate_height(&base);
        base.has_media = true;
        base.reply_to_msg_id = Some(0);
        assert!(estimate_height(&base) > base_height);
    }

    #[test]
    fn thumbnails_reserve_more_room_and_deleted_rows_collapse() {
        let mut message = message(1, 1, "foto");
        message.has_media = true;
        let media_height = estimate_height(&message);
        message.is_photo = true;
        assert!(estimate_height(&message) > media_height);
        message.is_deleted = true;
        assert!(estimate_height(&message) < 80.0);
    }
}
