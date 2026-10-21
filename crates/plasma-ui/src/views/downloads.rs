//! Downloads view placeholder for the Rust shell.

use crate::state::{push_toast, ToastKind, UiState};
use dioxus::prelude::*;

#[component]
pub fn DownloadsView() -> Element {
    let mut ui = use_context::<UiState>();

    rsx! {
        section { class: "view empty-state",
            div { class: "empty-card",
                div { class: "empty-icon", "↓" }
                h2 { "Downloads" }
                p {
                    "A fila nativa, o histórico e os lotes serão ligados ao DownloadManager \
                     na fase 5. O banco e as tabelas de histórico já existem."
                }
                div { class: "login-actions",
                    button {
                        class: "btn primary",
                        onclick: move |_| push_toast(
                            ui.toasts,
                            "Placeholder: a fila de downloads chega na fase 5.",
                            ToastKind::Info,
                        ),
                        "Testar notificação"
                    }
                    button {
                        class: "btn ghost",
                        onclick: move |_| ui.tab.set(crate::state::Tab::Telegram),
                        "Voltar ao Telegram"
                    }
                }
            }
        }
    }
}
