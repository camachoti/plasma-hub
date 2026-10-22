//! Custom select control.
//!
//! The React frontend uses a custom dropdown (instead of the native `select`)
//! so it can draw the chevron and the option list with the same design system
//! tokens. This component is the Rust equivalent and uses the shell icons.
//!
//! The option list is rendered at the app root (like the context menu) so it is
//! never clipped by modals or scrollable panels: a menu inside a `.modal-card`
//! with `overflow: hidden` used to disappear under the panel.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::icons::{Icon, IconName};
use crate::state::UiState;
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

/// Open option list, positioned in viewport coordinates.
#[derive(Clone, PartialEq)]
pub struct SelectPortal {
    /// Identifies the select that owns the menu.
    pub id: u64,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub options: Vec<SelectOption>,
    pub value: String,
    pub on_change: EventHandler<String>,
}

static NEXT_SELECT_ID: AtomicU64 = AtomicU64::new(1);

/// Measures the trigger and publishes the option list at the app root.
fn open_select(
    mut ui: UiState,
    id: u64,
    options: Vec<SelectOption>,
    value: String,
    on_change: EventHandler<String>,
) {
    spawn(async move {
        let script = format!(
            r#"
            const element = document.querySelector('[data-select="{id}"]');
            if (!element) return null;
            const rect = element.getBoundingClientRect();
            return {{
                x: rect.left,
                y: rect.top,
                width: rect.width,
                height: rect.height,
                below: window.innerHeight - rect.bottom,
                above: rect.top,
            }};
            "#
        );
        let Ok(result) = document::eval(&script).await else {
            return;
        };
        let Some(rect) = result.as_object() else {
            return;
        };
        let number = |key: &str| {
            rect.get(key)
                .and_then(|value| value.as_f64())
                .unwrap_or(0.0)
        };
        let top = number("y");
        let height = number("height");
        let below = number("below");
        let above = number("above");
        // Roughly one row per option plus the menu padding; flip upwards when the
        // control is too close to the bottom of the window.
        let menu_height = (options.len() as f64 * 34.0).min(260.0) + 12.0;
        let y = if below < menu_height && above > below {
            (top - menu_height - 4.0).max(8.0)
        } else {
            top + height + 4.0
        };
        ui.select_portal.set(Some(SelectPortal {
            id,
            x: number("x"),
            y,
            width: number("width"),
            options,
            value,
            on_change,
        }));
    });
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
    let mut ui = use_context::<UiState>();
    let id = use_hook(|| NEXT_SELECT_ID.fetch_add(1, Ordering::Relaxed));
    let selected = options
        .iter()
        .find(|option| option.value == value)
        .map(|option| option.label.clone());
    let open = ui
        .select_portal()
        .map(|portal| portal.id == id)
        .unwrap_or(false);

    // Closing a modal unmounts the control; drop its stale menu with it.
    use_drop(move || {
        if ui.select_portal().map(|portal| portal.id) == Some(id) {
            ui.select_portal.set(None);
        }
    });

    rsx! {
        div { class: if open { "select open" } else { "select" },
            "data-select": "{id}",
            button {
                r#type: "button",
                class: "select-trigger",
                disabled: disabled,
                aria_haspopup: "listbox",
                aria_expanded: "{open}",
                onmousedown: {
                    let options = options.clone();
                    let value = value.clone();
                    move |_| {
                        if disabled {
                            return;
                        }
                        if ui.select_portal().map(|portal| portal.id) == Some(id) {
                            ui.select_portal.set(None);
                            return;
                        }
                        open_select(ui, id, options.clone(), value.clone(), on_change);
                    }
                },
                onkeydown: {
                    let options = options.clone();
                    let value = value.clone();
                    move |event: KeyboardEvent| {
                        let is_space =
                            matches!(&event.key(), Key::Character(character) if character == " ");
                        if event.key() == Key::Enter || is_space {
                            event.prevent_default();
                            if disabled {
                                return;
                            }
                            if ui.select_portal().map(|portal| portal.id) == Some(id) {
                                ui.select_portal.set(None);
                            } else {
                                open_select(ui, id, options.clone(), value.clone(), on_change);
                            }
                        } else if event.key() == Key::Escape {
                            ui.select_portal.set(None);
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
        }
    }
}

/// Option list rendered at the app root.
#[component]
pub fn SelectMenu(portal: SelectPortal, on_close: EventHandler<()>) -> Element {
    let on_change = portal.on_change;
    rsx! {
        div {
            class: "select-portal-backdrop",
            onmousedown: move |_| on_close.call(()),
        }
        div {
            class: "select-portal-menu",
            role: "listbox",
            style: "left: {portal.x}px; top: {portal.y}px; width: {portal.width}px;",
            onmousedown: move |event| event.stop_propagation(),
            for option in portal.options.iter().cloned() {
                button {
                    r#type: "button",
                    class: if option.value == portal.value { "select-option selected" } else { "select-option" },
                    role: "option",
                    aria_selected: "{option.value == portal.value}",
                    onmousedown: {
                        let value = option.value.clone();
                        move |_| {
                            on_change.call(value.clone());
                            on_close.call(());
                        }
                    },
                    span { class: "select-option-label", "{option.label}" }
                    if option.value == portal.value {
                        span { class: "select-check",
                            Icon { name: IconName::Check, size: 14, class: "icon" }
                        }
                    }
                }
            }
        }
    }
}
