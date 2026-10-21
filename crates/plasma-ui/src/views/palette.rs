//! Command palette (Ctrl/Cmd+K action list).

use crate::state::{apply_palette, push_toast, AppServices, Tab, ToastKind, UiState};
use crate::theme::Palette;
use dioxus::prelude::*;

#[derive(Clone)]
enum Action {
    Tab(Tab),
    ToggleSpike,
    Palette(Palette),
    Logout,
}

struct Item {
    title: String,
    hint: &'static str,
    action: Action,
}

fn items() -> Vec<Item> {
    let mut items = vec![
        Item {
            title: "Abrir Telegram".into(),
            hint: "Ir para a aba",
            action: Action::Tab(Tab::Telegram),
        },
        Item {
            title: "Abrir Downloads".into(),
            hint: "Ir para a aba",
            action: Action::Tab(Tab::Downloads),
        },
        Item {
            title: "Abrir Twitter / X".into(),
            hint: "Ir para a aba",
            action: Action::Tab(Tab::Twitter),
        },
        Item {
            title: "Abrir Ajustes".into(),
            hint: "Ir para a aba",
            action: Action::Tab(Tab::Settings),
        },
        Item {
            title: "Abrir spike de virtualização".into(),
            hint: "5.000 mensagens",
            action: Action::ToggleSpike,
        },
    ];

    for palette in Palette::ALL {
        items.push(Item {
            title: format!("Paleta {}", palette.label()),
            hint: "Aparência",
            action: Action::Palette(palette),
        });
    }

    items.push(Item {
        title: "Sair para a tela de login".into(),
        hint: "Sessão",
        action: Action::Logout,
    });

    items
}

fn apply(action: Action, services: &AppServices, mut ui: UiState) {
    match action {
        Action::Tab(tab) => ui.tab.set(tab),
        Action::ToggleSpike => {
            ui.tab.set(Tab::Telegram);
            ui.show_spike.set(true);
        }
        Action::Palette(palette) => apply_palette(services, ui, palette),
        Action::Logout => {
            ui.logged_in.set(false);
            ui.show_palette.set(false);
            push_toast(ui.toasts, "Sessão encerrada (demo).", ToastKind::Info);
        }
    }
    ui.show_palette.set(false);
}

#[component]
pub fn CommandPalette() -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    if !ui.show_palette() {
        return rsx! {};
    }

    let mut query = use_signal(String::new);
    let needle = query().to_lowercase();
    let filtered: Vec<Item> = items()
        .into_iter()
        .filter(|item| needle.is_empty() || item.title.to_lowercase().contains(&needle))
        .collect();
    let first_action = filtered.first().map(|item| item.action.clone());
    let keyboard_services = services.clone();

    rsx! {
        div { class: "modal-backdrop",
            div { class: "command-modal",
                input {
                    class: "command-input",
                    r#type: "text",
                    placeholder: "Buscar ações… (Esc para fechar)",
                    autofocus: true,
                    value: query(),
                    oninput: move |event| query.set(event.value()),
                    onkeydown: move |event| {
                        match event.key() {
                            Key::Escape => ui.show_palette.set(false),
                            Key::Enter => {
                                if let Some(action) = first_action.clone() {
                                    apply(action, &keyboard_services, ui);
                                }
                            }
                            _ => {}
                        }
                    },
                }
                div { class: "command-list",
                    if filtered.is_empty() {
                        div { class: "command-empty", "Nenhuma ação encontrada." }
                    } else {
                        for item in filtered {
                            {
                                let action = item.action.clone();
                                let row_services = services.clone();
                                rsx! {
                                    button {
                                        key: "{item.title}",
                                        class: "command-item",
                                        onclick: move |_| apply(action.clone(), &row_services, ui),
                                        span { "{item.title}" }
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
