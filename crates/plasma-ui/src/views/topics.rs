//! Topic list dialog: search, hide/show and pick a forum topic.
//!
//! Mirrors the topic list the React frontend shows in place of the timeline,
//! but as a dialog so the Rust shell keeps the message timeline visible behind
//! it. The layout follows the shell design system (modal card, search field,
//! list rows with avatars).

use crate::context::{ContextMenuState, ContextTarget};
use crate::icons::{Icon, IconName};
use crate::state::{topic_visibility_key, UiState};
use dioxus::prelude::*;
use plasma_telegram::{TdlibChatInfo, TdlibForumTopicInfo};

/// Same palette used by the React topic list, so avatars stay recognizable.
const TOPIC_COLORS: [(&str, &str); 8] = [
    ("#5ca9e6", "#7d95ff"),
    ("#e8786e", "#f5a623"),
    ("#6ec6b8", "#43a047"),
    ("#ab7ae6", "#e66fa0"),
    ("#f5a623", "#f7c948"),
    ("#5cb8e6", "#4fc3f7"),
    ("#e66fa0", "#ef5350"),
    ("#66bb6a", "#aed581"),
];

fn topic_gradient(id: i64) -> String {
    let index = (id.unsigned_abs() % TOPIC_COLORS.len() as u64) as usize;
    let (from, to) = TOPIC_COLORS[index];
    format!("linear-gradient(135deg, {from}, {to})")
}

fn is_hidden(hidden: &[String], topic: &TdlibForumTopicInfo) -> bool {
    let key = topic_visibility_key(topic);
    hidden.contains(&key)
}

/// Dialog listing the topics of a forum, with search and hidden-topic control.
#[component]
pub fn TopicBrowser(chat: TdlibChatInfo, on_close: EventHandler<()>) -> Element {
    let mut ui = use_context::<UiState>();
    let mut query = use_signal(String::new);
    let mut show_hidden = use_signal(|| false);
    let chat_id = chat.id.parse::<i64>().ok();

    let topics = ui.topics();
    let hidden = ui.hidden_topics();
    let selected = ui.selected_topic();
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

    let open_all = move |_| {
        ui.selected_topic.set(None);
        ui.reply_to.set(None);
        on_close.call(());
    };

    rsx! {
        div {
            class: "topic-browser-backdrop",
            onclick: move |_| on_close.call(()),
            section {
                class: "topic-browser-panel",
                role: "dialog",
                aria_modal: "true",
                aria_label: "Lista de tópicos",
                onclick: move |event| event.stop_propagation(),
                header { class: "topic-browser-header",
                    div {
                        strong { "Tópicos" }
                        span { class: "hint", "{chat.title}" }
                    }
                    button {
                        class: "icon-button",
                        title: "Fechar",
                        onclick: move |_| on_close.call(()),
                        Icon { name: IconName::Close, size: 16, class: "icon" }
                    }
                }
                div { class: "topic-browser-toolbar",
                    input {
                        class: "topic-browser-search",
                        r#type: "search",
                        placeholder: "Pesquisar tópicos...",
                        value: "{query}",
                        oninput: move |event| query.set(event.value()),
                    }
                    if hidden_count > 0 {
                        button {
                            class: if show_hidden() { "topic-browser-toggle active" } else { "topic-browser-toggle" },
                            aria_pressed: "{show_hidden()}",
                            onclick: move |_| {
                                let next = !show_hidden();
                                show_hidden.set(next);
                            },
                            Icon {
                                name: if show_hidden() { IconName::EyeOff } else { IconName::Eye },
                                size: 15,
                                class: "icon",
                            }
                            span {
                                if show_hidden() {
                                    "Ocultar tópicos ocultos"
                                } else {
                                    "Exibir tópicos ocultos ({hidden_count})"
                                }
                            }
                        }
                    }
                }
                div { class: "topic-browser-list",
                    if topics.is_empty() {
                        div { class: "topic-browser-empty",
                            "Nenhum tópico disponível nesta conversa."
                        }
                    } else {
                        button {
                            class: "topic-browser-item all",
                            onclick: open_all,
                            div { class: "topic-browser-avatar all",
                                Icon { name: IconName::Chat, size: 19, class: "icon" }
                            }
                            div { class: "topic-browser-copy",
                                strong { "Todos os tópicos" }
                                span { "Ver mensagens de todos os tópicos" }
                            }
                            if selected_id.is_none() {
                                span { class: "topic-browser-check",
                                    Icon { name: IconName::Check, size: 15, class: "icon" }
                                }
                            }
                        }
                        div { class: "topic-browser-divider" }
                        for topic in listing.iter().cloned() {
                            {
                                let hidden_now = is_hidden(&hidden, &topic);
                                let active = selected_id == Some(topic.id);
                                rsx! {
                                    button {
                                        key: "{topic.kind}:{topic.id}",
                                        class: if hidden_now { "topic-browser-item hidden" } else { "topic-browser-item" },
                                        onclick: {
                                            let topic = topic.clone();
                                            move |_| {
                                                ui.selected_topic.set(Some(topic.clone()));
                                                ui.reply_to.set(None);
                                                on_close.call(());
                                            }
                                        },
                                        oncontextmenu: {
                                            let topic = topic.clone();
                                            move |event: Event<MouseData>| {
                                                event.prevent_default();
                                                event.stop_propagation();
                                                let Some(chat_id) = chat_id else { return; };
                                                let point = event.data().client_coordinates();
                                                ui.context_menu.set(Some(ContextMenuState::new(
                                                    point.x,
                                                    point.y,
                                                    ContextTarget::Topic {
                                                        chat_id,
                                                        topic: Box::new(topic.clone()),
                                                        hidden: hidden_now,
                                                    },
                                                )));
                                            }
                                        },
                                        div {
                                            class: "topic-browser-avatar",
                                            style: "background: {topic_gradient(topic.id)};",
                                            "{topic.title.chars().next().unwrap_or('?').to_uppercase()}"
                                        }
                                        div { class: "topic-browser-copy",
                                            strong {
                                                "{topic.title}"
                                                if topic.is_pinned {
                                                    span { class: "topic-browser-pin", "📌" }
                                                }
                                            }
                                            span {
                                                if hidden_now {
                                                    span { class: "topic-browser-hidden-label", "Oculto · " }
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
                                            span { class: "topic-browser-check",
                                                Icon { name: IconName::Check, size: 15, class: "icon" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if listing.is_empty() {
                            div { class: "topic-browser-empty",
                                if search.is_empty() {
                                    "Todos os tópicos estão ocultos."
                                } else {
                                    "Nenhum tópico corresponde à pesquisa."
                                }
                            }
                        }
                    }
                }
                footer { class: "topic-browser-footer",
                    span { class: "hint",
                        "{topics.len()} tópicos"
                        if hidden_count > 0 {
                            " · {hidden_count} ocultos"
                        }
                    }
                    button {
                        class: "btn small ghost",
                        onclick: move |_| on_close.call(()),
                        "Fechar"
                    }
                }
            }
        }
    }
}
