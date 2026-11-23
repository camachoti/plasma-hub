//! Variable-height virtual timeline for real Telegram messages.
//!
//! Albums (Telegram grouped media) are collapsed into a single timeline item
//! with a media grid, mirroring what the React dashboard does.

mod layout;
mod thumbnail;

use layout::*;
pub(crate) use layout::{format_timestamp, reply_sender, reply_snippet};
pub(crate) use thumbnail::MessageThumbnail;

use crate::context::{save_album, ContextMenuState, ContextTarget};
use crate::icons::{Icon, IconName};
use crate::state::{
    push_toast, react_to_message, set_if_changed, AppServices, LightboxMedia, ReactionPickerState,
    ToastKind, UiState,
};
use crate::theme::{avatar_gradient, avatar_initials, sender_color};
use dioxus::prelude::*;
use plasma_telegram::TdlibMessageInfo;
use std::collections::{HashMap, HashSet};

const DEFAULT_VIEWPORT: f64 = 620.0;
const TIMELINE_ID: &str = "plasma-timeline-scroll";
/// Reply previews are secondary to the timeline itself. Fetching every missing
/// quote at once can queue dozens of TDLib requests and delay the visible page.
const REPLY_FETCH_CONCURRENCY: usize = 1;

/// Observes the rendered rows and reports their real height to Rust.
///
/// The canvas positions rows with `top`/`height` before they exist in the DOM,
/// so it starts with estimates. Measuring `height: auto` for a frame gives the
/// exact height (the assigned height clips overflow with `overflow: hidden`),
/// and the layout is rebuilt with the measured value. The script only reports
/// changed rows, so the measurement converges after one render.
const MEASURE_ROWS_SCRIPT: &str = r#"
(() => {
    // A remounted timeline would otherwise leave the previous observers alive.
    if (window.__plasmaTimelineMeasureCleanup) {
        window.__plasmaTimelineMeasureCleanup();
    }
    const TIMELINE_ID = "plasma-timeline-scroll";
    const ROW_SELECTOR = "article.message-row[data-key]";
    let container = null;
    let frame = 0;
    let measureAll = true;
    const dirty = new Set();
    const lastHeights = new Map();

    const closestRow = (node) => {
        if (!node) return null;
        const element = node.nodeType === 1 ? node : node.parentElement;
        return element?.closest?.(ROW_SELECTOR) ?? null;
    };

    const markRow = (node) => {
        const row = closestRow(node);
        if (row?.dataset.key) dirty.add(row.dataset.key);
    };

    const onMutations = (records) => {
        for (const record of records) {
            if (record.type === "childList") {
                for (const node of record.addedNodes) markRow(node);
                markRow(record.target);
            } else if (record.type === "characterData") {
                markRow(record.target);
            }
        }
        schedule();
    };

    const measure = () => {
        const element = document.getElementById(TIMELINE_ID);
        if (!element) return;
        const scrollTop = element.scrollTop;
        const rows = [];
        for (const row of element.querySelectorAll(ROW_SELECTOR)) {
            if (measureAll || dirty.has(row.dataset.key)) rows.push(row);
        }
        dirty.clear();
        measureAll = false;
        if (rows.length === 0) return;

        const oldHeights = rows.map((row) => row.offsetHeight);
        const previous = rows.map((row) => row.style.height);
        // Batch the writes and natural-height reads: two layout passes for the
        // whole batch instead of one forced reflow per visible row.
        for (const row of rows) row.style.height = "auto";
        const measured = rows.map((row) => Math.ceil(row.offsetHeight));
        for (let index = 0; index < rows.length; index += 1) {
            rows[index].style.height = previous[index];
        }

        const heights = {};
        let deltaAbove = 0;
        let changed = false;
        for (let index = 0; index < rows.length; index += 1) {
            const row = rows[index];
            const key = row.dataset.key;
            const height = measured[index];
            if (!key || height <= 0) continue;
            const previousHeight = lastHeights.get(key);
            if (previousHeight == null || Math.abs(previousHeight - height) > 2) {
                changed = true;
                lastHeights.set(key, height);
            }
            const oldHeight = oldHeights[index];
            const top = parseFloat(row.style.top) || 0;
            heights[key] = height;
            if (top + oldHeight <= scrollTop + 0.5) {
                deltaAbove += height - oldHeight;
            }
        }
        if (!changed) return;
        dioxus.send({
            heights,
            deltaAbove,
            atBottom: element.__plasmaBottom == null || element.__plasmaBottom < 120,
        });
    };

    let debounce = 0;
    const schedule = () => {
        if (debounce) return;
        // Coalesce MutationObserver bursts from cache→quick→follow-up paints
        // into one measure pass so the Rust side is not flooded.
        debounce = setTimeout(() => {
            debounce = 0;
            if (frame) return;
            frame = requestAnimationFrame(() => {
                frame = 0;
                measure();
            });
        }, 48);
    };

    const markAll = () => {
        measureAll = true;
        schedule();
    };

    const mutations = new MutationObserver(onMutations);
    const bind = () => {
        const element = document.getElementById(TIMELINE_ID);
        if (!element) return false;
        if (container === element) return true;
        container = element;
        mutations.disconnect();
        // Text and scrollbar changes used to schedule a new measure for every
        // frame. A chat that had just opened then never returned to the event
        // loop. Structural changes are enough to pick up new rows.
        mutations.observe(element, {
            childList: true,
            subtree: true,
        });
        markAll();
        return true;
    };

    // The timeline may appear after messages are loaded, and switching chats
    // can replace the scroll container entirely. Rebind when that happens.
    const watcher = new MutationObserver(() => {
        const element = document.getElementById(TIMELINE_ID);
        if (element && element !== container) bind();
    });
    watcher.observe(document.body, { childList: true, subtree: true });
    window.__plasmaTimelineMarkAll = markAll;
    window.__plasmaTimelineMeasureCleanup = () => {
        window.__plasmaTimelineMarkAll = null;
        mutations.disconnect();
        watcher.disconnect();
        window.removeEventListener("resize", markAll);
        if (debounce) clearTimeout(debounce);
        if (frame) cancelAnimationFrame(frame);
    };
    bind();
    window.addEventListener("resize", markAll);
    markAll();
    setTimeout(markAll, 300);
})();
"#;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageAction {
    Reply,
    OpenMedia,
    OpenAlbumMedia(Vec<LightboxMedia>),
    Forward,
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
            // Keep the newest message visible when the viewport itself changes
            // (composer/reply banner growth, window resize) while at the bottom.
            if (typeof ResizeObserver !== "undefined") {{
                new ResizeObserver(() => {{
                    const bottom = element.__plasmaBottom;
                    if (bottom == null || bottom >= 120) return;
                    element.scrollTop = element.scrollHeight;
                    element.__plasmaBottom = 0;
                }}).observe(element);
            }}
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
    /// Parent-controlled jump request. The timeline clears it after scrolling.
    mut jump_to: Signal<Option<i64>>,
    /// Called when a reply quote is clicked. The parent can load the original
    /// when it is outside the currently loaded page, then set `jump_to`.
    on_jump_to_message: EventHandler<i64>,
    /// Called when the reader scrolls into the oldest loaded messages, so the
    /// caller can fetch the previous page (React parity: `startReached`).
    on_reach_start: EventHandler<()>,
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
    let mut measured_heights = use_signal(HashMap::<String, f64>::new);
    // Heights are scoped by chat id in the key; also clear and re-measure when
    // the user switches visual style or density so bubble vs. community rows
    // resize cleanly in a single frame.
    use_effect(move || {
        let _ = (chat_id, ui.shell.style(), ui.shell.density());
        measured_heights.set(HashMap::new());
        spawn(async move {
            let _ =
                document::eval("requestAnimationFrame(() => window.__plasmaTimelineMarkAll?.());")
                    .await;
        });
    });
    let mut highlighted_message = use_signal(|| None::<i64>);
    // Replies to messages outside the loaded page are fetched by id and kept
    // here so the quote can still show the original author and text.
    let reply_cache = use_signal(HashMap::<(i64, i64), (String, String)>::new);
    let mut requested_replies = use_hook(|| CopyValue::new(HashSet::<(i64, i64)>::new()));
    let layout_memo = use_memo(move || build_layout(&items_memo(), &measured_heights(), chat_id));
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
    let viewport_height = use_signal(|| DEFAULT_VIEWPORT);
    // Scroll events fire far more often than the visible window changes. Keep
    // the rendered range stable until the reader moved enough to matter.
    let mut last_scroll = use_hook(|| CopyValue::new((0.0, 0.0)));

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

    // Jump to a replied message. The target row may be outside the currently
    // rendered window, so first move `scroll_top` to its canvas offset; the
    // virtualized timeline then renders that row and we scroll the real
    // container to the same position on the next frames.
    use_effect(move || {
        let Some(target) = jump_to() else {
            return;
        };
        jump_to.set(None);
        let items = items_memo();
        let layout = layout_memo();
        let Some(offset) = message_offset(&items, &layout, target) else {
            push_toast(
                ui.shell.toasts,
                "A mensagem original não está carregada nesta página.",
                ToastKind::Info,
            );
            return;
        };
        scroll_top.set(offset);
        highlighted_message.set(Some(target));
        spawn(async move {
            let script = format!(
                r#"
                requestAnimationFrame(() => requestAnimationFrame(() => {{
                    const element = document.getElementById("{TIMELINE_ID}");
                    if (!element) return;
                    element.scrollTop = {offset};
                    element.__plasmaBottom =
                        element.scrollHeight - element.scrollTop - element.clientHeight;
                }}));
                true
                "#
            );
            let _ = document::eval(&script).await;
            tokio::time::sleep(std::time::Duration::from_millis(1800)).await;
            if highlighted_message() == Some(target) {
                highlighted_message.set(None);
            }
        });
    });

    // Resolve replies whose original message is outside the loaded page so the
    // quote shows the same author + excerpt users expect from Telegram and
    // WhatsApp. Clicking the quote asks the parent to load the missing context.
    let reply_services = services.clone();
    use_effect(move || {
        let items = items_memo();
        let layout = layout_memo();
        let (start, end) = visible_range(
            &layout.offsets,
            scroll_top(),
            viewport_height(),
            items.len(),
        );
        let cached = reply_cache
            .peek()
            .keys()
            .copied()
            .collect::<HashSet<(i64, i64)>>();
        let mut missing = Vec::new();
        for item in items.iter().skip(start).take(end.saturating_sub(start)) {
            let messages: &[TdlibMessageInfo] = match item {
                TimelineItem::Message(message) => std::slice::from_ref(message.as_ref()),
                TimelineItem::Album(messages) => messages.as_slice(),
            };
            for message in messages {
                let Some(reply_id) = message.reply_to_msg_id else {
                    continue;
                };
                let key = (chat_id, reply_id);
                if reply_preview(&items, reply_id).is_some()
                    || cached.contains(&key)
                    || !requested_replies.write().insert(key)
                {
                    continue;
                }
                missing.push(reply_id);
            }
        }
        if missing.is_empty() {
            return;
        }
        let services = reply_services.clone();
        let mut cache = reply_cache;
        let messages_loading = ui.conversation.messages_loading;
        spawn(async move {
            // Wait for the open-chat page before spending TDLib on quotes.
            for _ in 0..80 {
                if !*messages_loading.peek() && !services.telegram.conversation_priority_active() {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
            let permits = std::sync::Arc::new(tokio::sync::Semaphore::new(REPLY_FETCH_CONCURRENCY));
            let mut tasks = tokio::task::JoinSet::new();
            for reply_id in missing {
                let Ok(permit) = permits.clone().acquire_owned().await else {
                    break;
                };
                let services = services.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    let preview = match services.telegram.get_message(chat_id, reply_id).await {
                        Ok(message) => (reply_sender(&message), reply_snippet(&message)),
                        Err(_) => (
                            "Mensagem original".to_string(),
                            "Não disponível".to_string(),
                        ),
                    };
                    (reply_id, preview)
                });
            }
            let mut found = Vec::new();
            while let Some(result) = tasks.join_next().await {
                if let Ok(preview) = result {
                    found.push(preview);
                }
            }
            if found.is_empty() {
                return;
            }
            cache.with_mut(|cache| {
                for (reply_id, preview) in found {
                    cache.insert((chat_id, reply_id), preview);
                }
            });
        });
    });

    // Feed the rendered height of every row back into the layout. Estimates
    // remain useful for rows that were never on screen, but a message longer
    // than its estimate must grow instead of being clipped by the canvas.
    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(MEASURE_ROWS_SCRIPT);
            while let Ok(payload) = eval.recv::<RowMeasurements>().await {
                let changed = {
                    let current = measured_heights.peek();
                    payload.heights.iter().any(|(key, height)| {
                        height.is_finite()
                            && *height > 0.0
                            && current
                                .get(key)
                                .is_none_or(|current| (*current - *height).abs() > 2.0)
                    })
                };
                if !changed {
                    continue;
                }
                measured_heights.with_mut(|current| {
                    for (key, height) in payload.heights {
                        if height.is_finite() && height > 0.0 {
                            current.insert(key, height);
                        }
                    }
                });
                // Dioxus applies the new layout on the next frame. Rows that
                // grew above the viewport would otherwise move the reader's
                // position, so compensate; at the bottom, stay pinned.
                let script = if payload.at_bottom {
                    format!(
                        r#"
                        requestAnimationFrame(() => requestAnimationFrame(() => {{
                            const element = document.getElementById("{TIMELINE_ID}");
                            if (!element) return;
                            element.scrollTop = element.scrollHeight;
                            element.__plasmaBottom = 0;
                        }}));
                        true
                        "#
                    )
                } else if payload.delta_above.abs() > 0.5 {
                    let delta = payload.delta_above;
                    format!(
                        r#"
                        requestAnimationFrame(() => requestAnimationFrame(() => {{
                            const element = document.getElementById("{TIMELINE_ID}");
                            if (!element) return;
                            element.scrollTop = Math.max(0, element.scrollTop + {delta});
                            element.__plasmaBottom =
                                element.scrollHeight - element.scrollTop - element.clientHeight;
                        }}));
                        true
                        "#
                    )
                } else {
                    String::new()
                };
                if !script.is_empty() {
                    let _ = document::eval(&script).await;
                }
                // A burst of row measurements otherwise stays on this task and
                // the desktop stops receiving events.
                tokio::task::yield_now().await;
            }
        });
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
                    let top = event.scroll_top();
                    let height = event.client_height() as f64;
                    let (last_top, last_height) = *last_scroll.read();
                    // Overscan keeps the viewport covered while the window lags
                    // by a few pixels, so avoid re-rendering for every event.
                    if (top - last_top).abs() >= 24.0 || (height - last_height).abs() > 0.5 {
                        last_scroll.set((top, height));
                        set_if_changed(scroll_top, top);
                        set_if_changed(viewport_height, height);
                    }
                    // The caller ignores this while a page is in flight or when
                    // there is nothing older to fetch.
                    if top < 220.0 {
                        on_reach_start.call(());
                    }
                },
                div {
                    class: "message-canvas",
                    style: "height: {layout.total_height}px;",
                    for (item, top, height) in rows {
                        {
                            let key = scoped_key(chat_id, &item);
                            match item {
                                TimelineItem::Message(message) => {
                                    let message = *message;
                                    let mut row_class = String::from("message-row");
                                    if message.is_deleted {
                                        row_class.push_str(" deleted");
                                    } else if selected.contains(&message.id) {
                                        row_class.push_str(" selected");
                                    } else if message.out {
                                        row_class.push_str(" own");
                                    }
                                    if highlighted_message() == Some(message.id) {
                                        row_class.push_str(" highlight");
                                    }
                                    let reply_quote = message.reply_to_msg_id.map(|reply_id| {
                                        if let Some(target) = reply_preview(&items, reply_id) {
                                            return (
                                                reply_id,
                                                reply_sender(target),
                                                reply_snippet(target),
                                            );
                                        }
                                        let cached = {
                                            let cache = reply_cache.read();
                                            cache.get(&(chat_id, reply_id)).cloned()
                                        };
                                        if let Some((sender, text)) = cached {
                                            (reply_id, sender, text)
                                        } else {
                                            (
                                                reply_id,
                                                "Mensagem original".to_string(),
                                                "Carregando…".to_string(),
                                            )
                                        }
                                    });
                                    rsx! {
                                        article {
                                            key: "{key}",
                                            "data-key": "{key}",
                                            class: "{row_class}",
                                            style: "top: {top}px; height: {height}px;",
                                            oncontextmenu: {
                                                let message = message.clone();
                                                move |event: Event<MouseData>| {
                                                    event.prevent_default();
                                                    let point = event.data().client_coordinates();
                                                    ui.panels.context_menu.set(Some(ContextMenuState::new(
                                                        point.x,
                                                        point.y,
                                                        ContextTarget::Message {
                                                            chat_id,
                                                            message: Box::new(message.clone()),
                                                        },
                                                    )));
                                                }
                                            },
                                            if !message.out {
                                                {
                                                    let seed = if message.sender_id.trim().is_empty() {
                                                        &message.sender_name
                                                    } else {
                                                        &message.sender_id
                                                    };
                                                    let grad = avatar_gradient(seed);
                                                    let mono = avatar_initials(&message.sender_name);
                                                    rsx! {
                                                        div {
                                                            class: "avatar",
                                                            style: "background: {grad};",
                                                            oncontextmenu: {
                                                                let message = message.clone();
                                                                move |event: Event<MouseData>| {
                                                                    if message.sender_id.trim().is_empty() {
                                                                        return;
                                                                    }
                                                                    event.prevent_default();
                                                                    event.stop_propagation();
                                                                    let point = event.data().client_coordinates();
                                                                    ui.panels.context_menu.set(Some(ContextMenuState::new(
                                                                        point.x,
                                                                        point.y,
                                                                        ContextTarget::User {
                                                                            chat_id,
                                                                            sender_id: message.sender_id.clone(),
                                                                            sender_name: message.sender_name.clone(),
                                                                        },
                                                                    )));
                                                                }
                                                            },
                                                            "{mono}"
                                                        }
                                                    }
                                                }
                                            } else {
                                                div {
                                                    class: "avatar own-avatar",
                                                    "V"
                                                }
                                            }
                                            div { class: "message-bubble",
                                                if !message.out {
                                                    {
                                                        let seed = if message.sender_id.trim().is_empty() {
                                                            &message.sender_name
                                                        } else {
                                                            &message.sender_id
                                                        };
                                                        let name_color = sender_color(seed);
                                                        rsx! {
                                                            div { class: "message-meta",
                                                                strong {
                                                                    style: "color: {name_color};",
                                                                    oncontextmenu: {
                                                                        let message = message.clone();
                                                                        move |event: Event<MouseData>| {
                                                                            if message
                                                                                .sender_id
                                                                                .trim()
                                                                                .is_empty()
                                                                            {
                                                                                return;
                                                                            }
                                                                            event.prevent_default();
                                                                            event.stop_propagation();
                                                                            let point = event
                                                                                .data()
                                                                                .client_coordinates();
                                                                            ui.panels.context_menu.set(Some(
                                                                                ContextMenuState::new(
                                                                                    point.x,
                                                                                    point.y,
                                                                                    ContextTarget::User {
                                                                                        chat_id,
                                                                                        sender_id: message
                                                                                            .sender_id
                                                                                            .clone(),
                                                                                        sender_name: message
                                                                                            .sender_name
                                                                                            .clone(),
                                                                                    },
                                                                                ),
                                                                            ));
                                                                        }
                                                                    },
                                                                    if message.sender_name.trim().is_empty() {
                                                                        "Mensagem"
                                                                    } else {
                                                                        "{message.sender_name}"
                                                                    }
                                                                }
                                                                span { class: "msg-inline-time", "{format_timestamp(message.date)}" }
                                                            }
                                                        }
                                                    }
                                                } else {
                                                    div { class: "message-meta own-meta",
                                                        strong { "Você" }
                                                        span { class: "msg-inline-time", "{format_timestamp(message.date)}" }
                                                    }
                                                }
                                                if let Some((reply_id, reply_sender, reply_text)) = reply_quote {
                                                    button {
                                                        r#type: "button",
                                                        class: "message-reply",
                                                        title: "Ir para a mensagem original",
                                                        onclick: move |event| {
                                                            event.stop_propagation();
                                                            on_jump_to_message.call(reply_id);
                                                        },
                                                        span { class: "message-reply-sender", "{reply_sender}" }
                                                        span { class: "message-reply-text", "{reply_text}" }
                                                    }
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
                                                if !message.reactions.is_empty() {
                                                    div { class: "message-reactions",
                                                        for reaction in message.reactions.iter() {
                                                            button {
                                                                r#type: "button",
                                                                class: if reaction.mine {
                                                                    "reaction-chip mine"
                                                                } else {
                                                                    "reaction-chip"
                                                                },
                                                                title: "Reagir com {reaction.emoji}",
                                                                onclick: {
                                                                    let emoji = reaction.emoji.clone();
                                                                    let message_id = message.id;
                                                                    let services = services.clone();
                                                                    move |_| react_to_message(
                                                                        &services,
                                                                        ui,
                                                                        chat_id,
                                                                        message_id,
                                                                        emoji.clone(),
                                                                    )
                                                                },
                                                                span { "{reaction.emoji}" }
                                                                span { class: "reaction-chip-count", "{reaction.count}" }
                                                            }
                                                        }
                                                    }
                                                }
                                                div { class: "message-footer",
                                                    span { class: "message-time", "{format_timestamp(message.date)}" }
                                                    if message.out {
                                                        span {
                                                            class: "message-sent",
                                                            title: "Enviada",
                                                            aria_label: "Enviada",
                                                            Icon { name: IconName::Check, size: 13, class: "icon" }
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
                                                    button {
                                                        class: "msg-action",
                                                        title: "Reagir",
                                                        onclick: {
                                                            let message_id = message.id;
                                                            move |event: Event<MouseData>| {
                                                                let point = event.data().client_coordinates();
                                                                ui.panels.reaction_picker.set(Some(ReactionPickerState {
                                                                    chat_id,
                                                                    message_id,
                                                                    x: point.x,
                                                                    y: point.y,
                                                                }));
                                                            }
                                                        },
                                                        Icon { name: IconName::Smile, size: 14, class: "icon" }
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
                                    let mut row_class = String::from("message-row album-row");
                                    if all_selected {
                                        row_class.push_str(" selected");
                                    } else if first.out {
                                        row_class.push_str(" own");
                                    }
                                    if album.iter().any(|item| {
                                        highlighted_message() == Some(item.id)
                                    }) {
                                        row_class.push_str(" highlight");
                                    }
                                    let columns = if album.len() == 1 { 1 } else { 2 };
                                    let cell_aspects = album_cell_aspects(&album);
                                    let span_last = album.len() > 2 && album.len() % 2 == 1;
                                    let grid_class = if span_last {
                                        "album-grid span-last"
                                    } else {
                                        "album-grid"
                                    };
                                    let count = album.len();
                                    let album_lightbox: Vec<LightboxMedia> = album
                                        .iter()
                                        .map(LightboxMedia::from_message)
                                        .collect();
                                    rsx! {
                                        article {
                                            key: "{key}",
                                            "data-key": "{key}",
                                            class: "{row_class}",
                                            style: "top: {top}px; height: {height}px;",
                                            oncontextmenu: {
                                                let album = album.clone();
                                                let anchor_id = first.id;
                                                move |event: Event<MouseData>| {
                                                    event.prevent_default();
                                                    let point = event.data().client_coordinates();
                                                    ui.panels.context_menu.set(Some(ContextMenuState::new(
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
                                            if !first.out {
                                                {
                                                    let seed = if first.sender_id.trim().is_empty() {
                                                        &first.sender_name
                                                    } else {
                                                        &first.sender_id
                                                    };
                                                    let grad = avatar_gradient(seed);
                                                    let mono = avatar_initials(&first.sender_name);
                                                    rsx! {
                                                        div {
                                                            class: "avatar",
                                                            style: "background: {grad};",
                                                            oncontextmenu: {
                                                                let first = first.clone();
                                                                move |event: Event<MouseData>| {
                                                                    if first.sender_id.trim().is_empty() {
                                                                        return;
                                                                    }
                                                                    event.prevent_default();
                                                                    event.stop_propagation();
                                                                    let point = event.data().client_coordinates();
                                                                    ui.panels.context_menu.set(Some(ContextMenuState::new(
                                                                        point.x,
                                                                        point.y,
                                                                        ContextTarget::User {
                                                                            chat_id,
                                                                            sender_id: first.sender_id.clone(),
                                                                            sender_name: first.sender_name.clone(),
                                                                        },
                                                                    )));
                                                                }
                                                            },
                                                            "{mono}"
                                                        }
                                                    }
                                                }
                                            } else {
                                                div {
                                                    class: "avatar own-avatar",
                                                    "V"
                                                }
                                            }
                                            div { class: "message-bubble album-bubble",
                                                div { class: "message-meta",
                                                    if !first.out {
                                                        {
                                                            let seed = if first.sender_id.trim().is_empty() {
                                                                &first.sender_name
                                                            } else {
                                                                &first.sender_id
                                                            };
                                                            let name_color = sender_color(seed);
                                                            rsx! {
                                                                strong {
                                                                    style: "color: {name_color};",
                                                                    oncontextmenu: {
                                                                        let first = first.clone();
                                                                        move |event: Event<MouseData>| {
                                                                            if first
                                                                                .sender_id
                                                                                .trim()
                                                                                .is_empty()
                                                                            {
                                                                                return;
                                                                            }
                                                                            event.prevent_default();
                                                                            event.stop_propagation();
                                                                            let point = event
                                                                                .data()
                                                                                .client_coordinates();
                                                                            ui.panels.context_menu.set(Some(
                                                                                ContextMenuState::new(
                                                                                    point.x,
                                                                                    point.y,
                                                                                    ContextTarget::User {
                                                                                        chat_id,
                                                                                        sender_id: first
                                                                                            .sender_id
                                                                                            .clone(),
                                                                                        sender_name: first
                                                                                            .sender_name
                                                                                            .clone(),
                                                                                    },
                                                                                ),
                                                                            ));
                                                                        }
                                                                    },
                                                                    if first.sender_name.trim().is_empty() {
                                                                        "Mensagem"
                                                                    } else {
                                                                        "{first.sender_name}"
                                                                    }
                                                                }
                                                            }
                                                        }
                                                    } else {
                                                        strong { class: "own-meta", "Você" }
                                                    }
                                                    span { class: "pill", "álbum · {count}" }
                                                }
                                                if let Some(caption) = caption.as_ref() {
                                                    if !caption.text.trim().is_empty() {
                                                        p { class: "message-text", "{caption.text}" }
                                                    }
                                                }
                                                div {
                                                    class: "{grid_class}",
                                                    style: "--album-cols: {columns};",
                                                    for (index, media) in album.iter().cloned().enumerate() {
                                                        {
                                                            let media_id = media.id;
                                                            let is_selected = selected.contains(&media_id);
                                                            let cell_class = if is_selected {
                                                                "album-cell selected"
                                                            } else {
                                                                "album-cell"
                                                            };
                                                            let aspect = cell_aspects
                                                                .get(index)
                                                                .copied()
                                                                .unwrap_or(1.52);
                                                            let cell_style =
                                                                format!("--album-aspect: {aspect:.3};");
                                                            let anchor_id = media_id;
                                                            let context_album = album.clone();
                                                            rsx! {
                                                                div {
                                                                    key: "{media_id}",
                                                                    class: "{cell_class}",
                                                                    style: "{cell_style}",
                                                                    oncontextmenu: {
                                                                        let context_album = context_album.clone();
                                                                        move |event: Event<MouseData>| {
                                                                            event.prevent_default();
                                                                            event.stop_propagation();
                                                                            let point = event.data().client_coordinates();
                                                                            ui.panels.context_menu.set(Some(ContextMenuState::new(
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
                                                                            let album_lightbox = album_lightbox.clone();
                                                                            move |_| {
                                                                                on_action.call((
                                                                                    media.clone(),
                                                                                    MessageAction::OpenAlbumMedia(
                                                                                        album_lightbox.clone(),
                                                                                    ),
                                                                                ))
                                                                            }
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
                                                div { class: "message-footer",
                                                    span { class: "message-time", "{format_timestamp(first.date)}" }
                                                    if first.out {
                                                        span {
                                                            class: "message-sent",
                                                            title: "Enviada",
                                                            aria_label: "Enviada",
                                                            Icon { name: IconName::Check, size: 13, class: "icon" }
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
            media_width: None,
            media_height: None,
            thumbnail_path: None,
            is_deleted: false,
            reactions: Vec::new(),
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
        let layout = build_layout(&items, &HashMap::new(), 7);
        assert_eq!(layout.offsets[0], 0.0);
        assert_eq!(layout.offsets[1], layout.heights[0]);
        assert_eq!(layout.total_height, layout.heights.iter().sum::<f64>());
    }

    #[test]
    fn measured_height_replaces_the_estimate() {
        let messages = vec![message(1, 1, "curta"), message(2, 2, "outra")];
        let items = group_albums(&messages);
        let estimated = build_layout(&items, &HashMap::new(), 7);
        let measured = HashMap::from([(scoped_key(7, &items[0]), estimated.heights[0] + 180.0)]);
        let layout = build_layout(&items, &measured, 7);
        assert_eq!(layout.heights[0], estimated.heights[0] + 180.0);
        assert_eq!(layout.offsets[1], layout.heights[0]);
        assert_eq!(layout.total_height, layout.heights.iter().sum::<f64>());
    }

    #[test]
    fn visible_range_is_bounded_at_both_ends() {
        let messages: Vec<_> = (0..100)
            .map(|index| message(index, index as i32, "mensagem"))
            .collect();
        let items = group_albums(&messages);
        let layout = build_layout(&items, &HashMap::new(), 7);
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
    fn reply_preview_finds_messages_inside_albums() {
        let original = message(1, 1, "mensagem original");
        let items = group_albums(&[original, photo(3, "7"), photo(4, "7")]);
        let target = reply_preview(&items, 1).expect("mensagem original");
        assert_eq!(target.sender_name, "Ana");
        assert_eq!(reply_snippet(target), "mensagem original");
        assert!(reply_preview(&items, 3).is_some());
        assert!(reply_preview(&items, 99).is_none());
    }

    #[test]
    fn reply_snippet_truncates_and_describes_media() {
        let long = message(1, 1, &"a".repeat(200));
        let snippet = reply_snippet(&long);
        assert_eq!(snippet.chars().count(), 111);
        assert!(snippet.ends_with('…'));

        let mut photo = message(2, 2, "");
        photo.has_media = true;
        photo.is_photo = true;
        assert_eq!(reply_snippet(&photo), "Foto");
    }

    #[test]
    fn reply_sender_labels_outgoing_messages_as_you() {
        let mut outgoing = message(1, 1, "/mystatus");
        outgoing.out = true;
        outgoing.sender_name = "Tiago".to_string();
        assert_eq!(reply_sender(&outgoing), "Você");

        let mut incoming = message(2, 2, "oi");
        incoming.sender_name.clear();
        assert_eq!(reply_sender(&incoming), "Mensagem");
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
    fn portrait_albums_are_taller_than_landscape_albums() {
        let mut portrait = photo(1, "7");
        portrait.media_width = Some(900);
        portrait.media_height = Some(1600);
        let mut second = portrait.clone();
        second.id = 2;
        let mut landscape = photo(3, "8");
        landscape.media_width = Some(1600);
        landscape.media_height = Some(900);
        let mut landscape_next = landscape.clone();
        landscape_next.id = 4;
        let portrait_album = TimelineItem::Album(vec![portrait, second]);
        let landscape_album = TimelineItem::Album(vec![landscape, landscape_next]);
        assert!(estimate_item_height(&portrait_album) > estimate_item_height(&landscape_album));
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
    fn finds_messages_inside_albums() {
        let album = TimelineItem::Album(vec![photo(1, "7"), photo(2, "7")]);
        assert!(album.contains(2));
        assert!(!album.contains(3));
        assert!(TimelineItem::Message(Box::new(photo(9, "7"))).contains(9));
    }

    #[test]
    fn large_timeline_layout_and_jump_lookup_stay_consistent() {
        let messages: Vec<_> = (0..10_000)
            .map(|index| message(index, index as i32, "mensagem de teste"))
            .collect();
        let items = group_albums(&messages);
        let layout = build_layout(&items, &HashMap::new(), 7);

        assert_eq!(items.len(), messages.len());
        assert_eq!(layout.offsets.len(), messages.len());
        assert_eq!(
            message_offset(&items, &layout, 9_999),
            Some(layout.offsets[9_999])
        );
        assert!(layout.total_height > 0.0);
    }

    #[test]
    fn album_keys_are_namespaced() {
        let album = TimelineItem::Album(vec![photo(1, "7"), photo(2, "7")]);
        assert_eq!(album.key(), "album-1");
        assert_eq!(TimelineItem::Message(Box::new(photo(9, "7"))).key(), "9");
    }
}
