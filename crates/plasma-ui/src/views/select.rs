//! Custom select control.
//!
//! The React frontend uses a custom dropdown (instead of the native `select`)
//! so it can draw the chevron and the option list with the same design system
//! tokens. This component is the Rust equivalent and uses the shell icons.

use crate::icons::{Icon, IconName};
use dioxus::prelude::*;

/// A single option of the select.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }
}

/// Select with the visual language of the React design system.
#[component]
pub fn Select(
    value: String,
    options: Vec<SelectOption>,
    placeholder: String,
    disabled: bool,
    on_change: EventHandler<String>,
) -> Element {
    let mut open = use_signal(|| false);
    let selected = options
        .iter()
        .find(|option| option.value == value)
        .map(|option| option.label.clone());

    rsx! {
        div { class: if open() { "select open" } else { "select" },
            button {
                r#type: "button",
                class: "select-trigger",
                disabled: disabled,
                aria_haspopup: "listbox",
                aria_expanded: "{open()}",
                // The dropdown opens on `mousedown`: the overlay that covers the
                // trigger is created right after, so a `click` would land on it
                // and close the list right away.
                onmousedown: move |_| {
                    if !disabled {
                        open.set(!open());
                    }
                },
                onkeydown: move |event: KeyboardEvent| {
                    let is_space = matches!(&event.key(), Key::Character(character) if character == " ");
                    if event.key() == Key::Enter || is_space {
                        event.prevent_default();
                        if !disabled {
                            open.set(!open());
                        }
                    }
                },
                span { class: "select-value",
                    match &selected {
                        Some(label) => rsx! { "{label}" },
                        None => rsx! { "{placeholder}" },
                    }
                }
                span { class: "select-chevron",
                    Icon { name: IconName::ChevronDown, size: 16, class: "icon" }
                }
            }
            if open() {
                div {
                    class: "select-backdrop",
                    onmousedown: move |_| open.set(false),
                }
                div { class: "select-options", role: "listbox",
                    for option in options {
                        button {
                            r#type: "button",
                            class: if option.value == value { "select-option selected" } else { "select-option" },
                            role: "option",
                            aria_selected: "{option.value == value}",
                            onmousedown: {
                                let value = option.value.clone();
                                move |_| {
                                    on_change.call(value.clone());
                                    open.set(false);
                                }
                            },
                            span { class: "select-option-label", "{option.label}" }
                            if option.value == value {
                                span { class: "select-check",
                                    Icon { name: IconName::Check, size: 14, class: "icon" }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
