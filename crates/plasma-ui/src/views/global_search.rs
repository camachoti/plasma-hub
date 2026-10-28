//! Cross-chat search results grouped by conversation.

use dioxus::prelude::*;
use plasma_telegram::TdlibGlobalSearchMessage;

use crate::views::timeline::{format_timestamp, reply_sender, reply_snippet};

struct GlobalSearchGroup {
    chat_id: String,
    chat_title: String,
    messages: Vec<TdlibGlobalSearchMessage>,
}

#[component]
pub fn GlobalSearchResults(
    query: String,
    results: Vec<TdlibGlobalSearchMessage>,
    on_open: EventHandler<TdlibGlobalSearchMessage>,
) -> Element {
    let total = results.len();
    let groups = group_by_chat(results);
    let group_count = groups.len();

    rsx! {
        div { class: "global-search-panel",
            div { class: "global-search-summary",
                span { class: "pill", "Busca global: {query}" }
                span { class: "hint", "{total} resultado(s) em {group_count} conversa(s)" }
            }
            if groups.is_empty() {
                div { class: "global-search-empty",
                    strong { "Nenhum resultado" }
                    span { class: "hint", "Nenhuma mensagem encontrada nas conversas." }
                }
            } else {
                for group in groups {
                    {
                        let GlobalSearchGroup {
                            chat_id,
                            chat_title,
                            messages,
                        } = group;
                        let count = messages.len();
                        let chat_title = if chat_title.trim().is_empty() {
                            "Conversa".to_string()
                        } else {
                            chat_title
                        };
                        rsx! {
                            section { class: "global-search-group", key: "{chat_id}",
                                div { class: "global-search-group-header",
                                    div {
                                        class: "global-search-avatar",
                                        "{chat_title.chars().next().unwrap_or('?')}"
                                    }
                                    div { class: "global-search-group-copy",
                                        strong { "{chat_title}" }
                                        span { class: "hint", "{count} resultado(s)" }
                                    }
                                }
                                for item in messages {
                                    {
                                        let message = item.message.clone();
                                        let target = item.clone();
                                        let row_key = format!("{}:{}", item.chat_id, item.message.id);
                                        rsx! {
                                            button {
                                                key: "{row_key}",
                                                r#type: "button",
                                                class: "global-search-row",
                                                onclick: move |_| on_open.call(target.clone()),
                                                div { class: "global-search-meta",
                                                    strong { "{reply_sender(&message)}" }
                                                    span { "{format_timestamp(message.date)}" }
                                                    if message.topic_id.is_some() {
                                                        span { class: "pill", "tópico" }
                                                    }
                                                }
                                                div {
                                                    class: "global-search-text",
                                                    "{reply_snippet(&message)}"
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

fn group_by_chat(results: Vec<TdlibGlobalSearchMessage>) -> Vec<GlobalSearchGroup> {
    let mut groups: Vec<GlobalSearchGroup> = Vec::new();
    for item in results {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| group.chat_id == item.chat_id)
        {
            group.messages.push(item);
        } else {
            groups.push(GlobalSearchGroup {
                chat_id: item.chat_id.clone(),
                chat_title: item.chat_title.clone(),
                messages: vec![item],
            });
        }
    }
    groups
}
