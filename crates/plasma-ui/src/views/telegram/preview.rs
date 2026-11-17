//! Local preview shown before Telegram is ready.

use crate::icons::{Icon, IconName};
use crate::state::{push_toast, ToastKind, UiState};
use crate::views::spike::VirtualTimeline;
use dioxus::prelude::*;

/* ------------------------------------------------------------------------- */
/* Preview before login                                                      */
/* ------------------------------------------------------------------------- */

#[derive(Clone, Copy)]
pub(super) struct PreviewChat {
    name: &'static str,
    preview: &'static str,
    time: &'static str,
    unread: u32,
    tone: &'static str,
}

pub(super) const PREVIEW_CHATS: [PreviewChat; 6] = [
    PreviewChat {
        name: "Plasma Hub",
        preview: "A migração para Rust começou 🚀",
        time: "agora",
        unread: 3,
        tone: "P",
    },
    PreviewChat {
        name: "Design System",
        preview: "Paletas, densidade e tokens prontos.",
        time: "09:42",
        unread: 0,
        tone: "D",
    },
    PreviewChat {
        name: "Android",
        preview: "NDK 27 e target aarch64 validados.",
        time: "ontem",
        unread: 1,
        tone: "A",
    },
    PreviewChat {
        name: "Rust Rewrite",
        preview: "Fase 3: TDLib, timeline e cache.",
        time: "ontem",
        unread: 0,
        tone: "R",
    },
    PreviewChat {
        name: "Downloads",
        preview: "Fila, retry e histórico na fase 5.",
        time: "seg",
        unread: 0,
        tone: "↓",
    },
    PreviewChat {
        name: "Twitter / X",
        preview: "Cookies continuam em disco.",
        time: "seg",
        unread: 0,
        tone: "X",
    },
];

#[component]
pub(super) fn PreviewTelegram() -> Element {
    let mut ui = use_context::<UiState>();
    let mut selected = use_signal(|| 0usize);
    let mut show_list = use_signal(|| true);
    let chat = PREVIEW_CHATS[selected()];
    let layout_class = if show_list() {
        "telegram-layout mobile-list"
    } else {
        "telegram-layout mobile-chat"
    };

    rsx! {
        div { class: "{layout_class}",
            aside { class: "chat-list-panel",
                header { class: "chat-list-header",
                    div { class: "chat-list-title-row",
                        h2 { "Conversas" }
                        span { class: "pill", "prévia" }
                    }
                }
                div { class: "chat-list",
                    for (index, item) in PREVIEW_CHATS.iter().enumerate() {
                        button {
                            key: "{item.name}",
                            class: if selected() == index { "chat-row active" } else { "chat-row" },
                            onclick: move |_| {
                                selected.set(index);
                                show_list.set(false);
                                ui.shell.show_spike.set(false);
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
                    div { class: "chat-header-title",
                        button {
                            class: "mobile-nav-back",
                            title: "Voltar para a lista de chats",
                            aria_label: "Voltar para a lista de chats",
                            onclick: move |_| show_list.set(true),
                            Icon { name: IconName::ChevronLeft, size: 18, class: "icon" }
                        }
                        div { class: "chat-header-copy",
                            strong { "{chat.name}" }
                            span { "Prévia local · entre para carregar suas conversas" }
                        }
                    }
                    div { class: "topbar-actions",
                        button {
                            class: "btn small ghost",
                            onclick: move |_| {
                                let next = !ui.shell.show_spike();
                                ui.shell.show_spike.set(next);
                                if next {
                                    push_toast(
                                        ui.shell.toasts,
                                        "Spike: 5.000 mensagens com alturas variáveis.",
                                        ToastKind::Info,
                                    );
                                }
                            },
                            if ui.shell.show_spike() { "Fechar spike" } else { "Spike 5k" }
                        }
                        button {
                            class: "btn small ghost",
                            onclick: move |_| {
                                let next = ui.conversation.telegram_tick().wrapping_add(1);
                                ui.conversation.telegram_tick.set(next);
                                ui.shell.logged_in.set(false);
                            },
                            "Entrar"
                        }
                        button {
                            class: "btn small primary",
                            onclick: move |_| push_toast(
                                ui.shell.toasts,
                                "Composer, envio e mídia entram na fase 4.",
                                ToastKind::Info,
                            ),
                            "Nova mensagem"
                        }
                    }
                }
                div { class: "chat-body",
                    if ui.shell.show_spike() {
                        VirtualTimeline {}
                    } else {
                        div { class: "empty-state",
                            div { class: "empty-card",
                                div { class: "empty-icon", "P" },
                                h2 { "{chat.name}" }
                                p {
                                    "Esta é a prévia da interface Rust. Entre com o Telegram para \
                                     carregar conversas, tópicos, busca e timeline reais."
                                }
                                div { class: "mock-messages",
                                    div { class: "mock-message",
                                        strong { "Ana" }
                                        p { "O backend já está em crates separados e o Tauri continua funcionando." }
                                    }
                                    div { class: "mock-message",
                                        strong { "Bruno" }
                                        p { "A timeline real usa virtualização e cache SQLite, sem mudar o app React." }
                                    }
                                    div { class: "mock-message own",
                                        strong { "Você" }
                                        p { "O shell Rust abre, autentica no TDLib e lê mensagens." }
                                    }
                                }
                                div { class: "login-actions",
                                    button {
                                        class: "btn primary",
                                        onclick: move |_| ui.shell.logged_in.set(false),
                                        "Entrar com Telegram"
                                    }
                                    button {
                                        class: "btn ghost",
                                        onclick: move |_| ui.shell.show_spike.set(true),
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
}
