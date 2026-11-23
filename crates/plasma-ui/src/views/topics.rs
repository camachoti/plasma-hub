//! Forum topic browser docked inside the Right Context Drawer.

use crate::context::{ContextMenuState, ContextTarget};
use crate::icons::{Icon, IconName};
use crate::state::{topic_visibility_key, UiState};
use crate::theme::avatar_gradient;
use dioxus::prelude::*;
use plasma_telegram::{TdlibChatInfo, TdlibForumTopicInfo};

fn is_hidden(hidden: &[String], topic: &TdlibForumTopicInfo) -> bool {
    let key = topic_visibility_key(topic);
    hidden.contains(&key)
}

/// Docked topic list for a forum, with search and hidden-topic visibility toggle.
#[component]
pub fn TopicBrowser(chat: TdlibChatInfo, on_close: EventHandler<()>) -> Element {
    let mut ui = use_context::<UiState>();
    let mut query = use_signal(String::new);
    let mut show_hidden = use_signal(|| false);
    let chat_id = chat.id.parse::<i64>().ok();

    let topics = ui.conversation.topics();
    let hidden = ui.conversation.hidden_topics();
    let selected = ui.conversation.selected_topic();
    let selected_id = selected.as_ref().map(|topic| topic.id);
    let search = query().trim().to_lowercase();
    let hidden_count = topics
        .iter()
        .filter(|topic| is_hidden(&hidden, topic))
        .count();
    let listing: Vec<TdlibForumTopicInfo> = topics
        .iter()
        .filter(|topic| show_hidden() || !is_hidden(&hidden, topic))
        .filter(|topic| search.is_empty() || topic.title.to_lowercase().contains(&search))
        .cloned()
        .collect();

    rsx! {
        section {
            class: "topic-browser-docked",
            aria_label: "Tópicos do fórum",
            div { class: "topic-browser-toolbar",
                div { class: "pill-search",
                    Icon { name: IconName::Search, size: 15, class: "icon" }
                    input {
                        r#type: "search",
                        placeholder: "Pesquisar tópicos…",
                        aria_label: "Pesquisar tópicos",
                        value: "{query}",
                        oninput: move |event| query.set(event.value()),
                    }
                }
                if hidden_count > 0 {
                    button {
                        class: if show_hidden() { "btn small active" } else { "btn small ghost" },
                        aria_pressed: "{show_hidden()}",
                        onclick: move |_| {
                            let next = !show_hidden();
                            show_hidden.set(next);
                        },
                        Icon {
                            name: if show_hidden() { IconName::EyeOff } else { IconName::Eye },
                            size: 14,
                            class: "icon",
                        }
                        span {
                            if show_hidden() {
                                "Ocultar ocultos"
                            } else {
                                "Exibir ocultos ({hidden_count})"
                            }
                        }
                    }
                }
            }

            div { class: "topic-browser-list",
                if topics.is_empty() {
                    div { class: "empty-state",
                        p { class: "hint", "Esta conversa não possui tópicos de fórum." }
                    }
                } else {
                    button {
                        class: if selected_id.is_none() { "topic-browser-item active" } else { "topic-browser-item" },
                        onclick: move |_| {
                            ui.conversation.selected_topic.set(None);
                            ui.panels.reply_to.set(None);
                        },
                        div { class: "topic-browser-avatar all",
                            Icon { name: IconName::Chat, size: 17, class: "icon" }
                        }
                        div { class: "topic-browser-copy",
                            strong { "Todos os tópicos" }
                            span { "Fluxo completo da conversa" }
                        }
                        if selected_id.is_none() {
                            Icon { name: IconName::Check, size: 15, class: "icon" }
                        }
                    }
                    div { class: "topic-browser-divider" }
                    for topic in listing.iter().cloned() {
                        {
                            let hidden_now = is_hidden(&hidden, &topic);
                            let active = selected_id == Some(topic.id);
                            let mut item_class = String::from("topic-browser-item");
                            if active {
                                item_class.push_str(" active");
                            }
                            if hidden_now {
                                item_class.push_str(" hidden");
                            }
                            let grad = avatar_gradient(&topic.title);
                            rsx! {
                                button {
                                    key: "{topic.kind}:{topic.id}",
                                    class: "{item_class}",
                                    onclick: {
                                        let topic = topic.clone();
                                        move |_| {
                                            ui.conversation.selected_topic.set(Some(topic.clone()));
                                            ui.panels.reply_to.set(None);
                                        }
                                    },
                                    oncontextmenu: {
                                        let topic = topic.clone();
                                        move |event: Event<MouseData>| {
                                            event.prevent_default();
                                            event.stop_propagation();
                                            let Some(chat_id) = chat_id else { return; };
                                            let point = event.data().client_coordinates();
                                            ui.panels.context_menu.set(Some(ContextMenuState::new(
                                                point.x,
                                                point.y,
                                                ContextTarget::Topic {
                                                    chat_id,
                                                    topic: Box::new(topic.clone()),
                                                    hidden: hidden_now,
                                                    browser_open: true,
                                                },
                                            )));
                                        }
                                    },
                                    div {
                                        class: "topic-browser-avatar",
                                        style: "background: {grad};",
                                        "{topic.title.chars().next().unwrap_or('?').to_uppercase()}"
                                    }
                                    div { class: "topic-browser-copy",
                                        strong {
                                            "{topic.title}"
                                            if topic.is_pinned { " 📌" }
                                        }
                                        span {
                                            if hidden_now {
                                                "Oculto · "
                                            }
                                            if topic.unread_count > 0 {
                                                "{topic.unread_count} não lidas"
                                                if topic.is_closed { " · Fechado" }
                                            } else if topic.is_closed {
                                                "Fechado"
                                            } else {
                                                "Aberto"
                                            }
                                        }
                                    }
                                    if active {
                                        Icon { name: IconName::Check, size: 15, class: "icon" }
                                    }
                                }
                            }
                        }
                    }
                    if listing.is_empty() {
                        p { class: "hint",
                            if search.is_empty() {
                                "Todos os tópicos estão ocultos."
                            } else {
                                "Nenhum tópico corresponde à pesquisa."
                            }
                        }
                    }
                }
            }

            footer { class: "media-panel-header",
                span { class: "hint",
                    "{topics.len()} tópicos"
                    if hidden_count > 0 {
                        " · {hidden_count} ocultos"
                    }
                }
                button {
                    class: "btn small ghost",
                    onclick: move |_| on_close.call(()),
                    "Ocultar painel"
                }
            }
        }
    }
}
