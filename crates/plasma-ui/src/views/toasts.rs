//! Transient notifications shared by every view.

use crate::state::UiState;
use dioxus::prelude::*;

#[component]
pub fn ToastStack() -> Element {
    let mut ui = use_context::<UiState>();
    let items = ui.shell.toasts.read().clone();

    rsx! {
        div { class: "toast-stack",
            for toast in items {
                {
                    let id = toast.id;
                    let kind = toast.kind.class();
                    rsx! {
                        div { key: "{id}", class: "toast {kind}",
                            span { "{toast.message}" }
                            button {
                                title: "Fechar",
                                onclick: move |_| {
                                    let mut next = ui.shell.toasts.read().clone();
                                    next.retain(|item| item.id != id);
                                    ui.shell.toasts.set(next);
                                },
                                "×"
                            }
                        }
                    }
                }
            }
        }
    }
}
