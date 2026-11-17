//! Conversation list for the signed-in Telegram view.

use super::conversation::bump_epoch;
use super::PendingSearchJump;
use crate::state::{set_if_changed, AppServices, UiState};
use crate::views::chat_list::{self, ChatRow};
use dioxus::prelude::*;
use plasma_telegram::{TdlibChatInfo, TdlibGlobalSearchMessage};
use std::collections::HashSet;

const CHAT_LIST_OVERSCAN: usize = 8;

const CHAT_LIST_VISIBILITY_SCRIPT: &str = r#"
(() => {
    if (window.__plasmaChatListWatch) {
        window.__plasmaChatListWatch();
        window.__plasmaChatListWatch = null;
    }
    const start = () => {
        const root = document.querySelector(".chat-list");
        if (!root) {
            requestAnimationFrame(start);
            return;
        }
        const visible = new Set();
        let frame = 0;
        let lastPayload = "";
        const send = () => {
            const payload = JSON.stringify([...visible]);
            if (payload === lastPayload) return;
            lastPayload = payload;
            cancelAnimationFrame(frame);
            frame = requestAnimationFrame(() => {
                dioxus.send(payload);
            });
        };
        const observer = new IntersectionObserver((entries) => {
            for (const entry of entries) {
                const id = entry.target.dataset.chatId;
                if (!id) continue;
                if (entry.isIntersecting) visible.add(id);
                else visible.delete(id);
            }
            send();
        }, { root, rootMargin: "320px 0px" });
        const watch = (row) => {
            if (!row.dataset || !row.dataset.chatId || row.dataset.watched === "1") return;
            row.dataset.watched = "1";
            observer.observe(row);
        };
        const forget = (node) => {
            if (!node || !node.dataset || !node.dataset.chatId) return;
            visible.delete(node.dataset.chatId);
            observer.unobserve(node);
        };
        root.querySelectorAll(".chat-row").forEach(watch);
        const mutations = new MutationObserver((records) => {
            for (const record of records) {
                record.addedNodes.forEach((node) => {
                    if (node.nodeType !== 1) return;
                    if (node.matches?.(".chat-row")) watch(node);
                    node.querySelectorAll?.(".chat-row").forEach(watch);
                });
                record.removedNodes.forEach((node) => {
                    if (node.nodeType !== 1) return;
                    if (node.matches?.(".chat-row")) forget(node);
                    node.querySelectorAll?.(".chat-row").forEach(forget);
                });
            }
            send();
        });
        mutations.observe(root, { childList: true, subtree: true });
        window.__plasmaChatListWatch = () => {
            observer.disconnect();
            mutations.disconnect();
        };
        send();
    };
    start();
})();
"#;

fn chat_window(scroll_top: f64, viewport: f64, row_h: f64, count: usize) -> (usize, usize) {
    if count == 0 || row_h <= 0.0 {
        return (0, 0);
    }
    let start = ((scroll_top / row_h).floor() as usize).saturating_sub(CHAT_LIST_OVERSCAN);
    let visible = ((viewport.max(1.0) / row_h).ceil() as usize) + 2 * CHAT_LIST_OVERSCAN;
    let end = (start + visible).min(count);
    (start.min(end), end)
}

#[component]
pub(super) fn ChatListColumn(
    mut chat_filter: Signal<String>,
    mut search_active: Signal<bool>,
    mut message_query: Signal<String>,
    mut global_results: Signal<Vec<TdlibGlobalSearchMessage>>,
    mut global_offset: Signal<String>,
    mut global_loading_more: Signal<bool>,
    mut pending_search_jump: Signal<Option<PendingSearchJump>>,
    mut search_epoch: Signal<u64>,
    mut chats_full_refresh: CopyValue<bool>,
) -> Element {
    let mut ui = use_context::<UiState>();
    let services = use_context::<AppServices>();
    let filter = chat_filter().trim().to_lowercase();
    let chats: Vec<TdlibChatInfo> = ui
        .conversation
        .chats()
        .into_iter()
        .filter(|chat| {
            filter.is_empty()
                || chat.title.to_lowercase().contains(&filter)
                || chat.last_message_text.to_lowercase().contains(&filter)
        })
        .collect();
    let selected_id = ui.conversation.selected_chat().map(|chat| chat.id);
    let row_h = ui.shell.density().row_height();
    let mut scroll_top = use_signal(|| 0.0);
    let viewport_h = use_signal(|| 720.0);
    let (start, end) = chat_window(scroll_top(), viewport_h(), row_h, chats.len());
    let total_h = chats.len() as f64 * row_h;
    let visible_rows: Vec<(f64, TdlibChatInfo)> = if start < end {
        chats[start..end]
            .iter()
            .enumerate()
            .map(|(offset, chat)| {
                let top = (start + offset) as f64 * row_h;
                (top, chat.clone())
            })
            .collect()
    } else {
        Vec::new()
    };
    let visible_chats = use_signal(HashSet::<String>::new);
    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(CHAT_LIST_VISIBILITY_SCRIPT);
            loop {
                let Ok(payload) = eval.recv::<String>().await else {
                    break;
                };
                let Ok(ids) = serde_json::from_str::<Vec<String>>(&payload) else {
                    continue;
                };
                set_if_changed(visible_chats, ids.into_iter().collect());
                tokio::task::yield_now().await;
            }
        });
    });

    rsx! {
            aside { class: "chat-list-panel",
                header { class: "chat-list-header",
                    div { class: "chat-list-title-row",
                        h2 { "Conversas" }
                        if ui.conversation.chats_loading() {
                            span { class: "pill", "carregando" }
                        }
                    }
                    div { class: "chat-search-row",
                        input {
                            class: "chat-search-input",
                            r#type: "search",
                            placeholder: "Filtrar conversas",
                            value: "{chat_filter}",
                            oninput: move |event| chat_filter.set(event.value()),
                        }
                        button {
                            class: "btn small ghost",
                            title: "Atualizar conversas",
                            onclick: move |_| {
                                chats_full_refresh.set(true);
                                let next = ui.conversation.telegram_tick().wrapping_add(1);
                                ui.conversation.telegram_tick.set(next);
                            },
                            "↻"
                        }
                    }
                    if let Some(error) = ui.conversation.chats_error() {
                        p { class: "error-banner", "{error}" }
                    }
                }
                div {
                    class: "chat-list",
                    onscroll: move |event| {
                        scroll_top.set(event.scroll_top());
                        let height = event.client_height() as f64;
                        if height > 1.0 {
                            set_if_changed(viewport_h, height);
                        }
                    },
                    if chats.is_empty() {
                        div { class: "chat-list-empty",
                            p { class: "hint",
                                if ui.conversation.chats_loading() {
                                    "Carregando conversas do Telegram..."
                                } else {
                                    "Nenhuma conversa encontrada."
                                }
                            }
                        }
                    } else {
                        div {
                            class: "chat-list-canvas",
                            style: "height: {total_h}px;",
                            for (top, chat) in visible_rows {
                                div {
                                    key: "{chat.id}",
                                    class: "chat-list-slot",
                                    style: "top: {top}px; height: {row_h}px;",
                                    ChatRow {
                                        chat: chat.clone(),
                                        active: selected_id.as_deref() == Some(chat.id.as_str()),
                                        visible_chats,
                                        on_open: {
                                            let services = services.clone();
                                            move |selected: TdlibChatInfo| {
                                                bump_epoch(search_epoch);
                                                search_active.set(false);
                                                message_query.set(String::new());
                                                ui.conversation.search_loading.set(false);
                                                ui.conversation.search_error.set(None);
                                                global_results.set(Vec::new());
                                                global_offset.set(String::new());
                                                global_loading_more.set(false);
                                                pending_search_jump.set(None);
                                                chat_list::open_chat(services.clone(), ui, selected);
                                            }
                                        },
                                        on_context: {
                                            move |(selected, x, y)| {
                                                chat_list::open_chat_menu(ui, selected, x, y);
                                            }
                                        },
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
    use super::chat_window;

    #[test]
    fn chat_window_includes_overscan_and_clamps() {
        let (start, end) = chat_window(0.0, 300.0, 60.0, 150);
        assert_eq!(start, 0);
        assert!(end > 5 && end <= 150);

        let (start, end) = chat_window(6000.0, 300.0, 60.0, 150);
        assert!(start > 80);
        assert!(end > start);
        assert!(end < 150);

        let (start, end) = chat_window(9000.0, 300.0, 60.0, 150);
        assert_eq!(end, 150);
        assert!(start < end);
    }
}
