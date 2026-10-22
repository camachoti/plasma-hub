//! Floating right-click menu rendered on top of the shell.

use crate::context::ContextMenuEntry;
use crate::icons::Icon;
use dioxus::prelude::*;

const MENU_ID: &str = "plasma-context-menu";
const VIEWPORT_MARGIN: f64 = 10.0;

/// Context menu shown at the pointer position.
///
/// The component keeps the menu inside the viewport and supports keyboard
/// navigation: `ArrowUp`/`ArrowDown` move between the entries and `Escape`
/// closes the menu.
#[component]
pub fn ContextMenu(
    entries: Vec<ContextMenuEntry>,
    x: f64,
    y: f64,
    on_select: EventHandler<String>,
    on_close: EventHandler<()>,
) -> Element {
    let mut position = use_signal(|| (x, y));

    // Keep the menu fully inside the viewport.
    use_effect(move || {
        spawn(async move {
            let script = r#"
            const element = document.getElementById("plasma-context-menu");
            if (!element) { return null; }
            const rect = element.getBoundingClientRect();
            return {
                width: rect.width,
                height: rect.height,
                viewportWidth: window.innerWidth,
                viewportHeight: window.innerHeight,
            };
            "#;
            let Ok(value) = document::eval(script).await else {
                return;
            };
            if value.is_null() {
                return;
            }
            let width = value
                .get("width")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(220.0);
            let height = value
                .get("height")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(200.0);
            let viewport_width = value
                .get("viewportWidth")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(1280.0);
            let viewport_height = value
                .get("viewportHeight")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(800.0);

            let (mut left, mut top) = position();
            if left + width + VIEWPORT_MARGIN > viewport_width {
                left = (viewport_width - width - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN);
            }
            if top + height + VIEWPORT_MARGIN > viewport_height {
                top = (viewport_height - height - VIEWPORT_MARGIN).max(VIEWPORT_MARGIN);
            }
            position.set((left, top));
        });
    });

    // Focus the first entry and handle the keyboard shortcuts.
    use_effect(move || {
        let mut eval = document::eval(
            r#"
            const menu = document.getElementById("plasma-context-menu");
            if (!menu) { return; }
            const items = Array.from(menu.querySelectorAll("button"));
            if (items.length > 0) { items[0].focus(); }
            menu.addEventListener("keydown", (event) => {
                const index = items.indexOf(document.activeElement);
                if (event.key === "ArrowDown") {
                    event.preventDefault();
                    items[Math.min(index + 1, items.length - 1)]?.focus();
                } else if (event.key === "ArrowUp") {
                    event.preventDefault();
                    items[Math.max(index - 1, 0)]?.focus();
                } else if (event.key === "Escape") {
                    event.preventDefault();
                    dioxus.send("escape");
                }
            });
            "#,
        );
        spawn(async move {
            while let Ok(command) = eval.recv::<String>().await {
                if command == "escape" {
                    on_close.call(());
                    break;
                }
            }
        });
    });

    let (left, top) = position();
    let prepared: Vec<(ContextMenuEntry, String)> = entries
        .iter()
        .map(|entry| {
            let mut class = String::from("context-menu-item");
            if entry.danger {
                class.push_str(" danger");
            }
            if entry.disabled {
                class.push_str(" disabled");
            }
            (entry.clone(), class)
        })
        .collect();

    rsx! {
        div {
            class: "context-menu-layer",
            onmousedown: move |_| on_close.call(()),
            oncontextmenu: move |event| event.prevent_default(),
            div {
                id: MENU_ID,
                class: "context-menu",
                style: "left: {left}px; top: {top}px;",
                onmousedown: move |event| event.stop_propagation(),
                oncontextmenu: move |event| event.prevent_default(),
                for (entry, class) in prepared {
                    if entry.separator_before {
                        div { class: "context-menu-separator" }
                    }
                    button {
                        r#type: "button",
                        class: "{class}",
                        disabled: entry.disabled,
                        onclick: {
                            let id = entry.id.clone();
                            move |_| {
                                on_select.call(id.clone());
                                on_close.call(());
                            }
                        },
                        span { class: "context-menu-icon",
                            if let Some(icon) = entry.icon {
                                Icon { name: icon, size: 16, class: "icon" }
                            }
                        }
                        span { class: "context-menu-label", "{entry.label}" }
                        if let Some(shortcut) = entry.shortcut {
                            span { class: "context-menu-shortcut", "{shortcut}" }
                        }
                    }
                }
            }
        }
    }
}
