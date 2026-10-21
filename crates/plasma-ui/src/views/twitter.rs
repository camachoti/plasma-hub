//! Twitter/X view placeholder for the Rust shell.

use crate::state::{push_toast, ToastKind, UiState};
use dioxus::prelude::*;

#[component]
pub fn TwitterView() -> Element {
    let mut ui = use_context::<UiState>();

    rsx! {
        section { class: "view empty-state",
            div { class: "empty-card",
                div { class: "empty-icon", "X" }
                h2 { "Twitter / X" }
                p {
                    "A biblioteca, os cookies e o downloader nativo já vivem em \
                     plasma-twitter. A tela Rust será migrada junto com os downloads."
                }
                div { class: "login-actions",
                    button {
                        class: "btn primary",
                        onclick: move |_| push_toast(
                            ui.toasts,
                            "Os cookies continuam no arquivo twitter-cookies.json.",
                            ToastKind::Info,
                        ),
                        "Ver status"
                    }
                    button {
                        class: "btn ghost",
                        onclick: move |_| ui.tab.set(crate::state::Tab::Settings),
                        "Abrir ajustes"
                    }
                }
            }
        }
    }
}
