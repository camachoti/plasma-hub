//! Variable-height virtual timeline for real Telegram messages.
//!
//! Albums (Telegram grouped media) are collapsed into a single timeline item
//! with a media grid, mirroring what the React dashboard does.

use crate::context::{save_album, ContextMenuState, ContextTarget};
use crate::icons::{Icon, IconName};
use crate::state::{AppServices, UiState};
use dioxus::prelude::*;
use plasma_telegram::TdlibMessageInfo;
use time::OffsetDateTime;

const OVERSCAN: usize = 6;
const DEFAULT_VIEWPORT: f64 = 620.0;
const TIMELINE_ID: &str = "plasma-timeline-scroll";
const ALBUM_CELL_HEIGHT: f64 = 104.0;
const ALBUM_GAP: f64 = 4.0;

/// A row of the timeline: a single message or a grouped album.
#[derive(Clone, Debug, PartialEq)]
enum TimelineItem {
    Message(Box<TdlibMessageInfo>),
    Album(Vec<TdlibMessageInfo>),
}

impl TimelineItem {
    fn key(&self) -> String {
        match self {
            TimelineItem::Message(message) => message.id.to_string(),
            TimelineItem::Album(messages) => format!("album-{}", messages[0].id),
        }
    }
}

/// Collapses consecutive messages that share a `grouped_id` into albums.
fn group_albums(messages: &[TdlibMessageInfo]) -> Vec<TimelineItem> {
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
struct TimelineLayout {
    offsets: Vec<f64>,
    heights: Vec<f64>,
    total_height: f64,
}

fn build_layout(items: &[TimelineItem]) -> TimelineLayout {
    let mut offsets = Vec::with_capacity(items.len());
    let mut heights = Vec::with_capacity(items.len());
    let mut top = 0.0;
    for item in items {
        offsets.push(top);
        let height = estimate_item_height(item);
        heights.push(height);
        top += height;
    }
    TimelineLayout {
        offsets,
        heights,
        total_height: top,
    }
}

/// Number of grid rows used by an album grid.
fn album_rows(count: usize) -> usize {
    if count <= 2 {
        1
    } else {
        count.div_ceil(2)
    }
}

fn estimate_item_height(item: &TimelineItem) -> f64 {
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

fn estimate_height(message: &TdlibMessageInfo) -> f64 {
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
        height += 18.0;
    }
    height.max(64.0)
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

/// How the timeline should deal with the scroll position after an update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ScrollIntent {
    /// Jump to the newest message (first render of a conversation).
    Bottom,
    /// Follow new messages when the reader is already at the bottom.
    Follow,
    /// Keep the visual position (older history was prepended).
    Anchor,
}

/// Decides how to scroll after the first and newest message ids changed.
///
/// Returns `None` when the timeline window did not change at all.
fn scroll_intent(
    previous: (Option<i64>, Option<i64>),
    current: (Option<i64>, Option<i64>),
) -> Option<ScrollIntent> {
    let (previous_first, previous_last) = previous;
    let (current_first, current_last) = current;
    if previous == current {
        return None;
    }
    // Appending at the bottom keeps the reader in place unless they follow.
    if previous_first.is_some() && previous_first == current_first && previous_last != current_last
    {
        return Some(ScrollIntent::Follow);
    }
    // Prepending history (or switching conversations) keeps the anchor.
    Some(ScrollIntent::Anchor)
}

/// Applies the scroll intent to the timeline container.
///
/// The container keeps the distance from the bottom on every scroll event, so
/// prepending history (which grows the canvas above the viewport) can be
/// compensated without losing the reader's place.
async fn apply_scroll(intent: ScrollIntent) {
    let mode = match intent {
        ScrollIntent::Bottom => "bottom",
        ScrollIntent::Follow => "follow",
        ScrollIntent::Anchor => "anchor",
    };
    let script = format!(
        r#"
        const element = document.getElementById("{TIMELINE_ID}");
        if (!element) return false;
        if (!element.__plasmaScrollBound) {{
            element.__plasmaScrollBound = true;
            element.addEventListener("scroll", () => {{
                element.__plasmaBottom =
                    element.scrollHeight - element.scrollTop - element.clientHeight;
            }}, {{ passive: true }});
        }}
        const mode = "{mode}";
        const anchor = element.__plasmaBottom;
        requestAnimationFrame(() => {{
            if (mode === "bottom") {{
                element.scrollTop = element.scrollHeight;
                element.__plasmaBottom = 0;
            }} else if (mode === "follow") {{
                if (anchor == null || anchor < 120) {{
                    element.scrollTop = element.scrollHeight;
                    element.__plasmaBottom = 0;
                }}
            }} else if (anchor == null) {{
                element.scrollTop = element.scrollHeight;
            }} else {{
                element.scrollTop =
                    Math.max(0, element.scrollHeight - element.clientHeight - anchor);
            }}
        }});
        true
        "#
    );
    let _ = document::eval(&script).await;
}

#[component]
pub fn MessageTimeline(
    chat_id: i64,
    messages: Signal<Vec<TdlibMessageInfo>>,
    search_results: Signal<Vec<TdlibMessageInfo>>,
    show_search: Signal<bool>,
    empty_label: String,
    selected: Vec<i64>,
    on_action: EventHandler<(TdlibMessageInfo, MessageAction)>,
    on_toggle_select: EventHandler<i64>,
    on_select_album: EventHandler<Vec<i64>>,
) -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();

    // The timeline renders either the conversation or the search results; both
    // sources are signals so the scroll effects keep tracking the active one.
    let source = move || {
        if show_search() {
            search_results()
        } else {
            messages()
        }
    };
    let items_memo = use_memo(move || group_albums(&source()));
    let layout_memo = use_memo(move || build_layout(&items_memo()));
    // First and newest ids identify what changed between two renders.
    let bounds = use_memo(move || {
        let list = source();
        (
            list.first().map(|message| message.id),
            list.last().map(|message| message.id),
        )
    });
    // Plain hook value: writing it must not wake the effect again.
    let mut previous_bounds = use_hook(|| CopyValue::new(None::<(Option<i64>, Option<i64>)>));
    let mut scroll_top = use_signal(|| 0.0);
    let mut viewport_height = use_signal(|| DEFAULT_VIEWPORT);

    let items = items_memo();
    let layout = layout_memo();
    let range = visible_range(
        &layout.offsets,
        scroll_top(),
        viewport_height(),
        items.len(),
    );
    let (start, end) = range;
    let rows: Vec<(TimelineItem, f64, f64)> = (start..end)
        .map(|index| {
            (
                items[index].clone(),
                layout.offsets[index],
                layout.heights[index],
            )
        })
        .collect();

    // Scroll only when the list really changed: a conversation switch jumps to
    // the bottom, new messages follow the reader when they are at the bottom,
    // and older history keeps the current position.
    use_effect(move || {
        let current = bounds();
        // The first render of a conversation always opens at the newest message.
        let intent = match *previous_bounds.read() {
            Some(previous) => match scroll_intent(previous, current) {
                Some(intent) => intent,
                None => return,
            },
            None => ScrollIntent::Bottom,
        };
        previous_bounds.set(Some(current));
        spawn(apply_scroll(intent));
    });

    rsx! {
        if items.is_empty() {
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
                    for (item, top, height) in rows {
                        {
                            let key = item.key();
                            match item {
                                TimelineItem::Message(message) => {
                                    let message = *message;
                                    let row_class = if message.is_deleted {
                                        "message-row deleted"
                                    } else if selected.contains(&message.id) {
                                        "message-row selected"
                                    } else if message.out {
                                        "message-row own"
                                    } else {
                                        "message-row"
                                    };
                                    rsx! {
                                        article {
                                            key: "{key}",
                                            class: "{row_class}",
                                            style: "top: {top}px; height: {height}px;",
                                            oncontextmenu: {
                                                let message = message.clone();
                                                move |event: Event<MouseData>| {
                                                    event.prevent_default();
                                                    let point = event.data().client_coordinates();
                                                    ui.context_menu.set(Some(ContextMenuState::new(
                                                        point.x,
                                                        point.y,
                                                        ContextTarget::Message {
                                                            chat_id,
                                                            message: Box::new(message.clone()),
                                                        },
                                                    )));
                                                }
                                            },
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
                                                    if message.has_media {
                                                        button {
                                                            class: if selected.contains(&message.id) { "msg-action active" } else { "msg-action" },
                                                            title: if selected.contains(&message.id) {
                                                                "Remover da seleção"
                                                            } else {
                                                                "Selecionar mídia"
                                                            },
                                                            onclick: {
                                                                let message_id = message.id;
                                                                move |_| on_toggle_select.call(message_id)
                                                            },
                                                            Icon { name: IconName::Check, size: 14, class: "icon" }
                                                        }
                                                    }
                                                    button {
                                                        class: "msg-action",
                                                        title: "Responder",
                                                        onclick: {
                                                            let message = message.clone();
                                                            move |_| on_action.call((message.clone(), MessageAction::Reply))
                                                        },
                                                        Icon { name: IconName::Reply, size: 14, class: "icon" }
                                                    }
                                                    if message.has_media {
                                                        button {
                                                            class: "msg-action",
                                                            title: "Abrir mídia",
                                                            onclick: {
                                                                let message = message.clone();
                                                                move |_| on_action.call((message.clone(), MessageAction::OpenMedia))
                                                            },
                                                            Icon { name: IconName::Media, size: 14, class: "icon" }
                                                        }
                                                        button {
                                                            class: "msg-action",
                                                            title: "Salvar mídia",
                                                            onclick: {
                                                                let services = services.clone();
                                                                let message = message.clone();
                                                                move |_| crate::context::save_media(
                                                                    services.clone(),
                                                                    ui,
                                                                    chat_id,
                                                                    message.clone(),
                                                                )
                                                            },
                                                            Icon { name: IconName::Save, size: 14, class: "icon" }
                                                        }
                                                    }
                                                    button {
                                                        class: "msg-action",
                                                        title: "Encaminhar",
                                                        onclick: {
                                                            let message = message.clone();
                                                            move |_| on_action.call((message.clone(), MessageAction::Forward))
                                                        },
                                                        Icon { name: IconName::Forward, size: 14, class: "icon" }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                TimelineItem::Album(album) => {
                                    let first = album[0].clone();
                                    let caption = album
                                        .iter()
                                        .find(|message| !message.text.trim().is_empty())
                                        .cloned();
                                    let ids: Vec<i64> = album.iter().map(|message| message.id).collect();
                                    let all_selected =
                                        ids.iter().all(|id| selected.contains(id));
                                    let row_class = if all_selected {
                                        "message-row album-row selected"
                                    } else if first.out {
                                        "message-row album-row own"
                                    } else {
                                        "message-row album-row"
                                    };
                                    let columns = if album.len() == 1 { 1 } else { 2 };
                                    let span_last = album.len() > 2 && album.len() % 2 == 1;
                                    let grid_class = if span_last {
                                        "album-grid span-last"
                                    } else {
                                        "album-grid"
                                    };
                                    let count = album.len();
                                    rsx! {
                                        article {
                                            key: "{key}",
                                            class: "{row_class}",
                                            style: "top: {top}px; height: {height}px;",
                                            oncontextmenu: {
                                                let album = album.clone();
                                                let anchor_id = first.id;
                                                move |event: Event<MouseData>| {
                                                    event.prevent_default();
                                                    let point = event.data().client_coordinates();
                                                    ui.context_menu.set(Some(ContextMenuState::new(
                                                        point.x,
                                                        point.y,
                                                        ContextTarget::Album {
                                                            chat_id,
                                                            messages: album.clone(),
                                                            anchor_id,
                                                        },
                                                    )));
                                                }
                                            },
                                            div { class: "avatar",
                                                "{first.sender_name.chars().next().unwrap_or('?')}"
                                            }
                                            div { class: "message-bubble album-bubble",
                                                div { class: "message-meta",
                                                    strong {
                                                        if first.sender_name.trim().is_empty() {
                                                            "Mensagem"
                                                        } else {
                                                            "{first.sender_name}"
                                                        }
                                                    }
                                                    span { "{format_timestamp(first.date)}" }
                                                    span { class: "pill", "álbum · {count}" }
                                                    if first.out {
                                                        span { class: "pill", "enviada" }
                                                    }
                                                }
                                                if let Some(caption) = caption.as_ref() {
                                                    if !caption.text.trim().is_empty() {
                                                        p { class: "message-text", "{caption.text}" }
                                                    }
                                                }
                                                div {
                                                    class: "{grid_class}",
                                                    style: "--album-cols: {columns};",
                                                    for media in album.iter().cloned() {
                                                        {
                                                            let media_id = media.id;
                                                            let is_selected = selected.contains(&media_id);
                                                            let cell_class = if is_selected {
                                                                "album-cell selected"
                                                            } else {
                                                                "album-cell"
                                                            };
                                                            let anchor_id = media_id;
                                                            let context_album = album.clone();
                                                            rsx! {
                                                                div {
                                                                    key: "{media_id}",
                                                                    class: "{cell_class}",
                                                                    oncontextmenu: {
                                                                        let context_album = context_album.clone();
                                                                        move |event: Event<MouseData>| {
                                                                            event.prevent_default();
                                                                            event.stop_propagation();
                                                                            let point = event.data().client_coordinates();
                                                                            ui.context_menu.set(Some(ContextMenuState::new(
                                                                                point.x,
                                                                                point.y,
                                                                                ContextTarget::Album {
                                                                                    chat_id,
                                                                                    messages: context_album.clone(),
                                                                                    anchor_id,
                                                                                },
                                                                            )));
                                                                        }
                                                                    },
                                                                    MessageThumbnail {
                                                                        chat_id,
                                                                        message_id: media.id,
                                                                        is_video: media.is_video,
                                                                        on_open: {
                                                                            let media = media.clone();
                                                                            move |_| on_action.call((media.clone(), MessageAction::OpenMedia))
                                                                        },
                                                                    }
                                                                    button {
                                                                        r#type: "button",
                                                                        class: if is_selected {
                                                                            "album-cell-select active"
                                                                        } else {
                                                                            "album-cell-select"
                                                                        },
                                                                        title: if is_selected {
                                                                            "Remover da seleção"
                                                                        } else {
                                                                            "Selecionar mídia"
                                                                        },
                                                                        onclick: move |event| {
                                                                            event.stop_propagation();
                                                                            on_toggle_select.call(media_id);
                                                                        },
                                                                        Icon { name: IconName::Check, size: 12, class: "icon" }
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                                div { class: "message-actions",
                                                    button {
                                                        class: if all_selected { "msg-action active" } else { "msg-action" },
                                                        title: if all_selected {
                                                            "Remover álbum da seleção"
                                                        } else {
                                                            "Selecionar álbum"
                                                        },
                                                        onclick: {
                                                            let ids = ids.clone();
                                                            move |_| on_select_album.call(ids.clone())
                                                        },
                                                        Icon { name: IconName::Check, size: 14, class: "icon" }
                                                    }
                                                    button {
                                                        class: "msg-action",
                                                        title: "Responder",
                                                        onclick: {
                                                            let first = first.clone();
                                                            move |_| on_action.call((first.clone(), MessageAction::Reply))
                                                        },
                                                        Icon { name: IconName::Reply, size: 14, class: "icon" }
                                                    }
                                                    button {
                                                        class: "msg-action",
                                                        title: "Salvar álbum",
                                                        onclick: {
                                                            let services = services.clone();
                                                            let album = album.clone();
                                                            move |_| save_album(
                                                                services.clone(),
                                                                ui,
                                                                chat_id,
                                                                album.clone(),
                                                            )
                                                        },
                                                        Icon { name: IconName::Download, size: 14, class: "icon" }
                                                    }
                                                    button {
                                                        class: "msg-action",
                                                        title: "Encaminhar",
                                                        onclick: {
                                                            let first = first.clone();
                                                            move |_| on_action.call((first.clone(), MessageAction::Forward))
                                                        },
                                                        Icon { name: IconName::Forward, size: 14, class: "icon" }
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

    fn photo(id: i64, group: &str) -> TdlibMessageInfo {
        let mut message = message(id, id as i32, "");
        message.has_media = true;
        message.is_photo = true;
        message.grouped_id = Some(group.to_string());
        message
    }

    #[test]
    fn offsets_are_cumulative_and_total_matches() {
        let messages = vec![message(1, 1, "curta"), message(2, 2, &"a".repeat(300))];
        let items = group_albums(&messages);
        let layout = build_layout(&items);
        assert_eq!(layout.offsets[0], 0.0);
        assert_eq!(layout.offsets[1], layout.heights[0]);
        assert_eq!(layout.total_height, layout.heights.iter().sum::<f64>());
    }

    #[test]
    fn visible_range_is_bounded_at_both_ends() {
        let messages: Vec<_> = (0..100)
            .map(|index| message(index, index as i32, "mensagem"))
            .collect();
        let items = group_albums(&messages);
        let layout = build_layout(&items);
        let (start, end) = visible_range(&layout.offsets, 0.0, 620.0, items.len());
        assert_eq!(start, 0);
        assert!(end > start);

        let (start, end) = visible_range(
            &layout.offsets,
            layout.total_height - 620.0,
            620.0,
            items.len(),
        );
        assert!(start < items.len());
        assert_eq!(end, items.len());
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

    #[test]
    fn consecutive_grouped_messages_become_one_album() {
        let messages = vec![
            photo(1, "42"),
            photo(2, "42"),
            photo(3, "42"),
            message(4, 4, "depois"),
            photo(5, "43"),
            photo(6, "43"),
        ];
        let items = group_albums(&messages);
        assert_eq!(items.len(), 3);
        match &items[0] {
            TimelineItem::Album(album) => {
                assert_eq!(album.len(), 3);
                assert_eq!(album[0].id, 1);
            }
            other => panic!("esperava álbum, veio {other:?}"),
        }
        assert!(matches!(&items[1], TimelineItem::Message(message) if message.id == 4));
        match &items[2] {
            TimelineItem::Album(album) => assert_eq!(album.len(), 2),
            other => panic!("esperava álbum, veio {other:?}"),
        }
    }

    #[test]
    fn non_consecutive_groups_are_split() {
        let messages = vec![
            photo(1, "42"),
            photo(2, "42"),
            message(3, 3, "no meio"),
            photo(4, "42"),
        ];
        let items = group_albums(&messages);
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[2], TimelineItem::Message(message) if message.id == 4));
    }

    #[test]
    fn album_height_grows_with_rows() {
        let two = TimelineItem::Album(vec![photo(1, "7"), photo(2, "7")]);
        let three = TimelineItem::Album(vec![photo(1, "7"), photo(2, "7"), photo(3, "7")]);
        let caption = TimelineItem::Album(vec![photo(1, "7"), photo(2, "7"), {
            let mut message = photo(3, "7");
            message.text = "legenda".to_string();
            message
        }]);
        assert!(estimate_item_height(&three) > estimate_item_height(&two));
        assert!(estimate_item_height(&caption) > estimate_item_height(&three));
    }

    #[test]
    fn scroll_intent_ignores_unchanged_windows() {
        assert_eq!(scroll_intent((Some(1), Some(9)), (Some(1), Some(9))), None);
        assert_eq!(scroll_intent((None, None), (None, None)), None);
    }

    #[test]
    fn scroll_intent_follows_new_messages_only_on_append() {
        assert_eq!(
            scroll_intent((Some(1), Some(9)), (Some(1), Some(12))),
            Some(ScrollIntent::Follow)
        );
        // Prepending history keeps the anchor.
        assert_eq!(
            scroll_intent((Some(1), Some(9)), (Some(-4), Some(9))),
            Some(ScrollIntent::Anchor)
        );
        // A different conversation also restarts from the bottom anchor.
        assert_eq!(
            scroll_intent((Some(1), Some(9)), (Some(40), Some(44))),
            Some(ScrollIntent::Anchor)
        );
        // The first page after an empty timeline jumps to the bottom.
        assert_eq!(
            scroll_intent((None, None), (Some(1), Some(9))),
            Some(ScrollIntent::Anchor)
        );
    }

    #[test]
    fn album_keys_are_namespaced() {
        let album = TimelineItem::Album(vec![photo(1, "7"), photo(2, "7")]);
        assert_eq!(album.key(), "album-1");
        assert_eq!(TimelineItem::Message(Box::new(photo(9, "7"))).key(), "9");
    }
}
