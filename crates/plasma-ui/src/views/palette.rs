//! Quick action & conversation launcher (`Ctrl/Cmd + K`).

use crate::icons::{Icon, IconName};
use crate::state::{
    apply_color_mode, apply_style, open_drawer_tab, push_toast, set_drawer_open, AppServices,
    DrawerTab, Tab, ToastKind, UiState,
};
use crate::theme::{ColorMode, UiStyle};
use crate::views::chat_list;
use dioxus::prelude::*;
use plasma_telegram::TdlibChatInfo;

#[derive(Clone)]
enum Action {
    Tab(Tab),
    OpenChat(TdlibChatInfo),
    ToggleSidebar,
    ToggleDrawer,
    OpenDrawer(DrawerTab),
    Style(UiStyle),
    Mode(ColorMode),
    #[cfg(debug_assertions)]
    ToggleSpike,
    Logout,
}

#[derive(Clone)]
struct Item {
    key: String,
    title: String,
    hint: String,
    icon: IconName,
    action: Action,
}

fn build_items(chats: &[TdlibChatInfo], query: &str) -> Vec<Item> {
    let mut items = Vec::new();
    let needle = query.trim().to_lowercase();

    // When typing, surface matching conversations first for instant jumping.
    if !needle.is_empty() {
        for chat in chats
            .iter()
            .filter(|chat| chat.title.to_lowercase().contains(&needle))
            .take(8)
        {
            items.push(Item {
                key: format!("chat-{}", chat.id),
                title: chat.title.clone(),
                hint: "Ir para conversa".into(),
                icon: IconName::Chat,
                action: Action::OpenChat(chat.clone()),
            });
        }
    }

    let commands = [
        Item {
            key: "nav-telegram".into(),
            title: "Ir para Conversas".into(),
            hint: "Telegram & Galerias".into(),
            icon: IconName::BrandTelegram,
            action: Action::Tab(Tab::Telegram),
        },
        Item {
            key: "nav-reels".into(),
            title: "Abrir Reels".into(),
            hint: "Reddit, Instagram e links".into(),
            icon: IconName::Play,
            action: Action::Tab(Tab::Reels),
        },
        Item {
            key: "nav-downloads".into(),
            title: "Abrir Downloads (Links & X/Twitter)".into(),
            hint: "Central unificada".into(),
            icon: IconName::ProgressDown,
            action: Action::Tab(Tab::Downloads),
        },
        Item {
            key: "nav-twitter".into(),
            title: "Analisar Post ou Perfil do X / Twitter".into(),
            hint: "Downloads · Modo X".into(),
            icon: IconName::BrandTwitter,
            action: Action::Tab(Tab::Twitter),
        },
        Item {
            key: "nav-settings".into(),
            title: "Abrir Ajustes".into(),
            hint: "Estilos, Cache & Sessão".into(),
            icon: IconName::Settings,
            action: Action::Tab(Tab::Settings),
        },
        Item {
            key: "panel-drawer".into(),
            title: "Alternar Gaveta Lateral".into(),
            hint: "Ctrl + J".into(),
            icon: IconName::LayoutSidebarRight,
            action: Action::ToggleDrawer,
        },
        Item {
            key: "panel-queue".into(),
            title: "Ver Fila Rápida na Gaveta".into(),
            hint: "Painel direito".into(),
            icon: IconName::Download,
            action: Action::OpenDrawer(DrawerTab::Queue),
        },
        Item {
            key: "panel-sidebar".into(),
            title: "Mostrar / Ocultar Lista de Conversas".into(),
            hint: "Ctrl + B".into(),
            icon: IconName::LayoutSidebar,
            action: Action::ToggleSidebar,
        },
    ];

    for item in commands {
        if needle.is_empty() || item.title.to_lowercase().contains(&needle) {
            items.push(item);
        }
    }

    for style in UiStyle::ALL {
        let title = format!("Estilo: {}", style.label());
        if needle.is_empty() || title.to_lowercase().contains(&needle) {
            items.push(Item {
                key: format!("style-{}", style.as_str()),
                title,
                hint: "Aparência".into(),
                icon: IconName::Palette,
                action: Action::Style(style),
            });
        }
    }

    for mode in ColorMode::ALL {
        let title = format!("Modo {}", mode.label());
        if needle.is_empty() || title.to_lowercase().contains(&needle) {
            items.push(Item {
                key: format!("mode-{}", mode.as_str()),
                title,
                hint: "Tema de cor".into(),
                icon: if mode == ColorMode::Dark {
                    IconName::Moon
                } else {
                    IconName::Sun
                },
                action: Action::Mode(mode),
            });
        }
    }

    #[cfg(debug_assertions)]
    {
        let title = "Abrir spike de virtualização (5.000 mensagens)".to_string();
        if needle.is_empty() || title.to_lowercase().contains(&needle) {
            items.push(Item {
                key: "debug-spike".into(),
                title,
                hint: "Diagnóstico".into(),
                icon: IconName::Sparkles,
                action: Action::ToggleSpike,
            });
        }
    }

    let logout_title = "Encerrar sessão do Telegram".to_string();
    if needle.is_empty() || logout_title.to_lowercase().contains(&needle) {
        items.push(Item {
            key: "session-logout".into(),
            title: logout_title,
            hint: "Logout TDLib".into(),
            icon: IconName::Logout,
            action: Action::Logout,
        });
    }

    items
}

fn apply(action: Action, services: &AppServices, mut ui: UiState) {
    match action {
        Action::Tab(tab) => {
            ui.shell.tab.set(tab);
            if tab != Tab::Telegram {
                ui.conversation.mobile_show_list.set(false);
            }
        }
        Action::OpenChat(chat) => {
            chat_list::open_chat(services.clone(), ui, chat);
        }
        Action::ToggleSidebar => {
            let next = !ui.shell.sidebar_open();
            ui.shell.sidebar_open.set(next);
        }
        Action::ToggleDrawer => {
            let next = !ui.shell.drawer_open();
            set_drawer_open(services, ui, next);
        }
        Action::OpenDrawer(tab) => {
            open_drawer_tab(services, ui, tab);
        }
        Action::Style(style) => apply_style(services, ui, style),
        Action::Mode(mode) => apply_color_mode(services, ui, mode),
        #[cfg(debug_assertions)]
        Action::ToggleSpike => {
            ui.shell.tab.set(Tab::Telegram);
            ui.shell.show_spike.set(true);
        }
        Action::Logout => {
            ui.shell.show_palette.set(false);
            let services = services.clone();
            spawn(async move {
                match services.telegram.log_out().await {
                    Ok(_) => {
                        ui.shell.logged_in.set(false);
                        ui.session.preview_mode.set(false);
                        ui.session.auth_user.set(None);
                        ui.conversation.chats.set(Vec::new());
                        ui.conversation.selected_chat.set(None);
                        push_toast(
                            ui.shell.toasts,
                            "Sessão Telegram encerrada.",
                            ToastKind::Success,
                        );
                    }
                    Err(error) => push_toast(
                        ui.shell.toasts,
                        format!("Falha ao sair: {error}"),
                        ToastKind::Error,
                    ),
                }
            });
            return;
        }
    }
    ui.shell.show_palette.set(false);
}

#[component]
pub fn CommandPalette() -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let mut query = use_signal(String::new);
    let mut cursor = use_signal(|| 0usize);

    if !ui.shell.show_palette() {
        return rsx! {};
    }

    let chats = ui.conversation.chats();
    let filtered = build_items(&chats, &query());
    let active_idx = cursor().min(filtered.len().saturating_sub(1));
    let selected_action = filtered.get(active_idx).map(|item| item.action.clone());
    let len = filtered.len();
    let keyboard_services = services.clone();

    rsx! {
        div {
            class: "modal-backdrop",
            onclick: move |_| ui.shell.show_palette.set(false),
            div {
                class: "command-modal",
                role: "dialog",
                aria_modal: "true",
                aria_label: "Busca rápida e comandos",
                onclick: move |event| event.stop_propagation(),
                div { class: "command-search-row",
                    Icon { name: IconName::Search, size: 18, class: "icon" }
                    input {
                        class: "command-input",
                        r#type: "text",
                        placeholder: "Ir para conversa, mudar estilo ou executar ação…",
                        autofocus: true,
                        value: "{query}",
                        oninput: move |event| {
                            query.set(event.value());
                            cursor.set(0);
                        },
                        onkeydown: move |event| match event.key() {
                            Key::Escape => ui.shell.show_palette.set(false),
                            Key::ArrowDown => {
                                event.prevent_default();
                                if len > 0 {
                                    cursor.set((active_idx + 1) % len);
                                }
                            }
                            Key::ArrowUp => {
                                event.prevent_default();
                                if len > 0 {
                                    cursor.set((active_idx + len - 1) % len);
                                }
                            }
                            Key::Enter => {
                                event.prevent_default();
                                if let Some(action) = selected_action.clone() {
                                    apply(action, &keyboard_services, ui);
                                }
                            }
                            _ => {}
                        },
                    }
                    span { class: "pill", "Esc" }
                }
                div { class: "command-list",
                    if filtered.is_empty() {
                        div { class: "command-empty", "Nenhum resultado encontrado." }
                    } else {
                        for (index, item) in filtered.into_iter().enumerate() {
                            {
                                let action = item.action.clone();
                                let row_services = services.clone();
                                let is_active = index == active_idx;
                                rsx! {
                                    button {
                                        key: "{item.key}",
                                        class: if is_active { "command-item active" } else { "command-item" },
                                        onmouseenter: move |_| cursor.set(index),
                                        onclick: move |_| apply(action.clone(), &row_services, ui),
                                        div { class: "command-item-main",
                                            Icon { name: item.icon, size: 16, class: "icon" }
                                            span { "{item.title}" }
                                        }
                                        small { "{item.hint}" }
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
