//! Telegram view: real TDLib chats, topics, search and cache when signed in;
//! a local preview on the same shell before login.

use crate::context::{ContextMenuState, ContextTarget};
use crate::icons::{Icon, IconName};
use crate::state::{
    load_hidden_topics, merge_messages, message_cache_key, push_toast, set_if_changed,
    topic_visibility_key, AppServices, ForwardState, LightboxState, MassDownloadProgress,
    ToastKind, UiState,
};
use crate::views::chat_list;
use crate::views::chat_list::ChatRow;
use crate::views::duplicate_search::{DuplicateSearchDialog, DuplicateSearchItem};
use crate::views::select::{Select, SelectOption};
use crate::views::spike::VirtualTimeline;
use crate::views::timeline::{format_timestamp, MessageAction, MessageThumbnail, MessageTimeline};
use crate::views::topics::TopicBrowser;
use dioxus::prelude::*;
use plasma_telegram::{
    TdlibChatInfo, TdlibForumTopicInfo, TdlibMassDownloadRequest, TdlibMessageInfo,
    TdlibSharedMediaItem,
};

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
    let mut composer = use_signal(String::new);
    let sending = use_signal(|| false);
    let composer_error = use_signal(|| None::<String>);
    let mut shared_open = use_signal(|| false);
    let mut selected_messages = use_signal(Vec::<i64>::new);
    let mut duplicate_search_open = use_signal(|| false);
    let mut shared_items = use_signal(Vec::<TdlibSharedMediaItem>::new);
    let mut shared_loading = use_signal(|| false);
    let mut shared_error = use_signal(|| None::<String>);
    let mut forwarding = use_signal(|| false);

    let chat_services = services.clone();
    use_effect(move || {
        let _tick = ui.telegram_tick();
        let services = chat_services.clone();
        spawn(async move {
            // Only the first load shows the placeholder; refreshes keep the list.
            set_if_changed(ui.chats_loading, ui.chats().is_empty());
            set_if_changed(ui.chats_error, None::<String>);
            match services.telegram.get_chats(150).await {
                Ok(result) if result.success => {
                    let dialogs = result.dialogs;
                    if let Some(selected) = ui.selected_chat() {
                        if let Some(updated) =
                            dialogs.iter().find(|chat| chat.id == selected.id).cloned()
                        {
                            set_if_changed(ui.selected_chat, Some(updated));
                        }
                    } else if let Some(first) = dialogs.first().cloned() {
                        if let Some(chat_id) = parse_chat_id(&first) {
                            let _ = services.telegram.read_chat(chat_id).await;
                        }
                        ui.selected_chat.set(Some(first));
                    }
                    set_if_changed(ui.chats, dialogs);
                }
                Ok(result) => {
                    set_if_changed(
                        ui.chats_error,
                        result
                            .error
                            .or_else(|| Some("TDLib não retornou conversas.".into())),
                    );
                }
                Err(error) => {
                    set_if_changed(ui.chats_error, Some(error));
                }
            }
            set_if_changed(ui.chats_loading, false);
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
                    set_if_changed(ui.topics, result.topics);
                }
            }
        });
    });

    // Hidden topics are tracked per chat; load them when the conversation
    // changes instead of on every Telegram tick.
    let mut hidden_topics_chat = use_hook(|| CopyValue::new(None::<i64>));
    let hidden_topics_services = services.clone();
    use_effect(move || {
        let chat_id = ui.selected_chat().and_then(|chat| parse_chat_id(&chat));
        if hidden_topics_chat() == chat_id {
            return;
        }
        hidden_topics_chat.set(chat_id);
        match chat_id {
            Some(chat_id) => load_hidden_topics(&hidden_topics_services, ui, chat_id),
            None => ui.hidden_topics.set(Vec::new()),
        }
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
            ui.messages_key.set(None);
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

    let shared_services = services.clone();
    use_effect(move || {
        if !shared_open() {
            return;
        }
        let Some(chat_id) = ui.selected_chat().and_then(|chat| parse_chat_id(&chat)) else {
            return;
        };
        let services = shared_services.clone();
        shared_loading.set(true);
        shared_error.set(None);
        spawn(async move {
            match services.telegram.shared_media(chat_id, 80).await {
                Ok(result) if result.success => shared_items.set(result.media),
                Ok(result) => shared_error.set(
                    result
                        .error
                        .or_else(|| Some("Mídia compartilhada indisponível.".into())),
                ),
                Err(error) => shared_error.set(Some(error)),
            }
            shared_loading.set(false);
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
    let timeline_chat_id = selected_id
        .as_deref()
        .and_then(|id| id.parse::<i64>().ok())
        .unwrap_or_default();
    let topic = ui.selected_topic();
    let topics = ui.topics();
    let in_search = search_active();
    let display_messages = if in_search {
        ui.search_results()
    } else {
        ui.messages()
    };
    let composer_disabled = selected
        .as_ref()
        .map(|chat| !chat.can_send_messages)
        .unwrap_or(true)
        || topic.as_ref().is_some_and(|topic| topic.is_closed);
    let media_disabled = selected
        .as_ref()
        .map(|chat| !chat.can_send_media)
        .unwrap_or(true)
        || topic.as_ref().is_some_and(|topic| topic.is_closed);

    let on_message_action = move |(message, action): (TdlibMessageInfo, MessageAction)| match action
    {
        MessageAction::Reply => ui.reply_to.set(Some(message)),
        MessageAction::OpenMedia => ui.lightbox.set(Some(LightboxState {
            chat_id: timeline_chat_id,
            message_id: message.id,
            is_video: message.is_video,
            is_photo: message.is_photo,
            title: message.sender_name,
        })),
        MessageAction::Forward => {
            if timeline_chat_id != 0 {
                ui.forward_state.set(Some(ForwardState {
                    source_chat_id: timeline_chat_id,
                    message,
                }));
            }
        }
    };

    let forward = ui.forward_state();
    let lightbox_state = ui.lightbox();
    let duplicate_items: Vec<DuplicateSearchItem> = display_messages
        .iter()
        .filter(|message| selected_messages().contains(&message.id) && message.has_media)
        .map(|message| DuplicateSearchItem {
            chat_id: timeline_chat_id,
            message_id: message.id,
            name: format!(
                "{} · {}",
                if message.is_video {
                    "Vídeo"
                } else if message.is_photo {
                    "Foto"
                } else {
                    "Mídia"
                },
                format_timestamp(message.date)
            ),
            is_video: message.is_video,
        })
        .collect();
    let mass_chat = selected.clone();
    let mass_topics = topics.clone();
    // Hidden topics disappear from the chips bar, except the one currently
    // open (it stays visible so the active conversation is not a mystery).
    let hidden_keys = ui.hidden_topics();
    let listed_topics: Vec<TdlibForumTopicInfo> = topics
        .iter()
        .filter(|item| {
            let hidden = hidden_keys
                .iter()
                .any(|key| *key == topic_visibility_key(item));
            !hidden || topic.as_ref().map(|current| current.id) == Some(item.id)
        })
        .cloned()
        .collect();
    let browser_chat = selected.clone();

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
                        ChatRow {
                            key: "{chat.id}",
                            chat: chat.clone(),
                            active: selected_id.as_deref() == Some(chat.id.as_str()),
                            on_open: {
                                let services = services.clone();
                                move |selected: TdlibChatInfo| {
                                    search_active.set(false);
                                    message_query.set(String::new());
                                    chat_list::open_chat(services.clone(), ui, selected);
                                }
                            },
                            on_context: {
                                move |(selected, x, y)| {
                                    chat_list::open_chat_menu(ui, selected, x, y);
                                }
                            },
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
                                title: "Baixar todas as mídias desta conversa",
                                onclick: move |_| ui.mass_download_open.set(true),
                                "Download em massa"
                            }
                            button {
                                class: "btn small ghost",
                                onclick: move |_| shared_open.set(true),
                                "Mídia"
                            }
                        }
                    }

                    if !topics.is_empty() {
                        div { class: "topics-bar",
                            button {
                                class: if topic.is_none() { "topic-chip active" } else { "topic-chip" },
                                onclick: move |_| {
                                    ui.selected_topic.set(None);
                                    ui.reply_to.set(None);
                                },
                                "Todos"
                            }
                            for item in listed_topics {
                                button {
                                    key: "{item.kind}:{item.id}",
                                    class: if topic.as_ref().map(|current| current.id) == Some(item.id) { "topic-chip active" } else { "topic-chip" },
                                    onclick: {
                                        let item = item.clone();
                                        move |_| {
                                            ui.selected_topic.set(Some(item.clone()));
                                            ui.reply_to.set(None);
                                        }
                                    },
                                    oncontextmenu: {
                                        let item = item.clone();
                                        let hidden = hidden_keys
                                            .iter()
                                            .any(|key| *key == topic_visibility_key(&item));
                                        move |event: Event<MouseData>| {
                                            event.prevent_default();
                                            event.stop_propagation();
                                            let point = event.data().client_coordinates();
                                            ui.context_menu.set(Some(ContextMenuState::new(
                                                point.x,
                                                point.y,
                                                ContextTarget::Topic {
                                                    chat_id: timeline_chat_id,
                                                    topic: Box::new(item.clone()),
                                                    hidden,
                                                },
                                            )));
                                        }
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
                                        chat_id: timeline_chat_id,
                                        messages: ui.messages,
                                        search_results: ui.search_results,
                                        show_search: search_active,
                                        empty_label: if in_search { "Nenhum resultado" } else { "Sem mensagens" },
                                        selected: selected_messages(),
                                        on_toggle_select: {
                                            move |message_id| {
                                                let mut current = selected_messages();
                                                if let Some(position) =
                                                    current.iter().position(|id| *id == message_id)
                                                {
                                                    current.remove(position);
                                                } else {
                                                    current.push(message_id);
                                                }
                                                selected_messages.set(current);
                                            }
                                        },
                                        on_action: on_message_action,
                                        on_select_album: move |ids: Vec<i64>| {
                                            let mut current = selected_messages();
                                            let all_selected =
                                                ids.iter().all(|id| current.contains(id));
                                            if all_selected {
                                                current.retain(|id| !ids.contains(id));
                                            } else {
                                                for id in ids {
                                                    if !current.contains(&id) {
                                                        current.push(id);
                                                    }
                                                }
                                            }
                                            selected_messages.set(current);
                                        },
                                    }
                                }
                            }
                        }
                    }

                    if !selected_messages().is_empty() {
                        div { class: "selection-bar",
                            div { class: "selection-bar-copy",
                                span { class: "pill", "{selected_messages().len()} selecionada(s)" }
                                span { class: "hint", "Mídias marcadas na conversa." }
                            }
                            div { class: "selection-bar-actions",
                                button {
                                    class: "btn small",
                                    disabled: duplicate_items.is_empty(),
                                    onclick: move |_| duplicate_search_open.set(true),
                                    "Procurar no sistema"
                                }
                                button {
                                    class: "btn small ghost",
                                    onclick: move |_| selected_messages.set(Vec::new()),
                                    "Limpar"
                                }
                            }
                        }
                    }

                    div { class: "composer",
                        if let Some(reply) = ui.reply_to() {
                            if !composer_disabled {
                                div { class: "reply-banner",
                                    div {
                                        span { class: "hint", "Respondendo a" }
                                        strong { " {reply.sender_name}" }
                                        p { "{reply.text}" }
                                    }
                                    button {
                                        class: "btn small ghost",
                                        onclick: move |_| ui.reply_to.set(None),
                                        "Cancelar"
                                    }
                                }
                            }
                        }
                        if let Some(error) = composer_error() {
                            p { class: "error-banner composer-error", "{error}" }
                        }
                        if composer_disabled {
                            // Read-only conversation: the whole composer is
                            // replaced by a compact notice.
                            div { class: "composer-readonly",
                                span { class: "composer-readonly-icon",
                                    Icon { name: IconName::Info, size: 14, class: "icon" }
                                }
                                span {
                                    if topic.as_ref().is_some_and(|topic| topic.is_closed) {
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
                                    rows: "2",
                                    placeholder: "Escreva uma mensagem...",
                                    value: "{composer}",
                                    oninput: move |event| composer.set(event.value()),
                                    onkeydown: {
                                        let services = services.clone();
                                        move |event| {
                                            if event.key() == Key::Enter
                                                && !event.modifiers().shift()
                                                && !event.is_composing()
                                            {
                                                event.prevent_default();
                                                send_text(
                                                    services.clone(),
                                                    ui,
                                                    composer,
                                                    sending,
                                                    composer_error,
                                                    ui.reply_to,
                                                );
                                            }
                                        }
                                    },
                                }
                                div { class: "composer-actions",
                                    button {
                                        class: "btn small ghost",
                                        disabled: media_disabled || sending(),
                                        title: "Anexar mídia",
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
                                                        composer,
                                                        sending,
                                                        composer_error,
                                                        ui.reply_to,
                                                    );
                                                });
                                            }
                                        },
                                        "Anexar"
                                    }
                                    button {
                                        class: "btn primary",
                                        disabled: sending(),
                                        onclick: {
                                            let services = services.clone();
                                            move |_| send_text(
                                                services.clone(),
                                                ui,
                                                composer,
                                                sending,
                                                composer_error,
                                                ui.reply_to,
                                            )
                                        },
                                        if sending() { "Enviando..." } else { "Enviar" }
                                    }
                                }
                            }
                            p { class: "hint composer-hint", "Enter envia · Shift+Enter quebra linha" }
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

            if shared_open() {
                SharedMediaPanel {
                    chat_id: timeline_chat_id,
                    chat_title: selected_id
                        .as_deref()
                        .and_then(|id| ui.chats().into_iter().find(|chat| chat.id == id))
                        .map(|chat| chat.title)
                        .unwrap_or_else(|| "Mídia".to_string()),
                    items: shared_items(),
                    loading: shared_loading(),
                    error: shared_error(),
                    on_close: move |_| shared_open.set(false),
                    on_open: move |item: TdlibSharedMediaItem| {
                        ui.lightbox.set(Some(LightboxState {
                            chat_id: timeline_chat_id,
                            message_id: item.id,
                            is_video: item.is_video,
                            is_photo: item.is_photo,
                            title: item.sender_name,
                        }));
                    },
                }
            }

            if let Some(state) = lightbox_state {
                MediaLightbox {
                    key: "{state.message_id}",
                    state,
                    on_close: move |_| ui.lightbox.set(None),
                }
            }

            if let Some(state) = forward {
                ForwardDialog {
                    state,
                    chats: ui.chats(),
                    busy: forwarding(),
                    on_close: move |_| ui.forward_state.set(None),
                    on_forward: {
                        let services = services.clone();
                        move |target: TdlibChatInfo| {
                            let Some(state) = ui.forward_state() else { return; };
                            let Some(target_id) = parse_chat_id(&target) else { return; };
                            forwarding.set(true);
                            let services = services.clone();
                            spawn(async move {
                                match services
                                    .telegram
                                    .forward_message(
                                        state.source_chat_id,
                                        state.message.id,
                                        target_id,
                                        None,
                                        None,
                                    )
                                    .await
                                {
                                    Ok(result) if result.success => {
                                        push_toast(
                                            ui.toasts,
                                            format!("Mensagem encaminhada para {}.", target.title),
                                            ToastKind::Success,
                                        );
                                        ui.forward_state.set(None);
                                    }
                                    Ok(result) => push_toast(
                                        ui.toasts,
                                        result
                                            .error
                                            .unwrap_or_else(|| "Não foi possível encaminhar.".to_string()),
                                        ToastKind::Error,
                                    ),
                                    Err(error) => push_toast(ui.toasts, error, ToastKind::Error),
                                }
                                forwarding.set(false);
                            });
                        }
                    },
                }
            }

            if duplicate_search_open() {
                DuplicateSearchDialog {
                    items: duplicate_items.clone(),
                    on_close: move |_| duplicate_search_open.set(false),
                }
            }
            if ui.topic_browser_open() {
                if let Some(chat) = browser_chat {
                    TopicBrowser {
                        chat,
                        on_close: move |_| ui.topic_browser_open.set(false),
                    }
                }
            }
            if ui.mass_download_open() {
                if let Some(chat) = mass_chat {
                    MassDownloadPanel {
                        chat,
                        topics: mass_topics,
                        on_close: move |_| ui.mass_download_open.set(false),
                    }
                }
            }
        }
    }
}

#[component]
fn SharedMediaPanel(
    chat_id: i64,
    chat_title: String,
    items: Vec<TdlibSharedMediaItem>,
    loading: bool,
    error: Option<String>,
    on_close: EventHandler<()>,
    on_open: EventHandler<TdlibSharedMediaItem>,
) -> Element {
    rsx! {
        div { class: "media-panel-backdrop", onclick: move |_| on_close.call(()),
            section {
                class: "media-panel",
                onclick: move |event| event.stop_propagation(),
                header { class: "media-panel-header",
                    div {
                        strong { "Mídia compartilhada" }
                        span { class: "hint", "{chat_title}" }
                    }
                    button {
                        class: "btn small ghost",
                        onclick: move |_| on_close.call(()),
                        "Fechar"
                    }
                }
                if let Some(error) = error {
                    p { class: "error-banner", "{error}" }
                }
                if loading {
                    div { class: "timeline-loading",
                        div { class: "spinner" }
                        span { "Carregando mídia..." }
                    }
                } else if items.is_empty() {
                    div { class: "empty-state",
                        div { class: "empty-card",
                            div { class: "empty-icon", "▣" }
                            h2 { "Nenhuma mídia" }
                            p { "Esta conversa ainda não tem fotos ou vídeos em cache no TDLib." }
                        }
                    }
                } else {
                    div { class: "shared-grid",
                        for item in items {
                            div {
                                key: "{item.id}",
                                class: "shared-item",
                                onclick: {
                                    let item = item.clone();
                                    move |_| on_open.call(item.clone())
                                },
                                MessageThumbnail {
                                    chat_id,
                                    message_id: item.id,
                                    is_video: item.is_video,
                                    on_open: {
                                        let item = item.clone();
                                        move |_| on_open.call(item.clone())
                                    },
                                }
                                div { class: "shared-meta",
                                    strong { "{item.sender_name}" }
                                    span {
                                        "{format_timestamp(item.date)}"
                                        if let Some(size) = item.media_size {
                                            " · {format_bytes(size)}"
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
}

#[component]
fn MassDownloadPanel(
    chat: TdlibChatInfo,
    topics: Vec<TdlibForumTopicInfo>,
    on_close: EventHandler<()>,
) -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let default_folder = services
        .env
        .paths()
        .download_dir()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut folder = use_signal(|| default_folder);
    let mut selected_topic_id = use_signal(|| "all".to_string());
    let mut split_by_user = use_signal(|| false);
    let mut split_by_album = use_signal(|| false);
    let mut album_split_mode = use_signal(|| "separator".to_string());

    let topic_options = {
        let mut options = vec![SelectOption::new("all", "Todos os tópicos")];
        options.extend(
            topics
                .iter()
                .map(|topic| SelectOption::new(topic.id.to_string(), topic.title.clone())),
        );
        options
    };
    let album_mode_options = vec![
        SelectOption::new("separator", "Separador"),
        SelectOption::new("comment", "Comentário"),
    ];

    let running = ui.mass_download_running();
    let stopping = ui.mass_download_stopping();
    let progress = ui.mass_download_progress();

    let chat_for_start = chat.clone();
    let topics_for_start = topics.clone();
    let start_services = services.clone();
    let start = move |_| {
        if ui.mass_download_running() {
            return;
        }
        let Some(chat_id) = parse_chat_id(&chat_for_start) else {
            push_toast(
                ui.toasts,
                "Chat inválido para download em massa.",
                ToastKind::Error,
            );
            return;
        };
        let folder_path = folder().trim().to_string();
        if folder_path.is_empty() {
            push_toast(ui.toasts, "Escolha uma pasta de destino.", ToastKind::Error);
            return;
        }
        let topic = topics_for_start
            .iter()
            .find(|topic| topic.id.to_string() == selected_topic_id())
            .cloned();
        let request = TdlibMassDownloadRequest {
            chat_id,
            folder_path,
            topic_id: topic.as_ref().map(|topic| topic.id),
            topic_kind: topic.as_ref().map(|topic| topic.kind.clone()),
            split_by_user: split_by_user(),
            split_by_album: split_by_album(),
            album_split_mode: Some(album_split_mode()),
        };

        ui.mass_download_running.set(true);
        ui.mass_download_stopping.set(false);
        ui.mass_download_progress.set(None);
        let services = start_services.clone();
        spawn(async move {
            let outcome = services.telegram.start_mass_download(request).await;
            match outcome {
                Ok(result) if result.success => {
                    let message = if result.aborted {
                        format!(
                            "Download em massa interrompido: {} baixados, {} ignorados.",
                            result.downloaded_count, result.skipped_count
                        )
                    } else {
                        format!(
                            "Download em massa concluído: {} baixados, {} ignorados, {} falharam.",
                            result.downloaded_count, result.skipped_count, result.failed_count
                        )
                    };
                    push_toast(ui.toasts, message, ToastKind::Success);
                }
                Ok(result) => push_toast(
                    ui.toasts,
                    result
                        .error
                        .unwrap_or_else(|| "Falha no download em massa.".to_string()),
                    ToastKind::Error,
                ),
                Err(error) => push_toast(ui.toasts, error, ToastKind::Error),
            }
            ui.mass_download_running.set(false);
            ui.mass_download_stopping.set(false);
        });
    };

    let stop_services = services.clone();
    let stop = move |_| {
        if !ui.mass_download_running() || ui.mass_download_stopping() {
            return;
        }
        ui.mass_download_stopping.set(true);
        let services = stop_services.clone();
        spawn(async move {
            if let Err(error) = services.telegram.stop_mass_download().await {
                push_toast(ui.toasts, error, ToastKind::Error);
                ui.mass_download_stopping.set(false);
            }
        });
    };

    let choose_folder = move |_| {
        spawn(async move {
            if let Some(path) = pick_download_folder().await {
                folder.set(path.to_string_lossy().to_string());
            }
        });
    };

    let percent = progress
        .as_ref()
        .filter(|progress| !progress.is_scanning && progress.total > 0)
        .map(|progress| (progress.downloaded / progress.total as f64 * 100.0).clamp(0.0, 100.0))
        .unwrap_or(0.0);
    let bar_width = if progress
        .as_ref()
        .is_some_and(|progress| progress.is_scanning)
    {
        100.0
    } else {
        percent
    };

    rsx! {
        div {
            class: "mass-download-backdrop",
            onclick: move |_| on_close.call(()),
            section {
                class: "mass-download-panel",
                onclick: move |event| event.stop_propagation(),
                header { class: "mass-download-header",
                    div {
                        strong { "Download em massa" }
                        span { class: "hint", "{chat.title}" }
                    }
                    button {
                        class: "btn small ghost",
                        onclick: move |_| on_close.call(()),
                        "Fechar"
                    }
                }
                div { class: "mass-download-body",
                    label { class: "mass-download-field",
                        span { "Pasta de destino" }
                        div { class: "mass-download-folder-row",
                            input {
                                class: "download-input",
                                readonly: true,
                                value: "{folder}",
                                placeholder: "Escolha uma pasta...",
                            }
                            button {
                                class: "btn small ghost",
                                disabled: running,
                                onclick: choose_folder,
                                "Procurar"
                            }
                        }
                    }
                    if !topics.is_empty() {
                        label { class: "mass-download-field",
                            span { "Tópico" }
                            Select {
                                value: selected_topic_id(),
                                options: topic_options.clone(),
                                placeholder: "Todos os tópicos",
                                disabled: running,
                                on_change: move |value| selected_topic_id.set(value),
                            }
                        }
                    }
                    div { class: "mass-download-options",
                        label {
                            input {
                                r#type: "checkbox",
                                checked: split_by_user(),
                                disabled: running,
                                onchange: move |event| split_by_user.set(event.checked()),
                            }
                            span { "Dividir por usuário" }
                        }
                        label {
                            input {
                                r#type: "checkbox",
                                checked: split_by_album(),
                                disabled: running,
                                onchange: move |event| split_by_album.set(event.checked()),
                            }
                            span { "Dividir por álbum" }
                        }
                        if split_by_album() {
                            div { class: "mass-download-album-mode",
                                Select {
                                    value: album_split_mode(),
                                    options: album_mode_options.clone(),
                                    placeholder: "Modo",
                                    disabled: running,
                                    on_change: move |value| album_split_mode.set(value),
                                }
                            }
                        }
                    }
                    div { class: "mass-download-actions",
                        if running {
                            button {
                                class: "btn primary",
                                disabled: stopping,
                                onclick: stop,
                                if stopping { "Parando..." } else { "Parar download" }
                            }
                        } else {
                            button {
                                class: "btn primary",
                                disabled: folder().trim().is_empty(),
                                onclick: start,
                                "Iniciar download"
                            }
                        }
                    }
                }
                if let Some(progress) = progress {
                    div { class: "mass-download-progress",
                        div { class: "mass-download-progress-head",
                            span { "{progress.current_file}" }
                            strong {
                                if progress.is_scanning {
                                    "Escaneando..."
                                } else {
                                    "{progress.downloaded.floor()} de {progress.total}"
                                }
                            }
                        }
                        div { class: "download-progress-bar",
                            div {
                                class: if progress.is_scanning { "download-progress-fill scanning" } else { "download-progress-fill" },
                                style: "width: {bar_width}%",
                            }
                        }
                        if !progress.is_scanning && progress.total > 0 {
                            div { class: "mass-download-summary",
                                span { "Baixados: {mass_status_count(&progress, \"completed\")}" }
                                span { "Já existem: {mass_status_count(&progress, \"skipped\")}" }
                                span { "Falharam: {mass_status_count(&progress, \"failed\")}" }
                                span { "Na fila: {mass_status_count(&progress, \"pending\") + mass_status_count(&progress, \"downloading\")}" }
                            }
                        }
                        div { class: "mass-download-list",
                            for item in progress.items.iter().take(500) {
                                div {
                                    key: "{item.message_id}",
                                    class: "mass-download-item {item.status}",
                                    div { class: "mass-download-item-name",
                                        span { "{item.name}" }
                                        if item.size > 0 {
                                            small { "{format_bytes(item.size)}" }
                                        }
                                    }
                                    span { class: "mass-download-item-status",
                                        "{mass_status_label(&item.status)}"
                                        if item.status == "downloading" {
                                            " {item.progress}%"
                                        }
                                    }
                                }
                            }
                            if progress.items.len() > 500 {
                                p { class: "hint",
                                    "Mostrando 500 de {progress.items.len()} arquivos."
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn mass_status_count(progress: &MassDownloadProgress, status: &str) -> usize {
    progress
        .items
        .iter()
        .filter(|item| item.status == status)
        .count()
}

fn mass_status_label(status: &str) -> &str {
    match status {
        "pending" => "Na fila",
        "downloading" => "Baixando",
        "completed" => "Salvo",
        "skipped" => "Já existe",
        "failed" => "Falhou",
        "stopped" => "Interrompido",
        _ => status,
    }
}

#[component]
fn MediaLightbox(state: LightboxState, on_close: EventHandler<()>) -> Element {
    let services = use_context::<AppServices>();
    let toasts = use_context::<UiState>().toasts;
    let save_services = services.clone();
    let resource_state = state.clone();
    let resource = use_resource(move || {
        let services = services.clone();
        let state = resource_state.clone();
        async move {
            services
                .telegram
                .prepare_playback(state.chat_id, state.message_id)
                .await
        }
    });
    let mut saving = use_signal(|| false);
    let mut save_error = use_signal(|| None::<String>);

    let (source, mime_type, load_error) = match &*resource.read_unchecked() {
        Some(Ok(playback)) if playback.success => {
            (playback.url.clone(), playback.mime_type.clone(), None)
        }
        Some(Ok(playback)) => (
            None,
            playback.mime_type.clone(),
            playback
                .error
                .clone()
                .or_else(|| Some("Não foi possível preparar a mídia.".to_string())),
        ),
        Some(Err(error)) => (None, None, Some(error.clone())),
        None => (None, None, None),
    };
    let save_error_message = save_error();

    rsx! {
        div { class: "lightbox-backdrop", onclick: move |_| on_close.call(()),
            div {
                class: "lightbox-card",
                onclick: move |event| event.stop_propagation(),
                header { class: "lightbox-header",
                    div {
                        strong { "{state.title}" }
                        span { class: "hint", "Mensagem {state.message_id}" }
                    }
                    button {
                        class: "btn small ghost",
                        onclick: move |_| on_close.call(()),
                        "Fechar"
                    }
                }
                div { class: "lightbox-stage",
                    if let Some(error) = load_error {
                        p { class: "error-banner", "{error}" }
                    } else if let Some(url) = source {
                        if state.is_video {
                            video {
                                class: "lightbox-media",
                                src: "{url}",
                                controls: true,
                                autoplay: true,
                                playsinline: true,
                            }
                        } else if state.is_photo {
                            img { class: "lightbox-media", src: "{url}", alt: "" }
                        } else {
                            div { class: "lightbox-file",
                                p { "Este anexo não tem prévia no shell Rust." }
                                a { class: "btn small primary", href: "{url}", target: "_blank", "Abrir arquivo" }
                            }
                        }
                    } else {
                        div { class: "timeline-loading",
                            div { class: "spinner" }
                            span { "Preparando mídia..." }
                        }
                    }
                    if let Some(error) = save_error_message {
                        p { class: "error-banner lightbox-save-error", "{error}" }
                    }
                }
                footer { class: "lightbox-footer",
                    span { class: "hint",
                        if let Some(mime) = mime_type.as_deref() {
                            "{mime}"
                        } else {
                            ""
                        }
                    }
                    div { class: "topbar-actions",
                        button {
                            class: "btn primary",
                            disabled: saving(),
                            onclick: {
                                let services = save_services.clone();
                                move |_| {
                                    saving.set(true);
                                    save_error.set(None);
                                    let services = services.clone();
                                    let chat_id = state.chat_id;
                                    let message_id = state.message_id;
                                    spawn(async move {
                                        match services
                                            .telegram
                                            .save_media(chat_id, message_id, None)
                                            .await
                                        {
                                            Ok(result) if result.success => push_toast(
                                                toasts,
                                                format!(
                                                    "Mídia salva em {}.",
                                                    result.file_path.unwrap_or_default()
                                                ),
                                                ToastKind::Success,
                                            ),
                                            Ok(result) => save_error.set(
                                                result.error.or_else(|| {
                                                    Some("Não foi possível salvar a mídia.".into())
                                                }),
                                            ),
                                            Err(error) => save_error.set(Some(error)),
                                        }
                                        saving.set(false);
                                    });
                                }
                            },
                            if saving() { "Salvando..." } else { "Salvar" }
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ForwardDialog(
    state: ForwardState,
    chats: Vec<TdlibChatInfo>,
    busy: bool,
    on_close: EventHandler<()>,
    on_forward: EventHandler<TdlibChatInfo>,
) -> Element {
    rsx! {
        div { class: "forward-backdrop", onclick: move |_| on_close.call(()),
            section {
                class: "modal-card",
                onclick: move |event| event.stop_propagation(),
                header { class: "modal-header",
                    div {
                        strong { "Encaminhar mensagem" }
                        span { class: "hint", "{state.message.sender_name}" }
                    }
                    button {
                        class: "btn small ghost",
                        onclick: move |_| on_close.call(()),
                        "Fechar"
                    }
                }
                p { class: "hint", "{state.message.text}" }
                div { class: "modal-list",
                    for chat in chats {
                        button {
                            key: "{chat.id}",
                            class: "chat-row",
                            disabled: busy,
                            onclick: {
                                let chat = chat.clone();
                                move |_| on_forward.call(chat.clone())
                            },
                            div { class: "chat-avatar", "{chat.title.chars().next().unwrap_or('?')}" }
                            div { class: "chat-copy",
                                span { class: "chat-name", "{chat.title}" }
                                span { class: "chat-preview", "{chat_kind(&chat)}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn send_text(
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
    let Some(chat) = ui.selected_chat() else {
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
    let topic = ui.selected_topic();
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
                composer.set(String::new());
                reply.set(None);
                if let Some(message) = result.message {
                    if ui.selected_chat().map(|chat| chat.id) == Some(expected_chat.clone())
                        && ui.selected_topic().map(|topic| topic.id) == expected_topic
                    {
                        let mut messages = ui.messages();
                        merge_messages(&mut messages, [message.clone()]);
                        ui.messages.set(messages.clone());
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
            Err(send_error) => error.set(Some(send_error)),
        }
        sending.set(false);
    });
}

fn send_media_file(
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
    let Some(chat) = ui.selected_chat() else {
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
    let topic = ui.selected_topic();
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
                composer.set(String::new());
                reply.set(None);
                if let Some(message) = result.message {
                    if ui.selected_chat().map(|chat| chat.id) == Some(expected_chat.clone())
                        && ui.selected_topic().map(|topic| topic.id) == expected_topic
                    {
                        let mut messages = ui.messages();
                        merge_messages(&mut messages, [message.clone()]);
                        ui.messages.set(messages.clone());
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
            Err(send_error) => error.set(Some(send_error)),
        }
        sending.set(false);
    });
}

fn update_chat_preview(ui: UiState, chat_id: &str, message: &TdlibMessageInfo) {
    let mut chats = ui.chats();
    if let Some(chat) = chats.iter_mut().find(|chat| chat.id == chat_id) {
        chat.last_message_text = message.text.clone();
        chat.last_message_date = message.date;
        chat.last_message_has_media = message.has_media;
        chat.last_message_is_video = message.is_video;
        chat.last_message_is_photo = message.is_photo;
    }
    set_if_changed(ui.chats, chats);
    if let Some(selected) = ui.selected_chat() {
        if selected.id == chat_id {
            let mut selected = selected;
            selected.last_message_text = message.text.clone();
            selected.last_message_date = message.date;
            selected.last_message_has_media = message.has_media;
            selected.last_message_is_video = message.is_video;
            selected.last_message_is_photo = message.is_photo;
            set_if_changed(ui.selected_chat, Some(selected));
        }
    }
}

async fn pick_download_folder() -> Option<std::path::PathBuf> {
    #[cfg(not(target_os = "android"))]
    {
        rfd::FileDialog::new()
            .set_title("Escolher pasta para o download em massa")
            .pick_folder()
    }

    #[cfg(target_os = "android")]
    {
        None
    }
}

async fn pick_media_file() -> Option<std::path::PathBuf> {
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

fn format_bytes(bytes: i64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes = bytes.max(0) as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
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
    // Background refreshes (new messages, edits, deletions) must not tear down
    // the timeline: that used to blink the loading state and jump to the
    // newest message. Only a real conversation change clears the page.
    let keep_visible =
        ui.messages_key().as_deref() == Some(cache_key.as_str()) && !ui.messages().is_empty();
    if !keep_visible {
        ui.messages_loading.set(true);
        ui.messages.set(Vec::new());
        ui.messages_has_more.set(false);
    }
    ui.messages_key.set(Some(cache_key.clone()));
    ui.messages_error.set(None);

    let mut cached = Vec::new();
    if !keep_visible {
        if let Ok(cache) = services.telegram.cached_messages(cache_key.clone()).await {
            cached = cache
                .messages
                .iter()
                .filter_map(|value| serde_json::from_value::<TdlibMessageInfo>(value.clone()).ok())
                .collect();
            if !cached.is_empty() && conversation_matches(ui, &chat, topic.as_ref()) {
                set_if_changed(ui.messages, cached.clone());
            }
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
            let mut merged = if keep_visible { ui.messages() } else { cached };
            merge_messages(&mut merged, result.messages);
            set_if_changed(ui.messages, merged.clone());
            set_if_changed(ui.messages_has_more, result.has_more);
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
                set_if_changed(ui.messages, merged.clone());
                set_if_changed(ui.messages_has_more, result.has_more);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_media_sizes_for_shared_media() {
        assert_eq!(format_bytes(900), "900 B");
        assert_eq!(format_bytes(2 * 1024), "2 KB");
        assert_eq!(format_bytes(3 * 1024 * 1024), "3.0 MB");
        assert_eq!(format_bytes(2 * 1024 * 1024 * 1024), "2.0 GB");
        assert_eq!(format_bytes(-1), "0 B");
    }
}
