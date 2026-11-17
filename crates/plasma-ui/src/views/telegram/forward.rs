//! Forward-message dialog.

use crate::state::ForwardState;
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
    rsx! {
        div { class: "forward-backdrop", onclick: move |_| on_close.call(()),
            section {
                class: "modal-card",
                onclick: move |event| event.stop_propagation(),
                header { class: "modal-header",
                    div {
                        strong { "Encaminhar mensagem" }
                        span { class: "hint", "{state.message.sender_name}" }
                    }
                    button {
                        class: "btn small ghost",
                        onclick: move |_| on_close.call(()),
                        "Fechar"
                    }
                }
                p { class: "hint", "{state.message.text}" }
                div { class: "modal-list",
                    for chat in chats {
                        button {
                            key: "{chat.id}",
                            class: "chat-row",
                            disabled: busy,
                            onclick: {
                                let chat = chat.clone();
                                move |_| on_forward.call(chat.clone())
                            },
                            div { class: "chat-avatar", "{chat.title.chars().next().unwrap_or('?')}" }
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
