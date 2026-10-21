//! Demo login screen for the Rust shell.
//!
//! Real TDLib authentication lands in phase 3. This screen only proves the
//! shell, theme and skip-login preference; it never touches the network.

use crate::state::{apply_palette, push_toast, AppServices, ToastKind, UiState};
use crate::theme::Palette;
use dioxus::prelude::*;
use plasma_store::SettingKey;

#[component]
pub fn LoginView() -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let mut logged_in = ui.logged_in;

    rsx! {
        div { class: "login-screen",
            div { class: "login-card",
                div { class: "brand-mark", "P" }
                h1 { "Plasma Hub" }
                p { class: "muted",
                    "Prévia da interface Rust. O backend, o banco e as preferências já são os mesmos do app atual."
                }

                div { class: "login-divider" }
                p { class: "hint", "Escolha uma paleta para testar o tema" }
                div { class: "palette-row",
                    for palette in Palette::ALL {
                        button {
                            key: "{palette.as_str()}",
                            class: if ui.palette() == palette { "palette-chip active" } else { "palette-chip" },
                            style: "--swatch: {palette.swatch()};",
                            onclick: {
                                let services = services.clone();
                                move |_| apply_palette(&services, ui, palette)
                            },
                            "{palette.label()}"
                        }
                    }
                }

                div { class: "login-actions",
                    button {
                        class: "btn primary",
                        onclick: move |_| {
                            logged_in.set(true);
                            push_toast(ui.toasts, "Sessão de demonstração iniciada.", ToastKind::Success);
                        },
                        "Entrar (demo)"
                    }
                    button {
                        class: "btn ghost",
                        onclick: {
                            let services = services.clone();
                            move |_| {
                                logged_in.set(true);
                                match services.settings.set_bool(SettingKey::SKIP_LOGIN, true) {
                                    Ok(()) => push_toast(
                                        ui.toasts,
                                        "Login pulado; a preferência foi salva no SQLite.",
                                        ToastKind::Info,
                                    ),
                                    Err(error) => push_toast(
                                        ui.toasts,
                                        format!("Login pulado, mas a preferência não foi salva: {error}"),
                                        ToastKind::Error,
                                    ),
                                }
                            }
                        },
                        "Pular login"
                    }
                }
                p { class: "hint",
                    "A autenticação real do TDLib entra na fase 3. Nada aqui acessa sua conta."
                }
            }
        }
    }
}
