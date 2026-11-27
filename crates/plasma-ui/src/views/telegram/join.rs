//! Join bar and confirmation modal for Telegram links.

use crate::state::{push_toast, set_if_changed, AppServices, ToastKind, UiState};
use crate::theme::{avatar_gradient, avatar_initials};
use dioxus::prelude::*;
use plasma_telegram::TdlibChatInfo;

/// A chat the viewer can see, or an invite that still needs a decision.
#[derive(Clone, PartialEq)]
pub(super) struct JoinOffer {
    pub chat_id: Option<i64>,
    pub title: String,
    pub description: String,
    pub member_count: Option<i32>,
    pub is_channel: bool,
    pub is_group: bool,
    pub join_by_request: bool,
    pub invite_link: Option<String>,
    pub photo_data_url: Option<String>,
    pub show_modal: bool,
    pub pending: bool,
}

pub(super) fn confirm_join(
    services: AppServices,
    ui: UiState,
    mut join_offer: Signal<Option<JoinOffer>>,
) {
    let Some(offer) = join_offer() else {
        return;
    };
    if offer.pending {
        return;
    }
    spawn(async move {
        let result = if let Some(link) = offer.invite_link.clone() {
            services.telegram.join_invite(link).await
        } else if let Some(chat_id) = offer.chat_id {
            services.telegram.join_chat(chat_id).await
        } else {
            push_toast(
                ui.shell.toasts,
                "Não foi possível entrar neste chat.",
                ToastKind::Error,
            );
            return;
        };
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                push_toast(ui.shell.toasts, error.to_string(), ToastKind::Error);
                return;
            }
        };
        if !result.success {
            push_toast(
                ui.shell.toasts,
                result
                    .error
                    .unwrap_or_else(|| "Não foi possível entrar neste chat.".to_string()),
                ToastKind::Error,
            );
            return;
        }
        if let Some(chat) = result.chat.clone() {
            remember_chat(ui, chat);
        }
        if result.requested {
            let mut next = offer.clone();
            next.pending = true;
            next.show_modal = false;
            join_offer.set(Some(next));
            let name = offer.title.trim();
            let name = if name.is_empty() { "o chat" } else { name };
            push_toast(
                ui.shell.toasts,
                format!("Pedido para entrar em {name} enviado."),
                ToastKind::Info,
            );
            return;
        }
        join_offer.set(None);
        if offer.chat_id.is_none() {
            if let Some(chat) = result.chat {
                crate::views::chat_list::open_chat(services, ui, chat);
            }
        }
    });
}

fn remember_chat(ui: UiState, chat: TdlibChatInfo) {
    let mut chats = ui.conversation.chats();
    if let Some(existing) = chats.iter_mut().find(|item| item.id == chat.id) {
        *existing = chat.clone();
    } else {
        chats.insert(0, chat.clone());
    }
    set_if_changed(ui.conversation.chats, chats);
    if ui
        .conversation
        .selected_chat()
        .is_some_and(|selected| selected.id == chat.id)
    {
        set_if_changed(ui.conversation.selected_chat, Some(chat));
    }
}

#[component]
pub(super) fn JoinDialog(
    offer: JoinOffer,
    on_cancel: EventHandler<()>,
    on_confirm: EventHandler<()>,
) -> Element {
    let services = use_context::<AppServices>();
    let chat_id = offer.chat_id;
    let lookup = services.clone();
    let mut avatar_url =
        use_signal(move || chat_id.and_then(|chat_id| lookup.telegram.chat_avatar_url(chat_id)));
    use_effect(move || {
        let Some(chat_id) = chat_id else {
            return;
        };
        if avatar_url.peek().is_some() {
            return;
        }
        let services = services.clone();
        spawn(async move {
            let _ = services.telegram.ensure_chat_avatar(chat_id).await;
            if let Some(url) = services.telegram.chat_avatar_url(chat_id) {
                avatar_url.set(Some(url));
            }
        });
    });
    let photo = avatar_url().or(offer.photo_data_url.clone());
    let gradient = avatar_gradient(&offer.chat_id.unwrap_or_default().to_string());
    let initials = avatar_initials(&offer.title);
    let kind = if offer.is_channel {
        "Canal"
    } else if offer.is_group {
        "Grupo"
    } else {
        "Conversa"
    };
    let count = offer.member_count.map(|count| {
        let label = if offer.is_channel {
            "inscritos"
        } else {
            "membros"
        };
        format!("{} {label}", format_count(count))
    });
    let action = if offer.pending {
        "Pedido enviado"
    } else if offer.join_by_request {
        "Solicitar"
    } else {
        "Entrar"
    };

    rsx! {
        div {
            class: "forward-backdrop",
            onclick: move |_| on_cancel.call(()),
            section {
                class: "modal-card join-card",
                role: "dialog",
                aria_modal: "true",
                aria_label: "Entrar no chat",
                onclick: move |event| event.stop_propagation(),
                div { class: "join-hero",
                    if let Some(photo) = photo {
                        img { class: "join-photo", src: "{photo}", alt: "" }
                    } else {
                        div {
                            class: "join-photo join-photo-fallback",
                            style: "background: {gradient};",
                            "{initials}"
                        }
                    }
                    strong { "{offer.title}" }
                    p { class: "hint",
                        "{kind}"
                        if let Some(count) = count {
                            " · {count}"
                        }
                    }
                    if !offer.description.trim().is_empty() {
                        p { class: "join-description", "{offer.description}" }
                    }
                }
                div { class: "join-actions",
                    button {
                        class: "btn",
                        disabled: offer.pending,
                        onclick: move |_| on_cancel.call(()),
                        "Cancelar"
                    }
                    button {
                        class: "btn primary",
                        disabled: offer.pending,
                        onclick: move |_| on_confirm.call(()),
                        "{action}"
                    }
                }
            }
        }
    }
}

pub(super) fn join_bar_label(offer: &JoinOffer) -> String {
    if offer.pending {
        return "Pedido enviado. Aguardando um administrador.".to_string();
    }
    let kind = if offer.is_channel {
        "canal"
    } else if offer.is_group {
        "grupo"
    } else {
        "conversa"
    };
    if offer.join_by_request {
        format!("Você pode ver este {kind}. A entrada precisa de aprovação.")
    } else {
        format!("Você pode ver este {kind} sem participar.")
    }
}

fn format_count(count: i32) -> String {
    let digits = count.abs().to_string();
    let mut grouped = String::new();
    for (index, ch) in digits.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    grouped.chars().rev().collect()
}
