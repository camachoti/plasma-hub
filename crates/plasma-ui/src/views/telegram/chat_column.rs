//! Unified Messenger Sidebar: search capsule, instant scope pills, virtualized
//! conversations (Telegram + X galleries) and quick section bar.

use super::conversation::bump_epoch;
use super::PendingSearchJump;
use crate::icons::{Icon, IconName};
use crate::state::{
    open_drawer_tab, set_if_changed, AppServices, ChatScope, DrawerTab, Tab, UiState,
};
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

fn matches_scope(
    chat: &TdlibChatInfo,
    scope: ChatScope,
    badges: &std::collections::HashMap<String, usize>,
) -> bool {
    let is_twitter = crate::twitter_fake::is_twitter_fake_chat_id(&chat.id);
    match scope {
        ChatScope::All => true,
        ChatScope::Unread => {
            chat.unread_count > 0 || (is_twitter && badges.get(&chat.id).copied().unwrap_or(0) > 0)
        }
        ChatScope::Forums => chat.has_topics && !is_twitter,
        ChatScope::Twitter => is_twitter,
    }
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
    let mut scope = use_signal(|| ChatScope::All);

    let filter = chat_filter().trim().to_lowercase();
    let current_scope = scope();
    let badges = ui.conversation.twitter_gallery_badges();
    let chats: Vec<TdlibChatInfo> = ui
        .conversation
        .chats()
        .into_iter()
        .filter(|chat| matches_scope(chat, current_scope, &badges))
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

    let active_tab = ui.shell.tab();
    let downloads = ui.transfers.downloads();
    let active_downloads = downloads.iter().filter(|item| item.is_active()).count();
    let failed_downloads = downloads.iter().any(|item| item.status == "failed");
    let sidebar_class = if ui.shell.sidebar_open() {
        "sidebar-pane"
    } else {
        "sidebar-pane collapsed"
    };

    rsx! {
        aside {
            class: "{sidebar_class}",
            aria_label: "Conversas e fontes",
            div { class: "sidebar-top",
                div { class: "sidebar-search-bar",
                    div { class: "pill-search",
                        Icon { name: IconName::Search, size: 16, class: "icon" }
                        input {
                            r#type: "search",
                            placeholder: "Buscar conversas…",
                            aria_label: "Buscar conversas",
                            value: "{chat_filter}",
                            oninput: move |event| {
                                chat_filter.set(event.value());
                                scroll_top.set(0.0);
                            },
                        }
                        if ui.conversation.chats_loading() {
                            span { class: "spinner tiny" }
                        }
                    }
                    button {
                        class: "icon-button",
                        title: "Atualizar conversas",
                        aria_label: "Atualizar conversas",
                        onclick: move |_| {
                            chats_full_refresh.set(true);
                            let next = ui.conversation.telegram_tick().wrapping_add(1);
                            ui.conversation.telegram_tick.set(next);
                        },
                        Icon { name: IconName::Refresh, size: 17, class: "icon" }
                    }
                    button {
                        class: "icon-button accent",
                        title: "Importar link ou abrir X/Twitter",
                        aria_label: "Importar link ou abrir X/Twitter",
                        onclick: move |_| {
                            ui.shell.tab.set(Tab::Downloads);
                            ui.conversation.mobile_show_list.set(false);
                        },
                        Icon { name: IconName::Plus, size: 17, class: "icon" }
                    }
                }

                div { class: "sidebar-scopes", role: "tablist", aria_label: "Filtro de conversas",
                    for option in ChatScope::ALL {
                        button {
                            key: "{option.label()}",
                            role: "tab",
                            aria_selected: "{current_scope == option}",
                            class: if current_scope == option { "scope-pill active" } else { "scope-pill" },
                            onclick: move |_| {
                                scope.set(option);
                                scroll_top.set(0.0);
                            },
                            "{option.label()}"
                        }
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
                                "Carregando suas conversas…"
                            } else {
                                "Nenhuma conversa neste filtro."
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
                                    active: active_tab == Tab::Telegram
                                        && selected_id.as_deref() == Some(chat.id.as_str()),
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

            SidebarFooterBar {
                active_downloads,
                failed_downloads,
                on_open_queue: {
                    let services = services.clone();
                    move |_| open_drawer_tab(&services, ui, DrawerTab::Queue)
                },
            }
        }
    }
}

#[component]
pub(crate) fn SidebarFooterBar(
    active_downloads: usize,
    failed_downloads: bool,
    on_open_queue: EventHandler<()>,
) -> Element {
    let mut ui = use_context::<UiState>();
    let tab = ui.shell.tab();
    let in_downloads = matches!(tab, Tab::Downloads | Tab::Twitter);
    let mut dl_class = String::from("sidebar-nav-btn");
    if in_downloads {
        dl_class.push_str(" active");
    }
    if failed_downloads {
        dl_class.push_str(" failed");
    }

    rsx! {
        nav { class: "sidebar-footer", aria_label: "Seções do aplicativo",
            button {
                class: if tab == Tab::Telegram { "sidebar-nav-btn active" } else { "sidebar-nav-btn" },
                title: "Conversas",
                onclick: move |_| ui.shell.tab.set(Tab::Telegram),
                Icon { name: IconName::BrandTelegram, size: 17, class: "icon" }
                span { "Chats" }
            }
            button {
                class: "{dl_class}",
                title: "Downloads, Links & X/Twitter (clique direito abre fila rápida)",
                onclick: move |_| {
                    ui.shell.tab.set(Tab::Downloads);
                    ui.conversation.mobile_show_list.set(false);
                },
                oncontextmenu: move |event: Event<MouseData>| {
                    event.prevent_default();
                    on_open_queue.call(());
                },
                Icon { name: IconName::ProgressDown, size: 17, class: "icon" }
                span {
                    if active_downloads > 0 {
                        "Downloads ({active_downloads})"
                    } else {
                        "Downloads"
                    }
                }
                if active_downloads > 0 || failed_downloads {
                    span { class: "sidebar-nav-dot" }
                }
            }
            button {
                class: if tab == Tab::Settings {
                    "icon-button sidebar-settings-btn active"
                } else {
                    "icon-button sidebar-settings-btn"
                },
                title: "Ajustes e Estilos (Ctrl+K para comandos)",
                aria_label: "Ajustes",
                onclick: move |_| {
                    ui.shell.tab.set(Tab::Settings);
                    ui.conversation.mobile_show_list.set(false);
                },
                Icon { name: IconName::Settings, size: 18, class: "icon" }
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
