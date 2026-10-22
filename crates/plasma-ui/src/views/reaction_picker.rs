//! Small emoji picker for message reactions.
//!
//! Rendered at the shell root (same pattern as the context menu) so the
//! timeline scroll container cannot clip it.

use crate::state::{react_to_message, AppServices, ReactionPickerState, UiState};
use dioxus::prelude::*;

const PICKER_ID: &str = "plasma-reaction-picker";
const VIEWPORT_MARGIN: f64 = 10.0;
const EMOJIS: [&str; 8] = ["👍", "❤️", "🔥", "😂", "😮", "😢", "👏", "🎉"];

#[component]
pub fn ReactionPicker(target: ReactionPickerState, on_close: EventHandler<()>) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let mut position = use_signal(|| (target.x, target.y));

    // Keep the picker inside the viewport.
    use_effect(move || {
        spawn(async move {
            let script = r#"
            const element = document.getElementById("plasma-reaction-picker");
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
                .unwrap_or(320.0);
            let height = value
                .get("height")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(48.0);
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

    let (left, top) = position();
    rsx! {
        div {
            class: "context-menu-layer",
            onmousedown: move |_| on_close.call(()),
            div {
                id: PICKER_ID,
                class: "reaction-picker",
                style: "left: {left}px; top: {top}px;",
                onmousedown: move |event| event.stop_propagation(),
                for emoji in EMOJIS {
                    {
                        let services = services.clone();
                        rsx! {
                            button {
                                r#type: "button",
                                class: "reaction-picker-emoji",
                                title: "Reagir com {emoji}",
                                onclick: move |_| {
                                    react_to_message(
                                        &services,
                                        ui,
                                        target.chat_id,
                                        target.message_id,
                                        emoji.to_string(),
                                    );
                                    on_close.call(());
                                },
                                "{emoji}"
                            }
                        }
                    }
                }
            }
        }
    }
}
