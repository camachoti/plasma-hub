//! Forward-message dialog with live destination filtering.

use crate::icons::{Icon, IconName};
use crate::state::ForwardState;
use crate::theme::{avatar_gradient, avatar_initials};
use dioxus::prelude::*;
use plasma_telegram::TdlibChatInfo;

use super::conversation::chat_kind;

#[component]
pub(super) fn ForwardDialog(
    state: ForwardState,
    chats: Vec<TdlibChatInfo>,
    busy: bool,
    on_close: EventHandler<()>,
    on_forward: EventHandler<TdlibChatInfo>,
) -> Element {
    let mut filter = use_signal(String::new);
    let needle = filter().trim().to_lowercase();
    let visible: Vec<TdlibChatInfo> = chats
        .into_iter()
        .filter(|chat| !crate::twitter_fake::is_twitter_fake_chat_id(&chat.id))
        .filter(|chat| chat.can_send_messages)
        .filter(|chat| needle.is_empty() || chat.title.to_lowercase().contains(&needle))
        .collect();

    rsx! {
        div {
            class: "forward-backdrop",
            onclick: move |_| on_close.call(()),
            section {
                class: "modal-card",
                role: "dialog",
                aria_modal: "true",
                aria_label: "Encaminhar mensagem",
                onclick: move |event| event.stop_propagation(),
                header { class: "modal-header",
                    div {
                        strong { "Encaminhar mensagem" }
                        p { class: "hint", "De {state.message.sender_name}" }
                    }
                    button {
                        class: "icon-button",
                        title: "Fechar",
                        aria_label: "Fechar",
                        onclick: move |_| on_close.call(()),
                        Icon { name: IconName::Close, size: 16, class: "icon" }
                    }
                }
                div { class: "pill-search",
                    Icon { name: IconName::Search, size: 16, class: "icon" }
                    input {
                        r#type: "search",
                        placeholder: "Buscar conversa de destino…",
                        autofocus: true,
                        value: "{filter}",
                        oninput: move |event| filter.set(event.value()),
                    }
                }
                if !state.message.text.trim().is_empty() {
                    p { class: "hint forward-excerpt", "{state.message.text}" }
                }
                div { class: "modal-list",
                    if visible.is_empty() {
                        p { class: "hint", "Nenhuma conversa disponível para encaminhar." }
                    } else {
                        for chat in visible {
                            {
                                let grad = avatar_gradient(&chat.id);
                                let mono = avatar_initials(&chat.title);
                                rsx! {
                                    button {
                                        key: "{chat.id}",
                                        class: "chat-row",
                                        disabled: busy,
                                        onclick: {
                                            let chat = chat.clone();
                                            move |_| on_forward.call(chat.clone())
                                        },
                                        div {
                                            class: "chat-avatar",
                                            style: "background: {grad};",
                                            "{mono}"
                                        }
                                        div { class: "chat-copy",
                                            span { class: "chat-name", "{chat.title}" }
                                            span { class: "chat-preview", "{chat_kind(&chat)}" }
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
