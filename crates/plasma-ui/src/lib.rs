//! Plasma Hub Rust UI shell (Dioxus 0.7 WebView renderer).
//!
//! The shell owns presentation and local UI state only. Data, downloads and
//! Telegram live in the UI-agnostic crates and reach this layer through
//! [`state::AppServices`] and [`plasma_core::EventBus`] in later phases.

pub mod state;
pub mod theme;
pub mod views;

use dioxus::prelude::*;
use plasma_store::SettingKey;
use state::{AppServices, Tab, UiState};
use theme::{Density, Palette};
use views::{
    downloads::DownloadsView, login::LoginView, palette::CommandPalette, settings::SettingsView,
    telegram::TelegramView, toasts::ToastStack, twitter::TwitterView,
};

const SHELL_CSS: &str = include_str!("../assets/shell.css");

/// Root component. State is created once and shared through context.
#[component]
pub fn App() -> Element {
    let services = use_context::<AppServices>();
    let settings = services.settings.clone();

    let tab = use_signal(|| Tab::Telegram);
    let palette = use_signal(|| {
        Palette::from_stored(settings.get(SettingKey::APPEARANCE_PALETTE).ok().flatten())
    });
    let density = use_signal(|| {
        Density::from_stored(settings.get(SettingKey::APPEARANCE_DENSITY).ok().flatten())
    });
    let logged_in = use_signal(|| settings.get_bool(SettingKey::SKIP_LOGIN).unwrap_or(false));
    let show_spike = use_signal(|| false);
    let show_palette = use_signal(|| false);
    let toasts = use_signal(Vec::new);

    let ui = use_context_provider(|| UiState {
        tab,
        palette,
        density,
        logged_in,
        show_spike,
        show_palette,
        toasts,
    });

    // Global shortcuts live in the webview, so they work regardless of which
    // element currently has focus. The palette itself handles Enter/Escape
    // while its input is focused; this listener adds Ctrl/Cmd+K from anywhere.
    use_effect(move || {
        let mut ui = ui;
        spawn(async move {
            let mut eval = document::eval(
                r#"
                document.addEventListener("keydown", (event) => {
                    const key = event.key.toLowerCase();
                    if ((event.ctrlKey || event.metaKey) && key === "k") {
                        event.preventDefault();
                        dioxus.send("toggle");
                    } else if (event.key === "Escape") {
                        dioxus.send("close");
                    }
                });
                "#,
            );

            while let Ok(command) = eval.recv::<String>().await {
                match command.as_str() {
                    "toggle" => {
                        let next = !ui.show_palette();
                        ui.show_palette.set(next);
                    }
                    "close" => ui.show_palette.set(false),
                    _ => {}
                }
            }
        });
    });

    let palette = ui.palette();
    let density = ui.density();

    rsx! {
        style { dangerous_inner_html: SHELL_CSS }
        div {
            class: "plasma-root",
            "data-palette": palette.as_str(),
            "data-density": density.as_str(),
            if ui.logged_in() {
                Shell {}
            } else {
                LoginView {}
            }
            CommandPalette {}
            ToastStack {}
        }
    }
}

/// App chrome: rail, top bar and the active view.
#[component]
fn Shell() -> Element {
    let mut ui = use_context::<UiState>();
    let tab = ui.tab();

    rsx! {
        div { class: "shell",
            nav { class: "rail",
                div { class: "rail-brand", title: "Plasma Hub", "P" }
                for item in Tab::ALL {
                    button {
                        key: "{item.label()}",
                        class: if tab == item { "rail-item active" } else { "rail-item" },
                        title: "{item.label()}",
                        onclick: move |_| ui.tab.set(item),
                        "{item.rail_label()}"
                    }
                }
                div { class: "rail-spacer" }
                button {
                    class: "rail-item",
                    title: "Paleta de comandos",
                    onclick: move |_| ui.show_palette.set(true),
                    "⌘"
                }
            }
            div { class: "shell-main",
                header { class: "topbar",
                    div { class: "topbar-title",
                        strong { "Plasma Hub" }
                        span { "{tab.label()}" }
                    }
                    div { class: "topbar-actions",
                        button {
                            class: "btn small ghost",
                            title: "Ctrl/Cmd + K",
                            onclick: move |_| ui.show_palette.set(true),
                            "⌘K  Comandos"
                        }
                        button {
                            class: "btn small ghost",
                            onclick: move |_| ui.logged_in.set(false),
                            "Sair"
                        }
                    }
                }
                div { class: "content",
                    {match tab {
                        Tab::Telegram => rsx! { TelegramView {} },
                        Tab::Downloads => rsx! { DownloadsView {} },
                        Tab::Twitter => rsx! { TwitterView {} },
                        Tab::Settings => rsx! { SettingsView {} },
                    }}
                }
            }
        }
    }
}
