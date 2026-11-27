//! Transient notifications shared by every view.

use crate::{
    icons::{Icon, IconName},
    state::{AppServices, ToastAction, ToastKind, UiState},
};
use dioxus::prelude::*;
use std::collections::HashMap;
use std::time::Duration;

const TOAST_TTL_MS: u64 = 4_500;
const TOAST_TICK_MS: u64 = 200;

#[component]
pub fn ToastStack() -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let mut paused = use_signal(|| false);

    use_future(move || async move {
        let mut elapsed: HashMap<u64, u64> = HashMap::new();
        loop {
            tokio::time::sleep(Duration::from_millis(TOAST_TICK_MS)).await;
            if paused() {
                continue;
            }
            let current = ui.shell.toasts.read().clone();
            if current.is_empty() {
                elapsed.clear();
                continue;
            }
            elapsed.retain(|id, _| current.iter().any(|toast| toast.id == *id));
            let mut expired = Vec::new();
            for toast in &current {
                let entry = elapsed.entry(toast.id).or_insert(0);
                *entry = entry.saturating_add(TOAST_TICK_MS);
                if *entry >= TOAST_TTL_MS {
                    expired.push(toast.id);
                }
            }
            if !expired.is_empty() {
                let mut next = ui.shell.toasts.read().clone();
                next.retain(|toast| !expired.contains(&toast.id));
                ui.shell.toasts.set(next);
            }
        }
    });

    let items = ui.shell.toasts.read().clone();
    let total = items.len();

    rsx! {
        div {
            class: if total > 1 { "toast-stack has-multiple" } else { "toast-stack" },
            role: "region",
            aria_label: "Notificações",
            onmouseenter: move |_| paused.set(true),
            onmouseleave: move |_| paused.set(false),
            onfocusin: move |_| paused.set(true),
            onfocusout: move |_| paused.set(false),
            for (index, toast) in items.into_iter().rev().enumerate() {
                {
                    let id = toast.id;
                    let kind_class = toast.kind.class();
                    let is_error = toast.kind == ToastKind::Error;
                    let (title, subtitle) = split_toast_copy(&toast.message);
                    rsx! {
                        div {
                            key: "{id}",
                            class: "toast {kind_class}",
                            style: "--toast-index: {index}; --toast-ttl: {TOAST_TTL_MS}ms;",
                            role: if is_error { "alert" } else { "status" },
                            aria_live: if is_error { "assertive" } else { "polite" },
                            span { class: "toast-icon", aria_hidden: "true",
                                match toast.kind {
                                    ToastKind::Success => rsx! { Icon { name: IconName::Check, size: 15, class: "icon" } },
                                    ToastKind::Error => rsx! { span { class: "toast-glyph", "!" } },
                                    ToastKind::Info => rsx! { Icon { name: IconName::Info, size: 15, class: "icon" } },
                                }
                            }
                            div { class: "toast-copy",
                                strong { class: "toast-title", "{title}" }
                                if let Some(sub) = subtitle {
                                    span { class: "toast-subtitle", "{sub}" }
                                }
                            }
                            if let (Some(label), Some(action)) = (toast.action_label.clone(), toast.action.clone()) {
                                {
                                    let action_services = services.clone();
                                    rsx! {
                                        button {
                                            class: "toast-action-btn",
                                            r#type: "button",
                                            onclick: move |_| {
                                                match action.clone() {
                                                    ToastAction::OpenTab(tab) => ui.shell.tab.set(tab),
                                                    ToastAction::RetryDownload(ref dl_id) => {
                                                        crate::views::downloads::retry_download_by_id(
                                                            action_services.clone(),
                                                            ui,
                                                            dl_id,
                                                        );
                                                    }
                                                }
                                                let mut next = ui.shell.toasts.read().clone();
                                                next.retain(|item| item.id != id);
                                                ui.shell.toasts.set(next);
                                            },
                                            "{label}"
                                        }
                                    }
                                }
                            }
                            button {
                                class: "toast-close",
                                r#type: "button",
                                title: "Fechar",
                                aria_label: "Fechar notificação",
                                onclick: move |_| {
                                    let mut next = ui.shell.toasts.read().clone();
                                    next.retain(|item| item.id != id);
                                    ui.shell.toasts.set(next);
                                },
                                Icon { name: IconName::Close, size: 14, class: "icon" }
                            }
                            span { class: "toast-timer", aria_hidden: "true" }
                        }
                    }
                }
            }
        }
    }
}

fn split_toast_copy(message: &str) -> (&str, Option<&str>) {
    let trimmed = message.trim();
    if let Some((head, tail)) = trimmed.split_once(". ") {
        let tail = tail.trim();
        if !head.is_empty() && !tail.is_empty() {
            return (head, Some(tail));
        }
    }
    (trimmed, None)
}
