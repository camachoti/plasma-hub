//! Right-click context menu: shared state, entries and actions.
//!
//! The menu is rendered once at the root of the shell so every view can open
//! it. Views only describe *what* was right-clicked through [`ContextTarget`];
//! the actions live here so the behaviour stays consistent between the
//! timeline, the chat list and the download history.

use crate::downloads::DownloadRecord;
use crate::icons::IconName;
use crate::state::{
    push_toast, toggle_topic_hidden, AppServices, LightboxState, ReactionPickerState, ToastKind,
    UiState,
};
use dioxus::prelude::*;
use plasma_telegram::{TdlibForumTopicInfo, TdlibMessageInfo};

/// A single row of the context menu.
#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuEntry {
    pub id: String,
    pub label: String,
    pub icon: Option<IconName>,
    pub shortcut: Option<String>,
    pub disabled: bool,
    pub danger: bool,
    pub separator_before: bool,
}

impl ContextMenuEntry {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            disabled: false,
            danger: false,
            separator_before: false,
        }
    }

    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    pub fn disabled(mut self) -> Self {
        self.disabled = true;
        self
    }

    pub fn separator(mut self) -> Self {
        self.separator_before = true;
        self
    }
}

/// What the user right-clicked.
#[derive(Clone, PartialEq)]
pub enum ContextTarget {
    Chat {
        id: String,
    },
    Message {
        chat_id: i64,
        message: Box<TdlibMessageInfo>,
    },
    /// A forum topic chip or row. `hidden` mirrors the current visibility so
    /// the menu label can offer hide/show.
    Topic {
        chat_id: i64,
        topic: Box<TdlibForumTopicInfo>,
        hidden: bool,
    },
    /// The "Todos" chip. It has no topic of its own, so the menu only opens
    /// the topic browser.
    Topics,
    /// An album (grouped media) of the timeline. `anchor_id` is the media the
    /// user right-clicked, used by the single-item actions.
    Album {
        chat_id: i64,
        messages: Vec<TdlibMessageInfo>,
        anchor_id: i64,
    },
    Download {
        record: Box<DownloadRecord>,
    },
}

impl ContextTarget {
    /// Builds the menu shown for this target.
    pub fn entries(&self) -> Vec<ContextMenuEntry> {
        match self {
            ContextTarget::Chat { id } => vec![
                ContextMenuEntry::new("chat.open", "Abrir conversa").icon(IconName::Chat),
                ContextMenuEntry::new("chat.read", "Marcar como lida").icon(IconName::Eye),
                ContextMenuEntry::new("chat.mass", "Download em massa")
                    .icon(IconName::Download)
                    .separator(),
                ContextMenuEntry::new("chat.copy_id", "Copiar ID da conversa")
                    .icon(IconName::Copy)
                    .shortcut(id.clone()),
            ],
            ContextTarget::Message { message, .. } => {
                let mut entries = vec![
                    ContextMenuEntry::new("message.reply", "Responder").icon(IconName::Reply),
                    ContextMenuEntry::new("message.react", "Reagir").icon(IconName::Smile),
                    ContextMenuEntry::new("message.forward", "Encaminhar").icon(IconName::Forward),
                    ContextMenuEntry::new("message.copy", "Copiar texto")
                        .icon(IconName::Copy)
                        .separator(),
                ];
                if message.has_media && !message.is_deleted {
                    entries.push(
                        ContextMenuEntry::new("message.media", "Abrir mídia").icon(IconName::Media),
                    );
                    entries.push(
                        ContextMenuEntry::new("message.save", "Salvar mídia").icon(IconName::Save),
                    );
                }
                if message.is_deleted {
                    entries.push(
                        ContextMenuEntry::new("message.copy", "Copiar texto")
                            .icon(IconName::Copy)
                            .disabled(),
                    );
                }
                entries
            }
            ContextTarget::Topic { hidden, .. } => vec![
                ContextMenuEntry::new(
                    "topic.toggle",
                    if *hidden {
                        "Mostrar tópico"
                    } else {
                        "Ocultar tópico"
                    },
                )
                .icon(if *hidden {
                    IconName::Eye
                } else {
                    IconName::EyeOff
                }),
                ContextMenuEntry::new("topic.browse", "Exibir listagem de tópicos")
                    .icon(IconName::List)
                    .separator(),
            ],
            ContextTarget::Topics => {
                vec![
                    ContextMenuEntry::new("topic.browse", "Exibir listagem de tópicos")
                        .icon(IconName::List),
                ]
            }
            ContextTarget::Album {
                messages,
                anchor_id,
                ..
            } => {
                let anchor = messages
                    .iter()
                    .find(|message| message.id == *anchor_id)
                    .unwrap_or(&messages[0]);
                let mut entries = vec![
                    ContextMenuEntry::new("album.open", "Abrir mídia").icon(IconName::Media),
                    ContextMenuEntry::new("album.save_one", "Salvar esta mídia")
                        .icon(IconName::Save),
                    ContextMenuEntry::new(
                        "album.save_all",
                        format!("Salvar álbum ({} mídias)", messages.len()),
                    )
                    .icon(IconName::Download)
                    .separator(),
                ];
                if messages
                    .iter()
                    .any(|message| !message.text.trim().is_empty())
                {
                    entries.push(
                        ContextMenuEntry::new("album.copy", "Copiar legenda")
                            .icon(IconName::Copy)
                            .separator(),
                    );
                }
                if anchor.has_media {
                    entries.push(
                        ContextMenuEntry::new("album.forward", "Encaminhar")
                            .icon(IconName::Forward)
                            .separator(),
                    );
                }
                entries
            }
            ContextTarget::Download { record } => {
                let mut entries = Vec::new();
                if record.status == "completed" && record.file_path.is_some() {
                    entries.push(
                        ContextMenuEntry::new("download.open", "Abrir arquivo")
                            .icon(IconName::External),
                    );
                    entries.push(
                        ContextMenuEntry::new("download.reveal", "Mostrar na pasta")
                            .icon(IconName::Folder),
                    );
                }
                if record.file_path.is_some() {
                    entries.push(
                        ContextMenuEntry::new("download.copy_path", "Copiar caminho")
                            .icon(IconName::Copy),
                    );
                }
                if record.source_url.is_some() {
                    entries.push(
                        ContextMenuEntry::new("download.copy_url", "Copiar link")
                            .icon(IconName::Link),
                    );
                }
                entries.push(
                    ContextMenuEntry::new("download.remove", "Remover do histórico")
                        .icon(IconName::Trash)
                        .danger()
                        .separator(),
                );
                entries
            }
        }
    }
}

/// Position and content of the open context menu.
#[derive(Clone, PartialEq)]
pub struct ContextMenuState {
    pub x: f64,
    pub y: f64,
    pub target: ContextTarget,
    pub entries: Vec<ContextMenuEntry>,
}

impl ContextMenuState {
    pub fn new(x: f64, y: f64, target: ContextTarget) -> Self {
        let entries = target.entries();
        Self {
            x,
            y,
            target,
            entries,
        }
    }
}

/// Runs the action chosen in the context menu.
pub fn handle_context_action(
    services: AppServices,
    mut ui: UiState,
    target: &ContextTarget,
    id: &str,
) {
    match (target, id) {
        (ContextTarget::Chat { id: chat_id }, "chat.open") => {
            if let Some(chat) = ui.chats().into_iter().find(|chat| chat.id == *chat_id) {
                ui.selected_chat.set(Some(chat));
                ui.messages.set(Vec::new());
                ui.messages_key.set(None);
                ui.messages_has_more.set(false);
                ui.selected_topic.set(None);
                ui.hidden_topics.set(Vec::new());
                ui.topic_browser_open.set(false);
                ui.reply_to.set(None);
            }
        }
        (ContextTarget::Chat { id: chat_id }, "chat.read") => {
            let Ok(parsed) = chat_id.parse::<i64>() else {
                return;
            };
            let services = services.clone();
            spawn(async move {
                if let Err(error) = services.telegram.read_chat(parsed).await {
                    push_toast(ui.toasts, error, ToastKind::Error);
                }
            });
        }
        (ContextTarget::Chat { .. }, "chat.mass") => {
            ui.mass_download_open.set(true);
        }
        (ContextTarget::Chat { id: chat_id }, "chat.copy_id") => {
            let chat_id = chat_id.clone();
            spawn(async move {
                copy_to_clipboard(chat_id).await;
            });
        }
        (ContextTarget::Topic { chat_id, topic, .. }, "topic.toggle") => {
            let hidden = toggle_topic_hidden(&services, ui, *chat_id, topic);
            let message = if hidden {
                format!("Tópico \"{}\" oculto.", topic.title)
            } else {
                format!("Tópico \"{}\" visível novamente.", topic.title)
            };
            push_toast(ui.toasts, message, ToastKind::Info);
        }
        (ContextTarget::Topic { .. }, "topic.browse") | (ContextTarget::Topics, "topic.browse") => {
            ui.topic_browser_open.set(true);
        }
        (ContextTarget::Message { message, .. }, "message.reply") => {
            ui.reply_to.set(Some(message.as_ref().clone()));
        }
        (ContextTarget::Message { chat_id, message }, "message.react") => {
            if let Some(menu) = ui.context_menu() {
                ui.reaction_picker.set(Some(ReactionPickerState {
                    chat_id: *chat_id,
                    message_id: message.id,
                    x: menu.x,
                    y: menu.y,
                }));
            }
        }
        (ContextTarget::Message { message, .. }, "message.forward") => {
            let chat_id = ui
                .selected_chat()
                .and_then(|chat| chat.id.parse::<i64>().ok());
            if let Some(chat_id) = chat_id {
                ui.forward_state.set(Some(crate::state::ForwardState {
                    source_chat_id: chat_id,
                    message: message.as_ref().clone(),
                }));
            }
        }
        (ContextTarget::Message { message, .. }, "message.copy") => {
            let text = message.text.clone();
            if text.trim().is_empty() {
                push_toast(
                    ui.toasts,
                    "A mensagem não tem texto para copiar.",
                    ToastKind::Info,
                );
                return;
            }
            spawn(async move {
                copy_to_clipboard(text).await;
            });
        }
        (ContextTarget::Message { chat_id, message }, "message.media") => {
            ui.lightbox.set(Some(LightboxState {
                chat_id: *chat_id,
                message_id: message.id,
                is_video: message.is_video,
                is_photo: message.is_photo,
                title: message.sender_name.clone(),
            }));
        }
        (ContextTarget::Message { chat_id, message }, "message.save") => {
            save_media(services, ui, *chat_id, message.as_ref().clone());
        }
        (
            ContextTarget::Album {
                chat_id,
                messages,
                anchor_id,
            },
            "album.open",
        ) => {
            if let Some(message) = messages.iter().find(|message| message.id == *anchor_id) {
                ui.lightbox.set(Some(LightboxState {
                    chat_id: *chat_id,
                    message_id: message.id,
                    is_video: message.is_video,
                    is_photo: message.is_photo,
                    title: message.sender_name.clone(),
                }));
            }
        }
        (
            ContextTarget::Album {
                chat_id,
                messages,
                anchor_id,
            },
            "album.save_one",
        ) => {
            if let Some(message) = messages.iter().find(|message| message.id == *anchor_id) {
                save_media(services, ui, *chat_id, message.clone());
            }
        }
        (
            ContextTarget::Album {
                chat_id, messages, ..
            },
            "album.save_all",
        ) => {
            save_album(services, ui, *chat_id, messages.clone());
        }
        (ContextTarget::Album { messages, .. }, "album.copy") => {
            let text = messages
                .iter()
                .find(|message| !message.text.trim().is_empty())
                .map(|message| message.text.clone());
            let Some(text) = text else {
                push_toast(
                    ui.toasts,
                    "O álbum não tem legenda para copiar.",
                    ToastKind::Info,
                );
                return;
            };
            spawn(async move {
                copy_to_clipboard(text).await;
            });
        }
        (
            ContextTarget::Album {
                chat_id,
                messages,
                anchor_id,
            },
            "album.forward",
        ) => {
            if let Some(message) = messages.iter().find(|message| message.id == *anchor_id) {
                ui.forward_state.set(Some(crate::state::ForwardState {
                    source_chat_id: *chat_id,
                    message: message.clone(),
                }));
            }
        }
        (ContextTarget::Download { record }, "download.open") => {
            if let Some(path) = record.file_path.clone() {
                if let Err(error) = plasma_platform::open_path(&path) {
                    push_toast(ui.toasts, error, ToastKind::Error);
                }
            }
        }
        (ContextTarget::Download { record }, "download.reveal") => {
            if let Some(path) = record.file_path.clone() {
                if let Err(error) = plasma_platform::reveal_path(&path) {
                    push_toast(ui.toasts, error, ToastKind::Error);
                }
            }
        }
        (ContextTarget::Download { record }, "download.copy_path") => {
            if let Some(path) = record.file_path.clone() {
                spawn(async move {
                    copy_to_clipboard(path).await;
                });
            }
        }
        (ContextTarget::Download { record }, "download.copy_url") => {
            if let Some(url) = record.source_url.clone() {
                spawn(async move {
                    copy_to_clipboard(url).await;
                });
            }
        }
        (ContextTarget::Download { record }, "download.remove") => {
            let id = record.id.clone();
            let mut next = ui.downloads();
            next.retain(|entry| entry.id != id);
            ui.downloads.set(next.clone());
            let downloads = services.downloads.clone();
            spawn(async move {
                let _ = downloads.save_history(next).await;
            });
        }
        _ => {}
    }
}

/// Saves a single message media to the configured download folder.
pub fn save_media(services: AppServices, ui: UiState, chat_id: i64, message: TdlibMessageInfo) {
    let message_id = message.id;
    spawn(async move {
        match services
            .telegram
            .save_media(chat_id, message_id, None)
            .await
        {
            Ok(result) if result.success => push_toast(
                ui.toasts,
                format!("Mídia salva em {}.", result.file_path.unwrap_or_default()),
                ToastKind::Success,
            ),
            Ok(result) => push_toast(
                ui.toasts,
                result
                    .error
                    .unwrap_or_else(|| "Não foi possível salvar a mídia.".to_string()),
                ToastKind::Error,
            ),
            Err(error) => push_toast(ui.toasts, error, ToastKind::Error),
        }
    });
}

/// Saves every media of an album, one after the other.
pub fn save_album(
    services: AppServices,
    ui: UiState,
    chat_id: i64,
    messages: Vec<TdlibMessageInfo>,
) {
    if messages.is_empty() {
        return;
    }
    let total = messages.len();
    push_toast(
        ui.toasts,
        format!("Salvando {total} mídias do álbum..."),
        ToastKind::Info,
    );
    spawn(async move {
        let mut saved = 0usize;
        let mut last_path = None;
        for message in messages {
            if let Ok(result) = services
                .telegram
                .save_media(chat_id, message.id, None)
                .await
            {
                if result.success {
                    saved += 1;
                    last_path = result.file_path.or(last_path);
                }
            }
        }
        let failed = total - saved;
        let folder = last_path
            .and_then(|path| {
                std::path::Path::new(&path)
                    .parent()
                    .map(|parent| parent.display().to_string())
            })
            .unwrap_or_default();
        if failed == 0 {
            let detail = if folder.is_empty() {
                String::new()
            } else {
                format!(" em {folder}")
            };
            push_toast(
                ui.toasts,
                format!("Álbum salvo: {saved} mídias{detail}."),
                ToastKind::Success,
            );
        } else {
            push_toast(
                ui.toasts,
                format!("Álbum salvo parcialmente: {saved} de {total} mídias."),
                ToastKind::Error,
            );
        }
    });
}

/// Copies text to the clipboard using the webview.
///
/// The async clipboard API is preferred, with a hidden textarea as fallback
/// for contexts where the permission prompt is not available.
pub async fn copy_to_clipboard(text: String) -> bool {
    let payload = serde_json::to_string(&text).unwrap_or_else(|_| "\"\"".to_string());
    let script = format!(
        r#"
        const value = {payload};
        try {{
            if (navigator.clipboard && window.isSecureContext) {{
                await navigator.clipboard.writeText(value);
                return true;
            }}
        }} catch (error) {{ /* fall back to the textarea below */ }}
        const area = document.createElement("textarea");
        area.value = value;
        area.setAttribute("readonly", "");
        area.style.position = "fixed";
        area.style.top = "-1000px";
        area.style.opacity = "0";
        document.body.appendChild(area);
        area.select();
        let copied = false;
        try {{ copied = document.execCommand("copy"); }} catch (error) {{ copied = false; }}
        document.body.removeChild(area);
        return copied;
        "#
    );

    match document::eval(&script).await {
        Ok(value) => value.as_bool().unwrap_or(false),
        Err(_) => false,
    }
}
