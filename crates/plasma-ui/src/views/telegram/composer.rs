//! Composer: text, attachments and dropped media.

use crate::icons::{Icon, IconName};
use crate::state::{
    merge_messages, message_cache_key, push_toast, set_if_changed, AppServices, ToastKind, UiState,
};
use dioxus::html::HasFileData;
use dioxus::prelude::*;
use plasma_telegram::TdlibMessageInfo;
use std::path::Path;

use super::conversation::parse_chat_id;
use super::{
    DropMediaState, DroppedMediaSelection, PendingDroppedMedia, MAX_DROPPED_MEDIA_BYTES,
    MAX_DROPPED_MEDIA_FILES,
};

pub(super) fn send_text(
    services: AppServices,
    mut ui: UiState,
    mut composer: Signal<String>,
    mut sending: Signal<bool>,
    mut error: Signal<Option<String>>,
    mut reply: Signal<Option<TdlibMessageInfo>>,
) {
    if sending() {
        return;
    }
    let Some(chat) = ui.conversation.selected_chat() else {
        error.set(Some("Escolha uma conversa primeiro.".to_string()));
        return;
    };
    let Some(chat_id) = parse_chat_id(&chat) else {
        error.set(Some("Chat inválido.".to_string()));
        return;
    };
    let text = composer().trim().to_string();
    if text.is_empty() {
        error.set(Some("Digite uma mensagem.".to_string()));
        return;
    }
    let topic = ui.conversation.selected_topic();
    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());
    let reply_id = reply().map(|message| message.id);
    let expected_chat = chat.id.clone();
    let expected_topic = topic.as_ref().map(|item| item.id);
    let cache_key = message_cache_key(&chat.id, topic.as_ref());

    sending.set(true);
    error.set(None);
    spawn(async move {
        match services
            .telegram
            .send_message(chat_id, text, reply_id, topic_id, topic_kind)
            .await
        {
            Ok(result) if result.success => {
                let still_here = ui.conversation.selected_chat().map(|chat| chat.id)
                    == Some(expected_chat.clone())
                    && ui.conversation.selected_topic().map(|topic| topic.id) == expected_topic;
                if still_here {
                    composer.set(String::new());
                    reply.set(None);
                }
                if let Some(message) = result.message {
                    if still_here {
                        let mut messages = ui.conversation.messages();
                        merge_messages(&mut messages, [message.clone()]);
                        ui.conversation.messages.set(messages.clone());
                        let cache_services = services.clone();
                        spawn(async move {
                            let _ = cache_services
                                .telegram
                                .cache_messages(cache_key, messages)
                                .await;
                        });
                    }
                    update_chat_preview(ui, &expected_chat, &message);
                }
            }
            Ok(result) => error.set(
                result
                    .error
                    .or_else(|| Some("Não foi possível enviar.".to_string())),
            ),
            Err(send_error) => error.set(Some(send_error.to_string())),
        }
        sending.set(false);
    });
}

pub(super) fn send_media_file(
    services: AppServices,
    mut ui: UiState,
    file_path: String,
    mut composer: Signal<String>,
    mut sending: Signal<bool>,
    mut error: Signal<Option<String>>,
    mut reply: Signal<Option<TdlibMessageInfo>>,
) {
    if sending() {
        return;
    }
    let Some(chat) = ui.conversation.selected_chat() else {
        error.set(Some("Escolha uma conversa primeiro.".to_string()));
        return;
    };
    let Some(chat_id) = parse_chat_id(&chat) else {
        error.set(Some("Chat inválido.".to_string()));
        return;
    };
    let caption = composer().trim().to_string();
    let caption = if caption.is_empty() {
        None
    } else {
        Some(caption)
    };
    let topic = ui.conversation.selected_topic();
    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());
    let reply_id = reply().map(|message| message.id);
    let expected_chat = chat.id.clone();
    let expected_topic = topic.as_ref().map(|item| item.id);
    let cache_key = message_cache_key(&chat.id, topic.as_ref());

    sending.set(true);
    error.set(None);
    spawn(async move {
        match services
            .telegram
            .send_media(chat_id, file_path, caption, reply_id, topic_id, topic_kind)
            .await
        {
            Ok(result) if result.success => {
                let still_here = ui.conversation.selected_chat().map(|chat| chat.id)
                    == Some(expected_chat.clone())
                    && ui.conversation.selected_topic().map(|topic| topic.id) == expected_topic;
                if still_here {
                    composer.set(String::new());
                    reply.set(None);
                }
                if let Some(message) = result.message {
                    if still_here {
                        let mut messages = ui.conversation.messages();
                        merge_messages(&mut messages, [message.clone()]);
                        ui.conversation.messages.set(messages.clone());
                        let cache_services = services.clone();
                        spawn(async move {
                            let _ = cache_services
                                .telegram
                                .cache_messages(cache_key, messages)
                                .await;
                        });
                    }
                    update_chat_preview(ui, &expected_chat, &message);
                }
            }
            Ok(result) => error.set(
                result
                    .error
                    .or_else(|| Some("Não foi possível enviar a mídia.".to_string())),
            ),
            Err(send_error) => error.set(Some(send_error.to_string())),
        }
        sending.set(false);
    });
}

pub(super) fn close_drop_media(mut drop: DropMediaState) {
    drop.pending.set(Vec::new());
    drop.caption.set(String::new());
    drop.target.set(None);
    drop.progress.set(None);
    drop.error.set(None);
    drop.open.set(false);
}

pub(super) fn collect_dropped_media(data: &DragData) -> DroppedMediaSelection {
    let mut files = Vec::new();
    let mut skipped = 0;
    for file in data.files() {
        if files.len() >= MAX_DROPPED_MEDIA_FILES {
            skipped += 1;
            continue;
        }
        let path = file.path();
        if path.as_os_str().is_empty() || !path.is_file() {
            skipped += 1;
            continue;
        }
        if file.size() > MAX_DROPPED_MEDIA_BYTES {
            skipped += 1;
            continue;
        }
        let name = file.name();
        let name = if name.trim().is_empty() {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Mídia")
                .to_string()
        } else {
            name
        };
        let Some(is_video) = dropped_media_kind(&name, file.content_type().as_deref()) else {
            skipped += 1;
            continue;
        };
        files.push(PendingDroppedMedia {
            path: path.to_string_lossy().to_string(),
            name,
            is_video,
            preview: None,
        });
    }
    DroppedMediaSelection { files, skipped }
}

pub(super) fn dropped_media_kind(name: &str, mime: Option<&str>) -> Option<bool> {
    let mime = mime.unwrap_or_default().to_ascii_lowercase();
    let extension = Path::new(name)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if mime.starts_with("video/")
        || matches!(
            extension.as_str(),
            "mp4" | "m4v" | "webm" | "mov" | "mkv" | "avi" | "3gp"
        )
    {
        Some(true)
    } else if mime.starts_with("image/")
        || matches!(
            extension.as_str(),
            "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" | "avif"
        )
    {
        Some(false)
    } else {
        None
    }
}

pub(super) fn open_dropped_media_preview(
    services: AppServices,
    mut drop: DropMediaState,
    target_chat: String,
    files: Vec<PendingDroppedMedia>,
) {
    if files.is_empty() {
        return;
    }
    drop.pending.set(files.clone());
    drop.caption.set(String::new());
    drop.target.set(Some(target_chat));
    drop.progress.set(None);
    drop.error.set(None);
    drop.cancel.set(false);
    drop.open.set(true);
    spawn(async move {
        for file in files {
            let preview = services
                .telegram
                .media_preview_data_url(file.path.clone(), file.is_video)
                .await
                .ok()
                .flatten();
            if let Some(preview) = preview {
                drop.pending.with_mut(|items| {
                    if let Some(item) = items.iter_mut().find(|item| item.path == file.path) {
                        item.preview = Some(preview);
                    }
                });
            }
        }
    });
}

pub(super) fn send_dropped_media(
    services: AppServices,
    mut ui: UiState,
    mut drop: DropMediaState,
    mut sending: Signal<bool>,
    mut reply: Signal<Option<TdlibMessageInfo>>,
) {
    if sending() || drop.pending().is_empty() {
        return;
    }
    let Some(chat) = ui.conversation.selected_chat() else {
        drop.error
            .set(Some("Escolha uma conversa primeiro.".to_string()));
        return;
    };
    let Some(chat_id) = parse_chat_id(&chat) else {
        drop.error.set(Some("Chat inválido.".to_string()));
        return;
    };
    if drop.target().as_deref() != Some(chat.id.as_str()) {
        drop.error.set(Some(
            "A conversa mudou. Selecione as mídias novamente.".to_string(),
        ));
        return;
    }

    let files = drop.pending();
    let caption = drop.caption().trim().to_string();
    let caption = (!caption.is_empty()).then_some(caption);
    let topic = ui.conversation.selected_topic();
    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());
    let reply_id = reply().map(|message| message.id);
    let expected_chat = chat.id.clone();
    let expected_topic = topic.as_ref().map(|item| item.id);
    let cache_key = message_cache_key(&chat.id, topic.as_ref());
    let total = files.len();

    sending.set(true);
    drop.cancel.set(false);
    drop.error.set(None);
    drop.progress.set(Some(0.0));
    spawn(async move {
        let mut first_reply = reply_id;
        for (index, file) in files.iter().enumerate() {
            if drop.cancel() {
                drop.pending.set(files[index..].to_vec());
                drop.progress.set(None);
                drop.error.set(Some("Envio cancelado.".to_string()));
                sending.set(false);
                return;
            }
            let file_caption = if index == 0 { caption.clone() } else { None };
            let file_reply = if index == 0 { first_reply } else { None };
            match services
                .telegram
                .send_media(
                    chat_id,
                    file.path.clone(),
                    file_caption,
                    file_reply,
                    topic_id,
                    topic_kind.clone(),
                )
                .await
            {
                Ok(result) if result.success => {
                    if let Some(message) = result.message {
                        apply_sent_message(
                            &services,
                            &mut ui,
                            &expected_chat,
                            expected_topic,
                            &cache_key,
                            message,
                        );
                    }
                    if index == 0 {
                        first_reply = None;
                        drop.caption.set(String::new());
                        reply.set(None);
                    }
                    let done = index + 1;
                    drop.progress
                        .set(Some((done as f32 / total as f32) * 100.0));
                    if drop.cancel() && done < total {
                        drop.pending.set(files[done..].to_vec());
                        drop.progress.set(None);
                        drop.error.set(Some("Envio cancelado.".to_string()));
                        sending.set(false);
                        return;
                    }
                }
                Ok(result) => {
                    drop.error.set(
                        result
                            .error
                            .or_else(|| Some(format!("Não foi possível enviar {}.", file.name))),
                    );
                    drop.pending.set(files[index..].to_vec());
                    drop.progress.set(None);
                    sending.set(false);
                    return;
                }
                Err(send_error) => {
                    drop.error.set(Some(send_error.to_string()));
                    drop.pending.set(files[index..].to_vec());
                    drop.progress.set(None);
                    sending.set(false);
                    return;
                }
            }
        }

        drop.pending.set(Vec::new());
        drop.caption.set(String::new());
        drop.target.set(None);
        drop.open.set(false);
        drop.progress.set(None);
        reply.set(None);
        push_toast(
            ui.shell.toasts,
            if total == 1 {
                "Mídia enviada.".to_string()
            } else {
                format!("{total} mídias enviadas.")
            },
            ToastKind::Success,
        );
        sending.set(false);
    });
}

pub(super) fn apply_sent_message(
    services: &AppServices,
    ui: &mut UiState,
    expected_chat: &str,
    expected_topic: Option<i64>,
    cache_key: &str,
    message: TdlibMessageInfo,
) {
    if ui
        .conversation
        .selected_chat()
        .as_ref()
        .map(|chat| chat.id.as_str())
        == Some(expected_chat)
        && ui.conversation.selected_topic().map(|topic| topic.id) == expected_topic
    {
        let mut messages = ui.conversation.messages();
        merge_messages(&mut messages, [message.clone()]);
        ui.conversation.messages.set(messages.clone());
        let cache_services = services.clone();
        let cache_key = cache_key.to_string();
        spawn(async move {
            let _ = cache_services
                .telegram
                .cache_messages(cache_key, messages)
                .await;
        });
    }
    update_chat_preview(*ui, expected_chat, &message);
}

pub(super) fn update_chat_preview(ui: UiState, chat_id: &str, message: &TdlibMessageInfo) {
    let mut chats = ui.conversation.chats();
    if let Some(chat) = chats.iter_mut().find(|chat| chat.id == chat_id) {
        chat.last_message_text = message.text.clone();
        chat.last_message_date = message.date;
        chat.last_message_has_media = message.has_media;
        chat.last_message_is_video = message.is_video;
        chat.last_message_is_photo = message.is_photo;
    }
    set_if_changed(ui.conversation.chats, chats);
    if let Some(selected) = ui.conversation.selected_chat() {
        if selected.id == chat_id {
            let mut selected = selected;
            selected.last_message_text = message.text.clone();
            selected.last_message_date = message.date;
            selected.last_message_has_media = message.has_media;
            selected.last_message_is_video = message.is_video;
            selected.last_message_is_photo = message.is_photo;
            set_if_changed(ui.conversation.selected_chat, Some(selected));
        }
    }
}

pub(super) async fn pick_download_folder() -> Option<std::path::PathBuf> {
    #[cfg(not(target_os = "android"))]
    {
        rfd::FileDialog::new()
            .set_title("Escolher pasta para o download em massa")
            .pick_folder()
    }

    #[cfg(target_os = "android")]
    {
        // No folder picker yet; confirm the app Downloads directory so mass
        // download and "Procurar" stay usable on Android.
        plasma_platform::android_app_dirs()
            .ok()
            .map(|dirs| dirs.download_dir)
    }
}

pub(super) async fn pick_media_file() -> Option<std::path::PathBuf> {
    #[cfg(not(target_os = "android"))]
    {
        rfd::FileDialog::new()
            .set_title("Escolher mídia para enviar")
            .add_filter(
                "Mídia",
                &[
                    "jpg", "jpeg", "png", "webp", "gif", "mp4", "mov", "webm", "mkv", "mp3", "m4a",
                    "ogg", "wav", "pdf", "zip",
                ],
            )
            .pick_file()
    }

    #[cfg(target_os = "android")]
    {
        plasma_platform::pick_media_file().await
    }
}

#[component]
pub(super) fn DropMediaDialog(drop: super::DropMediaState, sending: Signal<bool>) -> Element {
    let ui = use_context::<crate::state::UiState>();
    let services = use_context::<crate::state::AppServices>();
    let dropped_files = drop.pending();
    let dropped_count = dropped_files.len();
    let drop_send_services = services.clone();
    let drop_retry_services = services.clone();

    rsx! {
            if drop.open() {
                div {
                    class: "drop-media-backdrop",
                    onclick: move |_| {
                        if sending() {
                            return;
                        }
                        close_drop_media(drop);
                    },
                    section {
                        id: "drop-media-modal",
                        class: "drop-media-modal",
                        tabindex: "-1",
                        onkeydown: move |event| {
                            if event.key() == Key::Escape {
                                event.prevent_default();
                                if sending() {
                                    // TDLib cannot abort an upload already in
                                    // flight, so stop after the current file.
                                    drop.cancel.set(true);
                                } else {
                                    close_drop_media(drop);
                                }
                            }
                        },
                        onclick: move |event| event.stop_propagation(),
                        header { class: "drop-media-header",
                            div {
                                strong { "Enviar mídias" }
                                span {
                                    class: "hint",
                                    if dropped_count == 1 {
                                        "1 arquivo selecionado"
                                    } else {
                                        "{dropped_count} arquivos selecionados"
                                    }
                                }
                            }
                            button {
                                class: "btn small ghost",
                                disabled: sending(),
                                title: "Fechar",
                                aria_label: "Fechar",
                                onclick: move |_| {
                                    if sending() {
                                        return;
                                    }
                                    close_drop_media(drop);
                                },
                                Icon { name: IconName::Close, size: 14, class: "icon" }
                            }
                        }
                        div { class: "drop-media-grid",
                            for (index, file) in dropped_files.iter().enumerate() {
                                div { key: "{file.path}", class: "drop-media-item",
                                    div { class: "drop-media-thumb",
                                        if let Some(preview) = file.preview.as_ref() {
                                            img { src: "{preview}", alt: "{file.name}" }
                                        } else {
                                            Icon {
                                                name: if file.is_video {
                                                    IconName::Play
                                                } else {
                                                    IconName::Media
                                                },
                                                size: 28,
                                                class: "icon drop-media-placeholder-icon",
                                            }
                                        }
                                        if file.is_video {
                                            span { class: "drop-media-video-badge",
                                                Icon { name: IconName::Play, size: 12, class: "icon" }
                                            }
                                        }
                                    }
                                    span {
                                        class: "drop-media-name",
                                        title: "{file.name}",
                                        "{file.name}"
                                    }
                                    button {
                                        class: "drop-media-remove",
                                        disabled: sending(),
                                        title: "Remover",
                                        aria_label: "Remover {file.name}",
                                        onclick: move |_| {
                                            drop.pending.with_mut(|items| {
                                                if index < items.len() {
                                                    items.remove(index);
                                                }
                                            });
                                            if drop.pending().is_empty() {
                                                close_drop_media(drop);
                                            }
                                        },
                                        Icon { name: IconName::Close, size: 12, class: "icon" }
                                    }
                                }
                            }
                        }
                        textarea {
                            class: "drop-media-caption",
                            rows: "3",
                            placeholder: "Adicionar uma legenda...",
                            value: "{drop.caption}",
                            disabled: sending(),
                            oninput: move |event| drop.caption.set(event.value()),
                        }
                        if let Some(error) = drop.error() {
                            p { class: "error-banner", "{error}" }
                        }
                        if let Some(progress) = drop.progress() {
                            div { class: "drop-media-progress",
                                div {
                                    class: "drop-media-progress-bar",
                                    style: "width: {progress}%;",
                                }
                            }
                        }
                        footer { class: "drop-media-actions",
                            button {
                                class: "btn ghost",
                                onclick: move |_| {
                                    if sending() {
                                        drop.cancel.set(true);
                                    } else {
                                        close_drop_media(drop);
                                    }
                                },
                                if sending() {
                                    if drop.cancel() {
                                        "Cancelando..."
                                    } else {
                                        "Cancelar envio"
                                    }
                                } else {
                                    "Cancelar"
                                }
                            }
                            if !sending() && drop.error().is_some() && !dropped_files.is_empty() {
                                button {
                                    class: "btn ghost",
                                    onclick: move |_| {
                                        send_dropped_media(
                                            drop_retry_services.clone(),
                                            ui,
                                            drop,
                                            sending,
                                            ui.panels.reply_to,
                                        );
                                    },
                                    "Tentar novamente"
                                }
                            }
                            button {
                                class: "btn primary",
                                disabled: sending() || dropped_files.is_empty(),
                                onclick: move |_| {
                                    send_dropped_media(
                                        drop_send_services.clone(),
                                        ui,
                                        drop,
                                        sending,
                                        ui.panels.reply_to,
                                    );
                                },
                                if sending() {
                                    "Enviando..."
                                } else if dropped_count == 1 {
                                    "Enviar mídia"
                                } else {
                                    "Enviar {dropped_count} mídias"
                                }
                            }
                        }
                    }
                }
            }
    }
}

/// Signals the composer reads and writes while a conversation stays open.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct ComposerInput {
    pub(super) text: Signal<String>,
    pub(super) sending: Signal<bool>,
    pub(super) error: Signal<Option<String>>,
}

/// Text field, reply banner and send/attach actions for the open conversation.
#[component]
pub(super) fn MessageComposer(
    input: ComposerInput,
    disabled: bool,
    media_disabled: bool,
    topic_closed: bool,
) -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let ComposerInput {
        mut text,
        sending,
        error,
    } = input;
    let can_send = !text().trim().is_empty();

    rsx! {
        div { class: "composer",
            if let Some(reply) = ui.panels.reply_to() {
                if !disabled {
                    div { class: "reply-banner",
                        div {
                            span { class: "hint", "Respondendo a" }
                            strong { " {reply.sender_name}" }
                            p { "{reply.text}" }
                        }
                        button {
                            class: "btn small ghost",
                            onclick: move |_| ui.panels.reply_to.set(None),
                            "Cancelar"
                        }
                    }
                }
            }
            if let Some(message) = error() {
                p { class: "error-banner composer-error", "{message}" }
            }
            if disabled {
                div { class: "composer-readonly",
                    span { class: "composer-readonly-icon",
                        Icon { name: IconName::Info, size: 14, class: "icon" }
                    }
                    span {
                        if topic_closed {
                            "Este tópico está fechado."
                        } else {
                            "Você não pode enviar mensagens nesta conversa."
                        }
                    }
                }
            } else {
                if media_disabled {
                    p { class: "composer-warning", "Envio de mídia indisponível nesta conversa." }
                }
                div { class: "composer-row",
                    textarea {
                        class: "composer-input",
                        rows: "1",
                        placeholder: "Escreva uma mensagem...",
                        value: "{text}",
                        oninput: move |event| text.set(event.value()),
                        onkeydown: {
                            let services = services.clone();
                            move |event| {
                                if event.key() == Key::Enter
                                    && !event.modifiers().shift()
                                    && !event.is_composing()
                                    && can_send
                                {
                                    event.prevent_default();
                                    send_text(
                                        services.clone(),
                                        ui,
                                        text,
                                        sending,
                                        error,
                                        ui.panels.reply_to,
                                    );
                                }
                            }
                        },
                    }
                    div { class: "composer-actions",
                        button {
                            class: "btn small ghost composer-icon-button",
                            disabled: media_disabled || sending(),
                            title: "Anexar mídia",
                            aria_label: "Anexar mídia",
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    let services = services.clone();
                                    spawn(async move {
                                        let Some(path) = pick_media_file().await else {
                                            return;
                                        };
                                        send_media_file(
                                            services,
                                            ui,
                                            path.to_string_lossy().to_string(),
                                            text,
                                            sending,
                                            error,
                                            ui.panels.reply_to,
                                        );
                                    });
                                }
                            },
                            Icon { name: IconName::Paperclip, size: 18, class: "icon" }
                        }
                        button {
                            class: "btn primary composer-icon-button composer-send-button",
                            disabled: sending() || !can_send,
                            title: if sending() {
                                "Enviando..."
                            } else if !can_send {
                                "Digite uma mensagem"
                            } else {
                                "Enviar mensagem"
                            },
                            aria_label: if sending() {
                                "Enviando..."
                            } else if !can_send {
                                "Digite uma mensagem"
                            } else {
                                "Enviar mensagem"
                            },
                            onclick: {
                                let services = services.clone();
                                move |_| send_text(
                                    services.clone(),
                                    ui,
                                    text,
                                    sending,
                                    error,
                                    ui.panels.reply_to,
                                )
                            },
                            Icon { name: IconName::Send, size: 18, class: "icon" }
                        }
                    }
                }
                p { class: "hint composer-hint", "Enter envia · Shift+Enter quebra linha" }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropped_media_detects_videos_and_images() {
        assert_eq!(dropped_media_kind("clip.MOV", None), Some(true));
        assert_eq!(
            dropped_media_kind("clip.mp4", Some("application/octet-stream")),
            Some(true)
        );
        assert_eq!(
            dropped_media_kind("photo.jpg", Some("application/octet-stream")),
            Some(false)
        );
        assert_eq!(
            dropped_media_kind("unknown", Some("image/png")),
            Some(false)
        );
        assert_eq!(dropped_media_kind("notes.txt", None), None);
    }
}
