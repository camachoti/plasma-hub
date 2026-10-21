//! Telegram shell: chat list, placeholder timeline and the virtualization spike.

use crate::state::{push_toast, ToastKind, UiState};
use crate::views::spike::VirtualTimeline;
use dioxus::prelude::*;

#[derive(Clone, Copy)]
struct Chat {
    name: &'static str,
    preview: &'static str,
    time: &'static str,
    unread: u32,
    tone: &'static str,
}

const CHATS: [Chat; 6] = [
    Chat {
        name: "Plasma Hub",
        preview: "A migração para Rust começou 🚀",
        time: "agora",
        unread: 3,
        tone: "P",
    },
    Chat {
        name: "Design System",
        preview: "Paletas, densidade e tokens prontos.",
        time: "09:42",
        unread: 0,
        tone: "D",
    },
    Chat {
        name: "Android",
        preview: "NDK 27 e target aarch64 validados.",
        time: "ontem",
        unread: 1,
        tone: "A",
    },
    Chat {
        name: "Rust Rewrite",
        preview: "Fase 2: shell + spike de virtualização.",
        time: "ontem",
        unread: 0,
        tone: "R",
    },
    Chat {
        name: "Downloads",
        preview: "Fila, retry e histórico na fase 5.",
        time: "seg",
        unread: 0,
        tone: "↓",
    },
    Chat {
        name: "Twitter / X",
        preview: "Cookies continuam em disco.",
        time: "seg",
        unread: 0,
        tone: "X",
    },
];

#[component]
pub fn TelegramView() -> Element {
    let mut ui = use_context::<UiState>();
    let mut selected = use_signal(|| 0usize);
    let chat = CHATS[selected()];

    rsx! {
        div { class: "telegram-layout",
            aside { class: "chat-list-panel",
                header { class: "chat-list-header",
                    h2 { "Conversas" }
                }
                div { class: "chat-list",
                    for (index, item) in CHATS.iter().enumerate() {
                        button {
                            key: "{item.name}",
                            class: if selected() == index { "chat-row active" } else { "chat-row" },
                            onclick: move |_| {
                                selected.set(index);
                                ui.show_spike.set(false);
                            },
                            div { class: "chat-avatar", "{item.tone}" }
                            div { class: "chat-copy",
                                span { class: "chat-name", "{item.name}" }
                                span { class: "chat-preview", "{item.preview}" }
                            }
                            div { class: "chat-meta",
                                span { class: "chat-time", "{item.time}" }
                                if item.unread > 0 {
                                    span { class: "unread", "{item.unread}" }
                                }
                            }
                        }
                    }
                }
            }

            main { class: "chat-main",
                header { class: "chat-header",
                    div { class: "chat-header-copy",
                        strong { "{chat.name}" }
                        span { "{chat.preview}" }
                    }
                    div { class: "topbar-actions",
                        button {
                            class: "btn small ghost",
                            onclick: move |_| {
                                let next = !ui.show_spike();
                                ui.show_spike.set(next);
                                if next {
                                    push_toast(
                                        ui.toasts,
                                        "Spike: 5.000 mensagens com alturas variáveis.",
                                        ToastKind::Info,
                                    );
                                }
                            },
                            if ui.show_spike() { "Fechar spike" } else { "Spike 5k" }
                        }
                        button {
                            class: "btn small primary",
                            onclick: move |_| push_toast(
                                ui.toasts,
                                "Composer, envio e mídia entram na fase 4.",
                                ToastKind::Info,
                            ),
                            "Nova mensagem"
                        }
                    }
                }
                div { class: "chat-body",
                    if ui.show_spike() {
                        VirtualTimeline {}
                    } else {
                        div { class: "empty-state",
                            div { class: "empty-card",
                                div { class: "empty-icon", "P" }
                                h2 { "{chat.name}" }
                                p {
                                    "Timeline de demonstração. A conversa real com TDLib, \
                                     tópicos e cache SQLite entra na fase 3."
                                }
                                div { class: "mock-messages",
                                    div { class: "mock-message",
                                        strong { "Ana" }
                                        p { "O backend já está em crates separados e o Tauri continua funcionando." }
                                    }
                                    div { class: "mock-message",
                                        strong { "Bruno" }
                                        p { "Os 62 comandos foram preservados; o frontend React não precisou mudar." }
                                    }
                                    div { class: "mock-message own",
                                        strong { "Você" }
                                        p { "Agora o shell Rust abre, navega e persiste as preferências no SQLite." }
                                    }
                                }
                                button {
                                    class: "btn primary",
                                    onclick: move |_| ui.show_spike.set(true),
                                    "Abrir spike de virtualização"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
