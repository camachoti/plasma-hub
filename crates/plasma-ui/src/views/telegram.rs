//! Telegram view: real TDLib chats, topics, search and cache when signed in;
//! a local preview on the same shell before login.

use crate::state::{
    merge_messages, message_cache_key, push_toast, AppServices, ToastKind, UiState,
};
use crate::views::spike::VirtualTimeline;
use crate::views::timeline::{format_timestamp, MessageTimeline};
use dioxus::prelude::*;
use plasma_telegram::{TdlibChatInfo, TdlibForumTopicInfo, TdlibMessageInfo};

#[component]
pub fn TelegramView() -> Element {
    let ui = use_context::<UiState>();
    if ui.auth_state() == "ready" {
        rsx! { RealTelegram {} }
    } else {
        rsx! { PreviewTelegram {} }
    }
}

#[component]
fn RealTelegram() -> Element {
    let mut ui = use_context::<UiState>();
    let services = use_context::<AppServices>();

    let mut chat_filter = use_signal(String::new);
    let mut message_query = use_signal(String::new);
    let mut search_active = use_signal(|| false);
    let loading_older = use_signal(|| false);
    let mut avatar_chat = use_signal(|| None::<String>);

    let chat_services = services.clone();
    use_effect(move || {
        let _tick = ui.telegram_tick();
        let services = chat_services.clone();
        spawn(async move {
            ui.chats_loading.set(true);
            ui.chats_error.set(None);
            match services.telegram.get_chats(150).await {
                Ok(result) if result.success => {
                    let dialogs = result.dialogs;
                    if let Some(selected) = ui.selected_chat() {
                        if let Some(updated) =
                            dialogs.iter().find(|chat| chat.id == selected.id).cloned()
                        {
                            ui.selected_chat.set(Some(updated));
                        }
                    } else if let Some(first) = dialogs.first().cloned() {
                        ui.selected_chat.set(Some(first));
                    }
                    ui.chats.set(dialogs);
                }
                Ok(result) => ui.chats_error.set(
                    result
                        .error
                        .or_else(|| Some("TDLib não retornou conversas.".into())),
                ),
                Err(error) => ui.chats_error.set(Some(error)),
            }
            ui.chats_loading.set(false);
        });
    });

    let topic_services = services.clone();
    use_effect(move || {
        let selected = ui.selected_chat();
        let _tick = ui.telegram_tick();
        let Some(chat) = selected else {
            ui.topics.set(Vec::new());
            ui.selected_topic.set(None);
            return;
        };
        if !chat.has_topics {
            ui.topics.set(Vec::new());
            ui.selected_topic.set(None);
            return;
        }
        let Some(chat_id) = parse_chat_id(&chat) else {
            return;
        };
        let services = topic_services.clone();
        spawn(async move {
            if let Ok(result) = services.telegram.get_forum_topics(chat_id, 200).await {
                if result.success && ui.selected_chat().map(|item| item.id) == Some(chat.id.clone())
                {
                    ui.topics.set(result.topics);
                }
            }
        });
    });

    let message_services = services.clone();
    use_effect(move || {
        let selected = ui.selected_chat();
        let topic = ui.selected_topic();
        let _tick = ui.telegram_tick();
        let Some(chat) = selected else {
            ui.messages.set(Vec::new());
            ui.messages_loading.set(false);
            ui.messages_has_more.set(false);
            return;
        };
        let services = message_services.clone();
        spawn(async move {
            load_conversation(services, ui, chat, topic).await;
        });
    });

    let avatar_services = services.clone();
    use_effect(move || {
        let current = ui.selected_chat().map(|chat| chat.id);
        if current == avatar_chat() {
            return;
        }
        avatar_chat.set(current.clone());
        let Some(chat_id) = current.as_deref().and_then(|id| id.parse::<i64>().ok()) else {
            ui.avatar.set(None);
            return;
        };
        let expected = current.unwrap_or_default();
        ui.avatar.set(None);
        let services = avatar_services.clone();
        spawn(async move {
            if let Ok(Some(data_url)) = services.telegram.download_avatar(chat_id).await {
                if ui.selected_chat().map(|item| item.id) == Some(expected) {
                    ui.avatar.set(Some(data_url));
                }
            }
        });
    });

    let filter = chat_filter().trim().to_lowercase();
    let chats: Vec<TdlibChatInfo> = ui
        .chats()
        .into_iter()
        .filter(|chat| {
            filter.is_empty()
                || chat.title.to_lowercase().contains(&filter)
                || chat.last_message_text.to_lowercase().contains(&filter)
        })
        .collect();
    let selected = ui.selected_chat();
    let selected_id = selected.as_ref().map(|chat| chat.id.clone());
    let topic = ui.selected_topic();
    let topics = ui.topics();
    let in_search = search_active();
    let display_messages = if in_search {
        ui.search_results()
    } else {
        ui.messages()
    };

    rsx! {
        div { class: "telegram-layout",
            aside { class: "chat-list-panel",
                header { class: "chat-list-header",
                    div { class: "chat-list-title-row",
                        h2 { "Conversas" }
                        if ui.chats_loading() {
                            span { class: "pill", "carregando" }
                        }
                    }
                    div { class: "chat-search-row",
                        input {
                            class: "chat-search-input",
                            r#type: "search",
                            placeholder: "Filtrar conversas",
                            value: "{chat_filter}",
                            oninput: move |event| chat_filter.set(event.value()),
                        }
                        button {
                            class: "btn small ghost",
                            title: "Atualizar conversas",
                            onclick: move |_| {
                                let next = ui.telegram_tick().wrapping_add(1);
                                ui.telegram_tick.set(next);
                            },
                            "↻"
                        }
                    }
                    if let Some(error) = ui.chats_error() {
                        p { class: "error-banner", "{error}" }
                    }
                }
                div { class: "chat-list",
                    if chats.is_empty() {
                        div { class: "chat-list-empty",
                            p { class: "hint",
                                if ui.chats_loading() {
                                    "Carregando conversas do Telegram..."
                                } else {
                                    "Nenhuma conversa encontrada."
                                }
                            }
                        }
                    }
                    for chat in chats {
                        button {
                            key: "{chat.id}",
                            class: if selected_id.as_deref() == Some(chat.id.as_str()) { "chat-row active" } else { "chat-row" },
                            onclick: {
                                let chat = chat.clone();
                                move |_| {
                                    ui.selected_chat.set(Some(chat.clone()));
                                    ui.selected_topic.set(None);
                                    ui.avatar.set(None);
                                    ui.messages.set(Vec::new());
                                    ui.messages_has_more.set(false);
                                    ui.search_results.set(Vec::new());
                                    ui.search_error.set(None);
                                    ui.search_loading.set(false);
                                    search_active.set(false);
                                    message_query.set(String::new());
                                }
                            },
                            div { class: "chat-avatar", "{chat.title.chars().next().unwrap_or('?')}" }
                            div { class: "chat-copy",
                                span { class: "chat-name", "{chat.title}" }
                                span { class: "chat-preview",
                                    if chat.last_message_has_media { "▣ " }
                                    if chat.last_message_text.trim().is_empty() {
                                        "{chat_kind(&chat)}"
                                    } else {
                                        "{chat.last_message_text}"
                                    }
                                }
                            }
                            div { class: "chat-meta",
                                span { class: "chat-time", "{format_timestamp(chat.last_message_date)}" }
                                if chat.unread_count > 0 {
                                    span { class: "unread", "{chat.unread_count}" }
                                }
                            }
                        }
                    }
                }
            }

            main { class: "chat-main",
                if let Some(chat) = selected {
                    header { class: "chat-header",
                        div { class: "chat-header-title",
                            if let Some(avatar) = ui.avatar() {
                                img { class: "chat-header-avatar", src: "{avatar}", alt: "" }
                            } else {
                                div { class: "chat-header-avatar", "{chat.title.chars().next().unwrap_or('?')}" }
                            }
                            div { class: "chat-header-copy",
                                strong { "{chat.title}" }
                                span {
                                    if let Some(topic) = topic.as_ref() {
                                        "Tópico: {topic.title}"
                                    } else {
                                        "{chat_kind(&chat)} · {chat.last_message_text}"
                                    }
                                }
                            }
                        }
                        div { class: "chat-header-actions",
                            input {
                                class: "chat-search-input header-search",
                                r#type: "search",
                                placeholder: "Buscar mensagens",
                                value: "{message_query}",
                                oninput: move |event| message_query.set(event.value()),
                            }
                            button {
                                class: "btn small primary",
                                disabled: ui.search_loading(),
                                onclick: {
                                    let services = services.clone();
                                    move |_| {
                                        let Some(chat) = ui.selected_chat() else { return; };
                                        let query = message_query().trim().to_string();
                                        if query.is_empty() {
                                            search_active.set(false);
                                            ui.search_results.set(Vec::new());
                                            return;
                                        }
                                        search_active.set(true);
                                        run_message_search(
                                            services.clone(),
                                            ui,
                                            chat,
                                            ui.selected_topic(),
                                            query,
                                        );
                                    }
                                },
                                if ui.search_loading() { "Buscando..." } else { "Buscar" }
                            }
                            if in_search {
                                button {
                                    class: "btn small ghost",
                                    onclick: move |_| {
                                        search_active.set(false);
                                        message_query.set(String::new());
                                        ui.search_results.set(Vec::new());
                                        ui.search_error.set(None);
                                        ui.search_loading.set(false);
                                    },
                                    "Limpar"
                                }
                            }
                            button {
                                class: "btn small ghost",
                                onclick: move |_| push_toast(
                                    ui.toasts,
                                    "Envio de mensagens e mídia entra na fase 4.",
                                    ToastKind::Info,
                                ),
                                "Nova"
                            }
                        }
                    }

                    if !topics.is_empty() {
                        div { class: "topics-bar",
                            button {
                                class: if topic.is_none() { "topic-chip active" } else { "topic-chip" },
                                onclick: move |_| ui.selected_topic.set(None),
                                "Todos"
                            }
                            for item in topics {
                                button {
                                    key: "{item.id}",
                                    class: if topic.as_ref().map(|current| current.id) == Some(item.id) { "topic-chip active" } else { "topic-chip" },
                                    onclick: {
                                        let item = item.clone();
                                        move |_| ui.selected_topic.set(Some(item.clone()))
                                    },
                                    if item.is_closed { "🔒 " }
                                    "{item.title}"
                                    if item.unread_count > 0 {
                                        span { class: "topic-unread", "{item.unread_count}" }
                                    }
                                }
                            }
                        }
                    }

                    div { class: "chat-body",
                        if let Some(error) = ui.messages_error() {
                            div { class: "error-banner timeline-error", "{error}" }
                        }
                        if ui.messages_loading() && display_messages.is_empty() {
                            div { class: "timeline-loading",
                                div { class: "spinner" }
                                span { "Carregando mensagens..." }
                            }
                        } else {
                            div { class: "timeline-shell",
                                if !in_search && ui.messages_has_more() {
                                    div { class: "timeline-toolbar",
                                        button {
                                            class: "btn small ghost",
                                            disabled: loading_older(),
                                            onclick: {
                                                let services = services.clone();
                                                move |_| {
                                                    let Some(chat) = ui.selected_chat() else { return; };
                                                    load_older(
                                                        services.clone(),
                                                        ui,
                                                        chat,
                                                        ui.selected_topic(),
                                                        loading_older,
                                                    );
                                                }
                                            },
                                            if loading_older() { "Carregando..." } else { "Carregar mensagens antigas" }
                                        }
                                        span { class: "hint", "{display_messages.len()} mensagens" }
                                    }
                                }
                                if in_search {
                                    div { class: "timeline-search-label",
                                        span { class: "pill", "Busca: {message_query}" }
                                        span { class: "hint", "{display_messages.len()} resultados" }
                                    }
                                }
                                div { class: "timeline-fill",
                                    MessageTimeline {
                                        messages: display_messages,
                                        empty_label: if in_search { "Nenhum resultado" } else { "Sem mensagens" },
                                    }
                                }
                            }
                        }
                    }
                } else {
                    div { class: "empty-state",
                        div { class: "empty-card",
                            div { class: "empty-icon", "TG" }
                            h2 { "Escolha uma conversa" }
                            p { "As conversas são carregadas do TDLib e guardadas no SQLite local." }
                        }
                    }
                }
            }
        }
    }
}

fn run_message_search(
    services: AppServices,
    mut ui: UiState,
    chat: TdlibChatInfo,
    topic: Option<TdlibForumTopicInfo>,
    query: String,
) {
    let Some(chat_id) = parse_chat_id(&chat) else {
        ui.search_error.set(Some("Chat inválido.".to_string()));
        return;
    };
    ui.search_loading.set(true);
    ui.search_error.set(None);
    ui.search_results.set(Vec::new());
    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());
    spawn(async move {
        match services
            .telegram
            .search_chat_messages(chat_id, query, 100, None, topic_id, topic_kind)
            .await
        {
            Ok(result) if result.success => ui.search_results.set(result.messages),
            Ok(result) => ui.search_error.set(
                result
                    .error
                    .or_else(|| Some("Busca não retornou resultados.".into())),
            ),
            Err(error) => ui.search_error.set(Some(error)),
        }
        ui.search_loading.set(false);
    });
}

async fn load_conversation(
    services: AppServices,
    mut ui: UiState,
    chat: TdlibChatInfo,
    topic: Option<TdlibForumTopicInfo>,
) {
    let Some(chat_id) = parse_chat_id(&chat) else {
        ui.messages_error.set(Some("Chat inválido.".to_string()));
        return;
    };
    let cache_key = message_cache_key(&chat.id, topic.as_ref());
    ui.messages_loading.set(true);
    ui.messages_error.set(None);
    ui.messages.set(Vec::new());
    ui.messages_has_more.set(false);

    let mut cached = Vec::new();
    if let Ok(cache) = services.telegram.cached_messages(cache_key.clone()).await {
        cached = cache
            .messages
            .iter()
            .filter_map(|value| serde_json::from_value::<TdlibMessageInfo>(value.clone()).ok())
            .collect();
        if !cached.is_empty() && conversation_matches(ui, &chat, topic.as_ref()) {
            ui.messages.set(cached.clone());
        }
    }

    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());
    match services
        .telegram
        .get_messages(chat_id, 80, 0, 0, topic_id, topic_kind)
        .await
    {
        Ok(result) if result.success => {
            if !conversation_matches(ui, &chat, topic.as_ref()) {
                return;
            }
            let mut merged = cached;
            merge_messages(&mut merged, result.messages);
            ui.messages.set(merged.clone());
            ui.messages_has_more.set(result.has_more);
            let _ = services.telegram.cache_messages(cache_key, merged).await;
        }
        Ok(result) => {
            if conversation_matches(ui, &chat, topic.as_ref()) {
                ui.messages_error.set(
                    result
                        .error
                        .or_else(|| Some("Mensagens indisponíveis.".into())),
                );
            }
        }
        Err(error) => {
            if conversation_matches(ui, &chat, topic.as_ref()) {
                ui.messages_error.set(Some(error));
            }
        }
    }

    if conversation_matches(ui, &chat, topic.as_ref()) {
        ui.messages_loading.set(false);
    }
}

fn load_older(
    services: AppServices,
    mut ui: UiState,
    chat: TdlibChatInfo,
    topic: Option<TdlibForumTopicInfo>,
    mut loading: Signal<bool>,
) {
    if loading() {
        return;
    }
    let Some(chat_id) = parse_chat_id(&chat) else {
        return;
    };
    let Some(oldest_id) = ui.messages().first().map(|message| message.id) else {
        return;
    };
    loading.set(true);
    let cache_key = message_cache_key(&chat.id, topic.as_ref());
    let topic_id = topic.as_ref().map(|item| item.id);
    let topic_kind = topic.as_ref().map(|item| item.kind.clone());
    spawn(async move {
        match services
            .telegram
            .get_messages(chat_id, 50, oldest_id, 0, topic_id, topic_kind)
            .await
        {
            Ok(result) if result.success => {
                if !conversation_matches(ui, &chat, topic.as_ref()) {
                    loading.set(false);
                    return;
                }
                let mut merged = ui.messages();
                merge_messages(&mut merged, result.messages);
                ui.messages.set(merged.clone());
                ui.messages_has_more.set(result.has_more);
                let _ = services.telegram.cache_messages(cache_key, merged).await;
            }
            Ok(result) => ui.messages_error.set(
                result
                    .error
                    .or_else(|| Some("Mensagens antigas indisponíveis.".into())),
            ),
            Err(error) => ui.messages_error.set(Some(error)),
        }
        loading.set(false);
    });
}

fn conversation_matches(
    ui: UiState,
    chat: &TdlibChatInfo,
    topic: Option<&TdlibForumTopicInfo>,
) -> bool {
    ui.selected_chat().map(|item| item.id) == Some(chat.id.clone())
        && ui.selected_topic().map(|item| (item.id, item.kind))
            == topic.map(|item| (item.id, item.kind.clone()))
}

fn parse_chat_id(chat: &TdlibChatInfo) -> Option<i64> {
    chat.id.parse::<i64>().ok()
}

fn chat_kind(chat: &TdlibChatInfo) -> &'static str {
    if chat.is_channel {
        "Canal"
    } else if chat.is_group {
        "Grupo"
    } else {
        "Conversa privada"
    }
}

/* ------------------------------------------------------------------------- */
/* Preview before login                                                      */
/* ------------------------------------------------------------------------- */

#[derive(Clone, Copy)]
struct PreviewChat {
    name: &'static str,
    preview: &'static str,
    time: &'static str,
    unread: u32,
    tone: &'static str,
}

const PREVIEW_CHATS: [PreviewChat; 6] = [
    PreviewChat {
        name: "Plasma Hub",
        preview: "A migração para Rust começou 🚀",
        time: "agora",
        unread: 3,
        tone: "P",
    },
    PreviewChat {
        name: "Design System",
        preview: "Paletas, densidade e tokens prontos.",
        time: "09:42",
        unread: 0,
        tone: "D",
    },
    PreviewChat {
        name: "Android",
        preview: "NDK 27 e target aarch64 validados.",
        time: "ontem",
        unread: 1,
        tone: "A",
    },
    PreviewChat {
        name: "Rust Rewrite",
        preview: "Fase 3: TDLib, timeline e cache.",
        time: "ontem",
        unread: 0,
        tone: "R",
    },
    PreviewChat {
        name: "Downloads",
        preview: "Fila, retry e histórico na fase 5.",
        time: "seg",
        unread: 0,
        tone: "↓",
    },
    PreviewChat {
        name: "Twitter / X",
        preview: "Cookies continuam em disco.",
        time: "seg",
        unread: 0,
        tone: "X",
    },
];

#[component]
fn PreviewTelegram() -> Element {
    let mut ui = use_context::<UiState>();
    let mut selected = use_signal(|| 0usize);
    let chat = PREVIEW_CHATS[selected()];

    rsx! {
        div { class: "telegram-layout",
            aside { class: "chat-list-panel",
                header { class: "chat-list-header",
                    div { class: "chat-list-title-row",
                        h2 { "Conversas" }
                        span { class: "pill", "prévia" }
                    }
                }
                div { class: "chat-list",
                    for (index, item) in PREVIEW_CHATS.iter().enumerate() {
                        button {
                            key: "{item.name}",
                            class: if selected() == index { "chat-row active" } else { "chat-row" },
                            onclick: move |_| {
                                selected.set(index);
                                ui.show_spike.set(false);
                            },
                            div { class: "chat-avatar", "{item.tone}" }
                            div { class: "chat-copy",
                                span { class: "chat-name", "{item.name}" }
                                span { class: "chat-preview", "{item.preview}" }
                            }
                            div { class: "chat-meta",
                                span { class: "chat-time", "{item.time}" }
                                if item.unread > 0 {
                                    span { class: "unread", "{item.unread}" }
                                }
                            }
                        }
                    }
                }
            }

            main { class: "chat-main",
                header { class: "chat-header",
                    div { class: "chat-header-copy",
                        strong { "{chat.name}" }
                        span { "Prévia local · entre para carregar suas conversas" }
                    }
                    div { class: "topbar-actions",
                        button {
                            class: "btn small ghost",
                            onclick: move |_| {
                                let next = !ui.show_spike();
                                ui.show_spike.set(next);
                                if next {
                                    push_toast(
                                        ui.toasts,
                                        "Spike: 5.000 mensagens com alturas variáveis.",
                                        ToastKind::Info,
                                    );
                                }
                            },
                            if ui.show_spike() { "Fechar spike" } else { "Spike 5k" }
                        }
                        button {
                            class: "btn small ghost",
                            onclick: move |_| {
                                let next = ui.telegram_tick().wrapping_add(1);
                                ui.telegram_tick.set(next);
                                ui.logged_in.set(false);
                            },
                            "Entrar"
                        }
                        button {
                            class: "btn small primary",
                            onclick: move |_| push_toast(
                                ui.toasts,
                                "Composer, envio e mídia entram na fase 4.",
                                ToastKind::Info,
                            ),
                            "Nova mensagem"
                        }
                    }
                }
                div { class: "chat-body",
                    if ui.show_spike() {
                        VirtualTimeline {}
                    } else {
                        div { class: "empty-state",
                            div { class: "empty-card",
                                div { class: "empty-icon", "P" },
                                h2 { "{chat.name}" }
                                p {
                                    "Esta é a prévia da interface Rust. Entre com o Telegram para \
                                     carregar conversas, tópicos, busca e timeline reais."
                                }
                                div { class: "mock-messages",
                                    div { class: "mock-message",
                                        strong { "Ana" }
                                        p { "O backend já está em crates separados e o Tauri continua funcionando." }
                                    }
                                    div { class: "mock-message",
                                        strong { "Bruno" }
                                        p { "A timeline real usa virtualização e cache SQLite, sem mudar o app React." }
                                    }
                                    div { class: "mock-message own",
                                        strong { "Você" }
                                        p { "O shell Rust abre, autentica no TDLib e lê mensagens." }
                                    }
                                }
                                div { class: "login-actions",
                                    button {
                                        class: "btn primary",
                                        onclick: move |_| ui.logged_in.set(false),
                                        "Entrar com Telegram"
                                    }
                                    button {
                                        class: "btn ghost",
                                        onclick: move |_| ui.show_spike.set(true),
                                        "Abrir spike de virtualização"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
