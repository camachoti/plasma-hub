//! Local preview components shown when browsing without an authenticated TDLib session.

use super::chat_column::SidebarFooterBar;
use crate::icons::{Icon, IconName};
use crate::state::{open_drawer_tab, push_toast, AppServices, DrawerTab, Tab, ToastKind, UiState};
use crate::theme::{avatar_gradient, avatar_initials};
use crate::views::spike::VirtualTimeline;
use dioxus::prelude::*;

#[derive(Clone, Copy)]
pub(super) struct PreviewChat {
    pub(super) name: &'static str,
    pub(super) preview: &'static str,
    pub(super) time: &'static str,
    pub(super) unread: u32,
}

pub(super) const PREVIEW_CHATS: [PreviewChat; 5] = [
    PreviewChat {
        name: "Plasma Hub",
        preview: "Bem-vindo ao novo mensageiro multi-estilo ✨",
        time: "agora",
        unread: 2,
    },
    PreviewChat {
        name: "Design & Mídia",
        preview: "🖼 Teste os 4 estilos visuais em Ajustes",
        time: "14:20",
        unread: 0,
    },
    PreviewChat {
        name: "Central de Downloads",
        preview: "🎬 Baixe vídeos e álbuns sem sair da conversa",
        time: "ontem",
        unread: 1,
    },
    PreviewChat {
        name: "Galerias X / Twitter",
        preview: "Siga perfis do X como galerias sincronizadas",
        time: "seg",
        unread: 0,
    },
    PreviewChat {
        name: "Virtualização 5k",
        preview: "Timeline fluida com alturas medidas em tempo real",
        time: "seg",
        unread: 0,
    },
];

#[component]
pub(super) fn PreviewSidebar(mut selected: Signal<usize>) -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
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
        aside { class: "{sidebar_class}", aria_label: "Conversas de demonstração",
            div { class: "sidebar-top",
                div { class: "sidebar-search-bar",
                    div { class: "pill-search",
                        Icon { name: IconName::Search, size: 16, class: "icon" }
                        input {
                            r#type: "search",
                            placeholder: "Modo sem conta…",
                            disabled: true,
                        }
                    }
                    button {
                        class: "icon-button accent",
                        title: "Importar link",
                        onclick: move |_| {
                            ui.shell.tab.set(Tab::Downloads);
                            ui.conversation.mobile_show_list.set(false);
                        },
                        Icon { name: IconName::Plus, size: 17, class: "icon" }
                    }
                }
            }

            div { class: "sidebar-preview-banner",
                span { "Prévia sem login" }
                button {
                    class: "btn small primary",
                    onclick: move |_| ui.shell.logged_in.set(false),
                    "Conectar"
                }
            }

            div { class: "chat-list",
                for (index, item) in PREVIEW_CHATS.iter().enumerate() {
                    {
                        let gradient = avatar_gradient(item.name);
                        let initials = avatar_initials(item.name);
                        let is_active = active_tab == Tab::Telegram && selected() == index;
                        rsx! {
                            button {
                                key: "{item.name}",
                                class: if is_active { "chat-row active" } else { "chat-row" },
                                onclick: move |_| {
                                    selected.set(index);
                                    ui.shell.tab.set(Tab::Telegram);
                                    ui.conversation.mobile_show_list.set(false);
                                    ui.shell.show_spike.set(index == 4);
                                },
                                div {
                                    class: "chat-avatar",
                                    style: "background: {gradient};",
                                    span { "{initials}" }
                                }
                                div { class: "chat-copy",
                                    div { class: "chat-name-row",
                                        span { class: "chat-name", "{item.name}" }
                                        span { class: "chat-time", "{item.time}" }
                                    }
                                    div { class: "chat-preview-row",
                                        span { class: "chat-preview", "{item.preview}" }
                                        if item.unread > 0 {
                                            span { class: "unread", "{item.unread}" }
                                        }
                                    }
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
pub(super) fn PreviewStage(selected: Signal<usize>) -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let chat = PREVIEW_CHATS[selected().min(PREVIEW_CHATS.len() - 1)];
    let gradient = avatar_gradient(chat.name);
    let initials = avatar_initials(chat.name);

    rsx! {
        div { class: "chat-main",
            div { class: "chat-header-wrap",
                header { class: "chat-header",
                    div { class: "chat-header-title",
                        button {
                            class: "icon-button mobile-nav-back",
                            title: "Voltar para conversas",
                            aria_label: "Voltar para conversas",
                            onclick: move |_| ui.conversation.mobile_show_list.set(true),
                            Icon { name: IconName::ChevronLeft, size: 18, class: "icon" }
                        }
                        if !ui.shell.sidebar_open() {
                            button {
                                class: "icon-button",
                                title: "Mostrar lista de conversas (Ctrl+B)",
                                aria_label: "Mostrar lista de conversas",
                                onclick: move |_| ui.shell.sidebar_open.set(true),
                                Icon { name: IconName::LayoutSidebar, size: 18, class: "icon" }
                            }
                        }
                        div {
                            class: "chat-header-avatar",
                            style: "background: {gradient};",
                            "{initials}"
                        }
                        div { class: "chat-header-copy",
                            strong { "{chat.name}" }
                            span { "Modo demonstração · Conecte sua conta para sincronizar chats" }
                        }
                    }
                    div { class: "chat-header-actions",
                        button {
                            class: "btn small soft",
                            onclick: move |_| {
                                let next = !ui.shell.show_spike();
                                ui.shell.show_spike.set(next);
                                if next {
                                    push_toast(
                                        ui.shell.toasts,
                                        "Demonstrando 5.000 mensagens virtualizadas.",
                                        ToastKind::Info,
                                    );
                                }
                            },
                            if ui.shell.show_spike() { "Ver resumo" } else { "Testar 5k msgs" }
                        }
                        button {
                            class: "btn small primary",
                            onclick: move |_| ui.shell.logged_in.set(false),
                            "Conectar Telegram"
                        }
                        button {
                            class: if ui.shell.drawer_open() { "icon-button active" } else { "icon-button" },
                            title: "Alternar gaveta lateral (Ctrl+J)",
                            aria_label: "Alternar gaveta lateral",
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    let next = !ui.shell.drawer_open();
                                    crate::state::set_drawer_open(&services, ui, next);
                                }
                            },
                            Icon { name: IconName::LayoutSidebarRight, size: 18, class: "icon" }
                        }
                    }
                }
            }

            div { class: "chat-body",
                if ui.shell.show_spike() {
                    VirtualTimeline {}
                } else {
                    div { class: "empty-state",
                        div { class: "empty-card",
                            div { class: "empty-icon",
                                Icon { name: IconName::BrandTelegram, size: 24, class: "icon" }
                            }
                            h2 { "{chat.name}" }
                            p {
                                "Você está navegando sem conta conectada. Os downloads universais \
                                 e o analisador nativo do X/Twitter já estão 100% funcionais."
                            }
                            div { class: "mock-messages",
                                div { class: "mock-message",
                                    strong { "Helena · 14:18" }
                                    p { "Você pode alternar entre os 4 estilos visuais em Ajustes a qualquer momento." }
                                }
                                div { class: "mock-message own",
                                    strong { "Você · 14:20" }
                                    p { "E acompanhar a fila de downloads na gaveta à direita sem sair da conversa!" }
                                }
                            }
                            div { class: "login-actions",
                                button {
                                    class: "btn primary",
                                    onclick: move |_| ui.shell.logged_in.set(false),
                                    "Entrar no Telegram"
                                }
                                button {
                                    class: "btn ghost",
                                    onclick: move |_| ui.shell.tab.set(Tab::Settings),
                                    "Escolher Estilo Visual"
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
