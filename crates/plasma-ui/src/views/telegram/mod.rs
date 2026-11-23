//! Telegram view: real TDLib chats, topics, search and cache when signed in;
//! a local preview on the same shell before login.

use crate::icons::{Icon, IconName};
use crate::state::DuplicateSearchMedia;
use crate::state::{
    forum_decision, load_hidden_topics, message_cache_key, prepare_forum_view, push_toast,
    recall_cached_topics, recall_forum_flag, refresh_topics_with_new_media, remember_chat_list,
    set_chat_has_topics, set_drawer_open, set_if_changed, AppServices, DrawerTab, ForumDecision,
    ForwardState, LightboxMedia, LightboxState, Tab, ToastKind, UiState,
};
use crate::views::chat_list;
use crate::views::downloads::{DownloadsView, QuickQueueDrawer};
use crate::views::duplicate_search::DuplicateSearchDialog;
use crate::views::global_search::GlobalSearchResults;
use crate::views::settings::SettingsView;
use crate::views::spike::VirtualTimeline;
use crate::views::timeline::{format_timestamp, MessageAction, MessageTimeline};
use crate::views::topics::TopicBrowser;
use dioxus::prelude::*;
use plasma_telegram::{
    TdlibChatInfo, TdlibForumTopicInfo, TdlibGlobalSearchMessage, TdlibMessageInfo,
    TdlibSharedMediaItem,
};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

mod chat_column;
mod composer;
mod conversation;
mod conversation_header;
mod forward;
mod mass_download;
mod media;
mod preview;

use chat_column::ChatListColumn;
use composer::{
    close_drop_media, collect_dropped_media, open_dropped_media_preview, ComposerInput,
    DropMediaDialog, MessageComposer,
};
use conversation::{
    chat_from_global_result, jump_to_message, load_conversation, load_forum_topics, load_older,
    parse_chat_id, run_global_search_more, search_topic_title, SearchWatch,
};
use conversation_header::ConversationHeader;
use forward::ForwardDialog;
use mass_download::MassDownloadPanel;
use media::{MediaLightbox, MediaPanel};
use preview::{PreviewSidebar, PreviewStage};

#[derive(Clone, PartialEq)]
struct PendingSearchJump {
    chat_id: String,
    topic_id: Option<i64>,
    topic_kind: Option<String>,
    message_id: i64,
}

#[derive(Clone, PartialEq)]
struct PendingDroppedMedia {
    path: String,
    name: String,
    is_video: bool,
    preview: Option<String>,
}

struct DroppedMediaSelection {
    files: Vec<PendingDroppedMedia>,
    skipped: usize,
}

#[derive(Clone, Copy, PartialEq)]
struct DropMediaState {
    pending: Signal<Vec<PendingDroppedMedia>>,
    open: Signal<bool>,
    caption: Signal<String>,
    target: Signal<Option<String>>,
    progress: Signal<Option<f32>>,
    error: Signal<Option<String>>,
    cancel: Signal<bool>,
}

impl DropMediaState {
    fn pending(&self) -> Vec<PendingDroppedMedia> {
        (self.pending)()
    }

    fn open(&self) -> bool {
        (self.open)()
    }

    fn caption(&self) -> String {
        (self.caption)()
    }

    fn target(&self) -> Option<String> {
        (self.target)()
    }

    fn progress(&self) -> Option<f32> {
        (self.progress)()
    }

    fn error(&self) -> Option<String> {
        (self.error)()
    }

    fn cancel(&self) -> bool {
        (self.cancel)()
    }
}

/// Cached sender media, keyed by chat and sender. The second tuple element is
/// the chat's `last_message_date` when the list was fetched, so reopening the
/// panel only hits TDLib again when the conversation actually changed.
type UserMediaCache = std::collections::HashMap<(i64, String), (Vec<TdlibSharedMediaItem>, i32)>;
const USER_MEDIA_CACHE_LIMIT: usize = 32;
const USER_MEDIA_PAGE_CAP: usize = 15;
const MAX_DROPPED_MEDIA_FILES: usize = 20;
const MAX_DROPPED_MEDIA_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[component]
pub fn TelegramView() -> Element {
    let ui = use_context::<UiState>();
    if ui.session.auth_state() == "ready" {
        rsx! { RealTelegram {} }
    } else {
        rsx! { PreviewWorkspace {} }
    }
}

#[component]
fn PreviewWorkspace() -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let preview_selected = use_signal(|| 0usize);
    let active_tab = ui.shell.tab();
    let drawer_open = ui.shell.drawer_open();
    let mobile_list = active_tab == Tab::Telegram && ui.conversation.mobile_show_list();
    let shell_class = if mobile_list {
        "workspace-shell mobile-show-list"
    } else {
        "workspace-shell mobile-show-stage"
    };

    rsx! {
        div { class: "{shell_class}",
            PreviewSidebar { selected: preview_selected }
            main { class: "stage-pane",
                match active_tab {
                    Tab::Telegram => rsx! { PreviewStage { selected: preview_selected } },
                    Tab::Downloads | Tab::Twitter => rsx! { DownloadsView {} },
                    Tab::Settings => rsx! { SettingsView {} },
                }
            }
            if drawer_open {
                aside { class: "drawer-pane", aria_label: "Painel lateral",
                    header { class: "drawer-header",
                        div { class: "drawer-tabs",
                            button { class: "drawer-tab active", "Fila" }
                        }
                        button {
                            class: "icon-button small",
                            title: "Fechar gaveta (Ctrl+J)",
                            aria_label: "Fechar gaveta",
                            onclick: move |_| set_drawer_open(&services, ui, false),
                            Icon { name: IconName::Close, size: 15, class: "icon" }
                        }
                    }
                    div { class: "drawer-body",
                        QuickQueueDrawer {}
                    }
                }
            }
            MobileBottomNav {}
        }
    }
}

#[component]
fn MobileBottomNav() -> Element {
    let mut ui = use_context::<UiState>();
    let tab = ui.shell.tab();
    let in_downloads = matches!(tab, Tab::Downloads | Tab::Twitter);

    rsx! {
        nav { class: "mobile-bottom-bar", aria_label: "Navegação móvel",
            button {
                class: if tab == Tab::Telegram { "sidebar-nav-btn active" } else { "sidebar-nav-btn" },
                onclick: move |_| {
                    ui.shell.tab.set(Tab::Telegram);
                    ui.conversation.mobile_show_list.set(true);
                },
                Icon { name: IconName::BrandTelegram, size: 18, class: "icon" }
                span { "Chats" }
            }
            button {
                class: if in_downloads { "sidebar-nav-btn active" } else { "sidebar-nav-btn" },
                onclick: move |_| {
                    ui.shell.tab.set(Tab::Downloads);
                    ui.conversation.mobile_show_list.set(false);
                },
                Icon { name: IconName::ProgressDown, size: 18, class: "icon" }
                span { "Downloads" }
            }
            button {
                class: if tab == Tab::Settings { "sidebar-nav-btn active" } else { "sidebar-nav-btn" },
                onclick: move |_| {
                    ui.shell.tab.set(Tab::Settings);
                    ui.conversation.mobile_show_list.set(false);
                },
                Icon { name: IconName::Settings, size: 18, class: "icon" }
                span { "Ajustes" }
            }
        }
    }
}

#[component]
fn RealTelegram() -> Element {
    let mut ui = use_context::<UiState>();
    let services = use_context::<AppServices>();

    let chat_filter = use_signal(String::new);
    let mut message_query = use_signal(String::new);
    let mut search_active = use_signal(|| false);
    let global_search = use_signal(|| false);
    let mut global_results = use_signal(Vec::<TdlibGlobalSearchMessage>::new);
    let mut global_offset = use_signal(String::new);
    let mut global_loading_more = use_signal(|| false);
    let mut pending_search_jump = use_signal(|| None::<PendingSearchJump>);
    let search_epoch = use_signal(|| 0u64);
    // Plain atomics — Signal/CopyValue peek+set inside this effect re-subscribed
    // and spawned tens of thousands of loads (leases >> 1, UI freeze).
    let conversation_epoch = use_hook(|| Arc::new(AtomicU64::new(0)));
    let conversation_epoch_messages = conversation_epoch.clone();
    let conversation_epoch_jump = conversation_epoch.clone();
    let conversation_epoch_timeline = conversation_epoch.clone();
    let mut loading_older = use_signal(|| false);
    let jump_to = use_signal(|| None::<i64>);
    let mut avatar_chat = use_signal(|| None::<String>);
    let composer = use_signal(String::new);
    let sending = use_signal(|| false);
    let composer_error = use_signal(|| None::<String>);
    let drop = DropMediaState {
        pending: use_signal(Vec::<PendingDroppedMedia>::new),
        open: use_signal(|| false),
        caption: use_signal(String::new),
        target: use_signal(|| None::<String>),
        progress: use_signal(|| None::<f32>),
        error: use_signal(|| None::<String>),
        cancel: use_signal(|| false),
    };
    let dragging_over = use_signal(|| false);
    let mut drag_depth = use_signal(|| 0u32);
    let mut selected_messages = use_signal(Vec::<i64>::new);
    let mut shared_items = use_signal(Vec::<TdlibSharedMediaItem>::new);
    let mut shared_loading = use_signal(|| false);
    let mut shared_error = use_signal(|| None::<String>);
    let mut user_media_items = use_signal(Vec::<TdlibSharedMediaItem>::new);
    let mut user_media_loading = use_signal(|| false);
    let mut user_media_loading_more = use_signal(|| false);
    let mut user_media_error = use_signal(|| None::<String>);
    let mut user_media_key = use_signal(|| None::<(i64, String)>);
    let mut user_media_generation = use_signal(|| 0u64);
    let mut user_media_cache = use_hook(|| CopyValue::new(UserMediaCache::new()));
    let mut forwarding = use_signal(|| false);
    // The chat object also changes when its preview or unread counters update.
    // Effects that only care about switching conversations key off the id.
    let selected_chat_key = use_memo(move || ui.conversation.selected_chat().map(|chat| chat.id));

    // A drop preview belongs to the conversation where it was opened. If the
    // selection changes underneath it, cancel any in-flight batch and discard
    // the stale queue instead of risking a send to the wrong chat.
    let mut drop_chat_state = drop;
    use_effect(move || {
        let current = selected_chat_key();
        if drop_chat_state.open() && drop_chat_state.target() != current {
            drop_chat_state.cancel.set(true);
            close_drop_media(drop_chat_state);
        }
    });

    // Keep keyboard focus inside the drop modal so Escape is handled even when
    // the caption is disabled during an upload.
    let drop_focus_state = drop;
    use_effect(move || {
        if !drop_focus_state.open() {
            return;
        }
        let focus_modal = sending();
        let selector = if focus_modal {
            "#drop-media-modal"
        } else {
            ".drop-media-caption"
        };
        let script = format!(
            r#"
            const modal = document.getElementById("drop-media-modal");
            const target = document.querySelector("{selector}");
            if (target && !target.disabled) {{
                target.focus();
            }} else if (modal) {{
                modal.focus();
            }}
            dioxus.send("focused");
            "#
        );
        spawn(async move {
            let mut eval = document::eval(&script);
            let _ = eval.recv::<String>().await;
        });
    });

    let mut chat_refresh_pending = use_hook(|| CopyValue::new(false));
    let mut chats_full_refresh = use_hook(|| CopyValue::new(false));
    let chat_services = services.clone();
    use_effect(move || {
        let _tick = ui.conversation.telegram_tick();
        // Coalesce bursts: at most one chat-list scan is in flight. New events
        // that arrive during the short delay are covered by the next scan.
        if *chat_refresh_pending.peek() {
            return;
        }
        chat_refresh_pending.set(true);
        let services = chat_services.clone();
        spawn(async move {
            let force_full = *chats_full_refresh.peek();
            chats_full_refresh.set(false);
            let had_cache = !ui.conversation.chats.peek().is_empty();
            // Soft reconcile keeps the open conversation ahead of a full 150
            // dialog scan. ↻ forces the complete Main+Archive pass.
            let soft = !force_full && had_cache;
            let limit = if force_full || !had_cache { 150 } else { 20 };
            if had_cache {
                tokio::time::sleep(std::time::Duration::from_millis(if force_full {
                    180
                } else {
                    500
                }))
                .await;
            }
            // Soft scans wait out an open-chat history load instead of queuing
            // behind it and then stampeding the client when the lease drops.
            if soft {
                for _ in 0..120 {
                    if !*ui.conversation.messages_loading.peek()
                        && !services.telegram.conversation_priority_active()
                    {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
                if *ui.conversation.messages_loading.peek()
                    || services.telegram.conversation_priority_active()
                {
                    chat_refresh_pending.set(false);
                    return;
                }
            }
            // Only the first load shows the placeholder; refreshes keep the list.
            set_if_changed(
                ui.conversation.chats_loading,
                ui.conversation.chats().is_empty(),
            );
            set_if_changed(ui.conversation.chats_error, None::<String>);
            let chats_result = if soft {
                services.telegram.get_chats_soft(limit).await
            } else {
                services.telegram.get_chats(limit).await
            };
            match chats_result {
                Ok(result) if result.success => {
                    let mut dialogs = result.dialogs;
                    crate::twitter_fake::merge_fake_dialogs_into(&mut dialogs, &services.settings);
                    let mut open_first = None;
                    if let Some(pending) =
                        crate::twitter_fake::take_pending_fake_chat(&services.settings)
                    {
                        if let Some(pending_chat) =
                            dialogs.iter().find(|chat| chat.id == pending).cloned()
                        {
                            open_first = Some(pending_chat);
                        }
                    }
                    if let Some(selected) = ui.conversation.selected_chat() {
                        if let Some(updated) =
                            dialogs.iter().find(|chat| chat.id == selected.id).cloned()
                        {
                            set_if_changed(ui.conversation.selected_chat, Some(updated));
                        }
                    } else if open_first.is_none() {
                        if let Some(first) = dialogs
                            .iter()
                            .find(|chat| !crate::twitter_fake::is_twitter_fake_chat_id(&chat.id))
                            .cloned()
                            .or_else(|| dialogs.first().cloned())
                        {
                            open_first = Some(first);
                        }
                    }
                    // Soft refresh only re-resolves the top of the Main list.
                    // Keep cached rows that were outside that window so the
                    // sidebar does not shrink after every auth tick.
                    let painted = if force_full || !had_cache {
                        dialogs
                    } else {
                        let mut seen: HashSet<String> =
                            dialogs.iter().map(|chat| chat.id.clone()).collect();
                        let mut merged = dialogs;
                        for chat in ui.conversation.chats.peek().iter() {
                            if seen.insert(chat.id.clone()) {
                                merged.push(chat.clone());
                            }
                        }
                        crate::twitter_fake::merge_fake_dialogs_into(
                            &mut merged,
                            &services.settings,
                        );
                        merged
                    };
                    // Paint the list and return to the window before opening
                    // the first conversation. History, topics and read-state
                    // all share the same TDLib client.
                    set_if_changed(ui.conversation.chats, painted.clone());
                    let _ = remember_chat_list(&services.settings, &painted);
                    set_if_changed(ui.conversation.chats_loading, false);
                    tokio::task::yield_now().await;
                    if let Some(first) = open_first {
                        let last_message_id = first.last_message_id;
                        let unread_hint = first.unread_count;
                        ui.conversation.selected_chat.set(Some(first));
                        let services = services.clone();
                        spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                            let Some(chat_id) = ui
                                .conversation
                                .selected_chat
                                .peek()
                                .as_ref()
                                .and_then(parse_chat_id)
                            else {
                                return;
                            };
                            let _ = services
                                .telegram
                                .read_chat(chat_id, last_message_id, Some(unread_hint))
                                .await;
                        });
                    }
                }
                Ok(result) => {
                    set_if_changed(
                        ui.conversation.chats_error,
                        result
                            .error
                            .or_else(|| Some("TDLib não retornou conversas.".into())),
                    );
                }
                Err(error) => {
                    set_if_changed(ui.conversation.chats_error, Some(error.to_string()));
                }
            }
            set_if_changed(ui.conversation.chats_loading, false);
            chat_refresh_pending.set(false);
        });
    });

    let topic_services = services.clone();
    // Chat currently painted, and the chat whose topic request is in flight.
    // A repeat tick for the same chat must not wipe the cached list.
    let mut topics_chat = use_hook(|| CopyValue::new(None::<String>));
    let mut topics_fetch = use_hook(|| CopyValue::new(None::<String>));
    // Session memory of the forum probe, so a plain group is not classified
    // again before the SQLite write lands.
    let mut forum_flags = use_hook(|| CopyValue::new(HashMap::<String, bool>::new()));
    let forum_selection = use_memo(move || {
        ui.conversation.selected_chat().map(|chat| {
            (
                chat.id.clone(),
                chat.has_topics,
                chat.is_group || chat.is_channel,
            )
        })
    });
    use_effect(move || {
        // Topics are loaded when the conversation changes. Live messages update
        // unread counts in place, so a tick must not page the topic list again.
        let Some((chat_id_key, has_topics, is_groupish)) = forum_selection() else {
            topics_chat.set(None);
            ui.conversation.topics.set(Vec::new());
            ui.conversation.selected_topic.set(None);
            return;
        };
        if crate::twitter_fake::is_twitter_fake_chat_id(&chat_id_key) {
            topics_chat.set(Some(chat_id_key));
            ui.conversation.topics.set(Vec::new());
            ui.conversation.selected_topic.set(None);
            return;
        }
        let Some(mut chat) = ui.conversation.selected_chat.peek().as_ref().cloned() else {
            return;
        };
        if chat.id != chat_id_key {
            return;
        }
        let Some(chat_id) = parse_chat_id(&chat) else {
            return;
        };
        let chat_changed = topics_chat.peek().as_deref() != Some(chat_id_key.as_str());
        if chat_changed {
            topics_chat.set(Some(chat_id_key.clone()));
            prepare_forum_view(&topic_services, ui, &mut chat);
            if chat.has_topics != has_topics {
                set_chat_has_topics(ui, &chat_id_key, chat.has_topics);
            }
        }
        let stored_flag = forum_flags
            .peek()
            .get(&chat_id_key)
            .copied()
            .or_else(|| recall_forum_flag(&topic_services, chat_id));
        if let Some(flag) = stored_flag {
            if !forum_flags.peek().contains_key(&chat_id_key) {
                let mut flags = forum_flags.peek().clone();
                flags.insert(chat_id_key.clone(), flag);
                forum_flags.set(flags);
            }
        }
        let decision = forum_decision(
            chat.has_topics || has_topics,
            is_groupish,
            recall_cached_topics(&topic_services, ui, &chat_id_key).is_some(),
            stored_flag,
        );
        if matches!(decision, ForumDecision::Plain) {
            if chat_changed {
                ui.conversation.topics.set(Vec::new());
                ui.conversation.selected_topic.set(None);
            }
            return;
        }
        // Cache already painted the chips. Re-paging 200 topics on every open
        // competed with history; a later ShowLoading/Probe still refreshes.
        if matches!(decision, ForumDecision::ShowCached) {
            return;
        }
        // Ignore ticks until this chat's refresh finishes, otherwise a second
        // call stacks behind it.
        if topics_fetch.peek().as_deref() == Some(chat_id_key.as_str()) {
            return;
        }
        topics_fetch.set(Some(chat_id_key.clone()));
        let known_forum = matches!(decision, ForumDecision::ShowLoading);
        load_forum_topics(
            topic_services.clone(),
            ui,
            chat_id_key,
            chat_id,
            known_forum,
            topics_fetch,
            forum_flags,
        );
    });

    // Hidden topics are tracked per chat; load them when the conversation
    // changes instead of on every Telegram tick.
    let mut hidden_topics_chat = use_hook(|| CopyValue::new(None::<i64>));
    let hidden_topics_services = services.clone();
    use_effect(move || {
        let chat_id = ui
            .conversation
            .selected_chat()
            .and_then(|chat| parse_chat_id(&chat));
        if hidden_topics_chat() == chat_id {
            return;
        }
        hidden_topics_chat.set(chat_id);
        match chat_id {
            Some(chat_id) => load_hidden_topics(&hidden_topics_services, ui, chat_id),
            None => ui.conversation.hidden_topics.set(Vec::new()),
        }
    });

    // After topics are known, check which ones gained media since the last mass download.
    let new_media_services = services.clone();
    let mut new_media_chat = use_hook(|| CopyValue::new(None::<i64>));
    use_effect(move || {
        let chat = ui.conversation.selected_chat();
        let topics = ui.conversation.topics();
        let Some(chat) = chat else {
            new_media_chat.set(None);
            ui.conversation
                .topics_with_new_media
                .set(std::collections::HashSet::new());
            return;
        };
        if crate::twitter_fake::is_twitter_fake_chat_id(&chat.id) || topics.is_empty() {
            new_media_chat.set(None);
            ui.conversation
                .topics_with_new_media
                .set(std::collections::HashSet::new());
            return;
        }
        let Some(chat_id) = parse_chat_id(&chat) else {
            return;
        };
        if new_media_chat() == Some(chat_id) {
            return;
        }
        new_media_chat.set(Some(chat_id));
        refresh_topics_with_new_media(new_media_services.clone(), ui, chat_id);
    });

    let message_services = services.clone();
    // Deduplicate reactive re-fires that repeat the same chat/topic/tick.
    let mut last_message_load_key =
        use_hook(|| CopyValue::new(None::<(Option<String>, Option<(i64, String)>, u64)>));
    use_effect(move || {
        let selected_key = selected_chat_key();
        let topic = ui.conversation.selected_topic();
        let tick = ui.conversation.message_tick();
        let topic_key = topic.as_ref().map(|item| (item.id, item.kind.clone()));
        let load_key = (selected_key.clone(), topic_key, tick);
        if *last_message_load_key.peek() == Some(load_key.clone()) {
            return;
        }
        last_message_load_key.set(Some(load_key));
        let Some(chat) = ui.conversation.selected_chat.peek().as_ref().cloned() else {
            ui.conversation.messages.set(Vec::new());
            ui.conversation.messages_loading.set(false);
            ui.conversation.messages_has_more.set(false);
            ui.conversation.messages_key.set(None);
            return;
        };
        if selected_key.is_none() {
            ui.conversation.messages.set(Vec::new());
            ui.conversation.messages_loading.set(false);
            ui.conversation.messages_has_more.set(false);
            ui.conversation.messages_key.set(None);
            return;
        }
        let services = message_services.clone();
        loading_older.set(false);
        let epoch = conversation_epoch_messages.clone();
        let generation = epoch.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
        spawn(async move {
            load_conversation(services, ui, chat, topic, epoch, generation).await;
        });
    });

    // A global search result needs the normal conversation load to finish
    // before the timeline can scroll to the target. Wait for the matching
    // cache key, then load the surrounding context when necessary.
    let pending_jump_services = services.clone();
    use_effect(move || {
        let Some(pending) = pending_search_jump() else {
            return;
        };
        let Some(chat) = ui.conversation.selected_chat() else {
            return;
        };
        if chat.id != pending.chat_id {
            return;
        }
        let topic = ui.conversation.selected_topic();
        let topic_matches = match pending.topic_id {
            Some(topic_id) => topic.as_ref().is_some_and(|item| {
                item.id == topic_id
                    && pending
                        .topic_kind
                        .as_ref()
                        .is_none_or(|kind| item.kind == *kind)
            }),
            None => topic.is_none(),
        };
        if !topic_matches {
            return;
        }
        let cache_key = message_cache_key(&chat.id, topic.as_ref());
        if ui.conversation.messages_loading()
            || ui.conversation.messages_key().as_deref() != Some(cache_key.as_str())
            || loading_older()
        {
            return;
        }
        pending_search_jump.set(None);
        let services = pending_jump_services.clone();
        jump_to_message(
            services,
            ui,
            pending.message_id,
            jump_to,
            false,
            loading_older,
            conversation_epoch_jump.clone(),
        );
    });

    let avatar_services = services.clone();
    use_effect(move || {
        let current = ui.conversation.selected_chat().map(|chat| chat.id);
        if current == avatar_chat() {
            return;
        }
        avatar_chat.set(current.clone());
        let Some(chat_id) = current.as_deref().and_then(|id| id.parse::<i64>().ok()) else {
            ui.session.avatar.set(None);
            return;
        };
        let expected = current.unwrap_or_default();
        ui.session.avatar.set(None);
        let services = avatar_services.clone();
        spawn(async move {
            let _ = services.telegram.ensure_chat_avatar(chat_id).await;
            if ui.conversation.selected_chat().map(|item| item.id) != Some(expected.clone()) {
                return;
            }
            if let Some(url) = services.telegram.chat_avatar_url(chat_id) {
                ui.session.avatar.set(Some(url));
            }
        });
    });

    let shared_services = services.clone();
    use_effect(move || {
        let wants_shared = ui.shell.drawer_open()
            && ui.shell.drawer_tab() == DrawerTab::Media
            && ui.panels.user_media().is_none();
        if !wants_shared {
            return;
        }
        let Some(chat_id) = ui
            .conversation
            .selected_chat()
            .and_then(|chat| parse_chat_id(&chat))
        else {
            shared_items.set(Vec::new());
            return;
        };
        let services = shared_services.clone();
        shared_loading.set(true);
        shared_error.set(None);
        spawn(async move {
            let result = services.telegram.shared_media(chat_id, 80).await;
            let still_here = ui.shell.drawer_open()
                && ui.shell.drawer_tab() == DrawerTab::Media
                && ui.panels.user_media().is_none()
                && ui
                    .conversation
                    .selected_chat()
                    .and_then(|chat| parse_chat_id(&chat))
                    == Some(chat_id);
            if !still_here {
                shared_loading.set(false);
                return;
            }
            match result {
                Ok(result) if result.success => shared_items.set(result.media),
                Ok(result) => shared_error.set(
                    result
                        .error
                        .or_else(|| Some("Mídia compartilhada indisponível.".into())),
                ),
                Err(error) => shared_error.set(Some(error.to_string())),
            }
            shared_loading.set(false);
        });
    });

    // The sender context menu opens the user media panel. Results are cached
    // per chat/sender so reopening the panel does not scan the history again;
    // a newer `last_message_date` invalidates the cached list.
    let user_media_services = services.clone();
    use_effect(move || {
        let Some(target) = ui.panels.user_media() else {
            return;
        };
        let key = (target.chat_id, target.sender_id.clone());
        let generation = user_media_generation.peek().wrapping_add(1);
        user_media_generation.set(generation);
        let version = ui
            .conversation
            .selected_chat
            .peek()
            .as_ref()
            .map(|chat| chat.last_message_date)
            .unwrap_or_default();
        let cached = user_media_cache.read().get(&key).cloned();

        // Never show another sender's list while the first request runs.
        let is_new_target = user_media_key.peek().as_ref() != Some(&key);
        if is_new_target {
            user_media_key.set(Some(key.clone()));
            user_media_items.set(Vec::new());
            user_media_loading.set(true);
            user_media_loading_more.set(false);
            user_media_error.set(None);
        }

        if let Some((items, cached_version)) = cached {
            user_media_items.set(items);
            user_media_loading.set(false);
            user_media_loading_more.set(false);
            user_media_error.set(None);
            if cached_version == version {
                return;
            }
        } else if is_new_target {
            user_media_items.set(Vec::new());
            user_media_loading.set(true);
            user_media_loading_more.set(false);
            user_media_error.set(None);
        }

        let services = user_media_services.clone();
        let expected = target.clone();
        spawn(async move {
            let is_current = move || {
                ui.panels.user_media().as_ref() == Some(&expected)
                    && user_media_generation() == generation
            };
            // Fetch the first page fast, then keep appending pages in the
            // background. The panel renders whatever has arrived so far
            // instead of waiting for the whole history to be scanned.
            let mut cursor = None;
            let mut collected: Vec<TdlibSharedMediaItem> = Vec::new();
            let mut seen_ids = HashSet::new();
            let mut has_more = true;
            let mut pages = 0usize;
            let mut first_media_rendered = false;
            let mut failed = false;
            let mut complete = false;

            while has_more {
                let page = services
                    .telegram
                    .user_media(target.chat_id, target.sender_id.clone(), 30, cursor)
                    .await;
                match page {
                    Ok(page) if page.success => {
                        if !is_current() {
                            return;
                        }
                        for item in page.media {
                            if seen_ids.insert(item.id) {
                                collected.push(item);
                            }
                        }
                        pages += 1;
                        let next_cursor = page.next_from_message_id;
                        has_more = page.has_more
                            && next_cursor != 0
                            && Some(next_cursor) != cursor
                            && pages < USER_MEDIA_PAGE_CAP;
                        cursor = Some(next_cursor);
                        if !collected.is_empty() {
                            user_media_items.set(collected.clone());
                            if !first_media_rendered {
                                first_media_rendered = true;
                                user_media_loading.set(false);
                            }
                            user_media_loading_more.set(has_more);
                        }
                        if !has_more {
                            complete = true;
                            break;
                        }
                    }
                    Ok(page) => {
                        if is_current() {
                            user_media_error.set(
                                page.error
                                    .or_else(|| Some("Mídias do usuário indisponíveis.".into())),
                            );
                            user_media_loading.set(false);
                            user_media_loading_more.set(false);
                        }
                        failed = true;
                        break;
                    }
                    Err(error) => {
                        if is_current() {
                            user_media_error.set(Some(error.to_string()));
                            user_media_loading.set(false);
                            user_media_loading_more.set(false);
                        }
                        failed = true;
                        break;
                    }
                }
            }

            if !is_current() {
                return;
            }

            // Only complete lists are cached; an interrupted progressive load
            // will simply restart from the newest page next time.
            if complete && !failed {
                let mut cache = user_media_cache.write();
                if cache.len() >= USER_MEDIA_CACHE_LIMIT {
                    let drop_count = cache.len() / 2;
                    let victims: Vec<_> = cache.keys().take(drop_count).cloned().collect();
                    for key in victims {
                        cache.remove(&key);
                    }
                }
                cache.insert(key.clone(), (collected.clone(), version));
            }
            user_media_items.set(collected);
            user_media_loading.set(false);
            user_media_loading_more.set(false);
            if !failed {
                user_media_error.set(None);
            }
        });
    });

    let selected = ui.conversation.selected_chat();
    let selected_id = selected.as_ref().map(|chat| chat.id.clone());
    let timeline_chat_id = selected
        .as_ref()
        .and_then(parse_chat_id)
        .unwrap_or_default();
    let topic = ui.conversation.selected_topic();
    let topics = ui.conversation.topics();
    let in_search = search_active();
    let is_global_search = in_search && global_search();
    let display_messages = if in_search && !global_search() {
        ui.conversation.search_results()
    } else {
        ui.conversation.messages()
    };
    let global_search_results = global_results();
    let active_loading = if in_search {
        ui.conversation.search_loading()
    } else {
        ui.conversation.messages_loading()
    };
    let active_error = if in_search {
        ui.conversation.search_error()
    } else {
        ui.conversation.messages_error()
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
    let topic_closed = topic.as_ref().is_some_and(|topic| topic.is_closed);
    let drop_preview_services = services.clone();

    let on_message_action = move |(message, action): (TdlibMessageInfo, MessageAction)| match action
    {
        MessageAction::Reply => ui.panels.reply_to.set(Some(message)),
        MessageAction::OpenMedia => ui.panels.lightbox.set(Some(LightboxState::from_message(
            timeline_chat_id,
            &message,
        ))),
        MessageAction::OpenAlbumMedia(album) => ui.panels.lightbox.set(Some(
            LightboxState::from_album(timeline_chat_id, &message, album),
        )),
        MessageAction::Forward => {
            if timeline_chat_id != 0 {
                ui.panels.forward_state.set(Some(ForwardState {
                    source_chat_id: timeline_chat_id,
                    message,
                }));
            }
        }
    };

    let forward = ui.panels.forward_state();
    let lightbox_state = ui.panels.lightbox();
    // The panel only renders items belonging to the current sender. Until the
    // effect catches up with a newly opened panel, it shows the skeleton.
    let user_media_target = ui.panels.user_media();
    let user_media_matches = user_media_target.as_ref().is_some_and(|target| {
        user_media_key().as_ref() == Some(&(target.chat_id, target.sender_id.clone()))
    });
    let user_media_panel_items = if user_media_matches {
        user_media_items()
    } else {
        Vec::new()
    };
    let user_media_panel_loading = if user_media_matches {
        user_media_loading()
    } else {
        true
    };
    let user_media_panel_error = if user_media_matches {
        user_media_error()
    } else {
        None
    };
    let selection_duplicate_items: Vec<DuplicateSearchMedia> = display_messages
        .iter()
        .filter(|message| selected_messages().contains(&message.id) && message.has_media)
        .map(|message| {
            let mut item = DuplicateSearchMedia::from_message(timeline_chat_id, message);
            item.name = format!(
                "{} · {}",
                if message.is_video {
                    "Vídeo"
                } else if message.is_photo {
                    "Foto"
                } else {
                    "Mídia"
                },
                format_timestamp(message.date)
            );
            item
        })
        .collect();
    let duplicate_search_items = ui.transfers.duplicate_search();
    let mass_chat = selected.clone();
    let mass_topics = topics.clone();
    let browser_chat = selected.clone();
    let active_tab = ui.shell.tab();
    let drawer_open = ui.shell.drawer_open();
    let drawer_tab = ui.shell.drawer_tab();
    let mass_initial_topic = ui.transfers.mass_download_topic().or_else(|| topic.clone());
    let drop_target_chat = selected_id.clone();
    let mobile_list =
        active_tab == Tab::Telegram && (selected.is_none() || ui.conversation.mobile_show_list());
    let layout_class = if mobile_list {
        "workspace-shell mobile-show-list"
    } else {
        "workspace-shell mobile-show-stage"
    };

    rsx! {
        div { class: "{layout_class}",
            ChatListColumn {
                chat_filter,
                search_active,
                message_query,
                global_results,
                global_offset,
                global_loading_more,
                pending_search_jump,
                search_epoch,
                chats_full_refresh,
            }

            main { class: "stage-pane",
                match active_tab {
                    Tab::Downloads | Tab::Twitter => rsx! { DownloadsView {} },
                    Tab::Settings => rsx! { SettingsView {} },
                    Tab::Telegram => rsx! {
                        div {
                            class: if dragging_over() { "chat-main dragging-over" } else { "chat-main" },
                            ondragenter: move |event: Event<DragData>| {
                                event.prevent_default();
                                if !media_disabled && !sending() {
                                    drag_depth.set(drag_depth().saturating_add(1));
                                    set_if_changed(dragging_over, true);
                                }
                            },
                            ondragover: move |event: Event<DragData>| {
                                event.prevent_default();
                                if !media_disabled && !sending() {
                                    set_if_changed(dragging_over, true);
                                }
                            },
                            ondragleave: move |_event: Event<DragData>| {
                                let next = drag_depth().saturating_sub(1);
                                drag_depth.set(next);
                                if next == 0 {
                                    set_if_changed(dragging_over, false);
                                }
                            },
                            ondrop: move |event: Event<DragData>| {
                                event.prevent_default();
                                drag_depth.set(0);
                                set_if_changed(dragging_over, false);
                                if sending() {
                                    push_toast(
                                        ui.shell.toasts,
                                        "Aguarde o envio atual terminar.",
                                        ToastKind::Info,
                                    );
                                    return;
                                }
                                if media_disabled {
                                    push_toast(
                                        ui.shell.toasts,
                                        "Envio de mídia indisponível nesta conversa.",
                                        ToastKind::Error,
                                    );
                                    return;
                                }
                                let Some(target_chat) = drop_target_chat.clone() else {
                                    push_toast(
                                        ui.shell.toasts,
                                        "Escolha uma conversa para enviar mídias.",
                                        ToastKind::Error,
                                    );
                                    return;
                                };
                                let selection = collect_dropped_media(&event.data());
                                if selection.files.is_empty() {
                                    push_toast(
                                        ui.shell.toasts,
                                        if selection.skipped > 0 {
                                            format!(
                                                "Nenhum arquivo compatível. Use fotos/vídeos de até 2 GB (máx. {}).",
                                                MAX_DROPPED_MEDIA_FILES
                                            )
                                        } else {
                                            "Arraste fotos ou vídeos para enviar.".to_string()
                                        },
                                        ToastKind::Error,
                                    );
                                    return;
                                }
                                if selection.skipped > 0 {
                                    push_toast(
                                        ui.shell.toasts,
                                        format!(
                                            "{} arquivo(s) ignorado(s) por tipo, tamanho ou limite de {}.",
                                            selection.skipped, MAX_DROPPED_MEDIA_FILES
                                        ),
                                        ToastKind::Info,
                                    );
                                }
                                open_dropped_media_preview(
                                    drop_preview_services.clone(),
                                    drop,
                                    target_chat,
                                    selection.files,
                                );
                            },
                            if ui.shell.show_spike() {
                                VirtualTimeline {}
                            } else if let Some(chat) = selected.clone() {
                                ConversationHeader {
                                    chat: chat.clone(),
                                    topic: topic.clone(),
                                    timeline_chat_id,
                                    in_search,
                                    global_search,
                                    search_active,
                                    message_query,
                                    global_results,
                                    global_offset,
                                    global_loading_more,
                                    search_epoch,
                                }

                                div { class: "chat-body",
                                    if let Some(error) = active_error {
                                        div { class: "error-banner timeline-error", "{error}" }
                                    }
                                    if is_global_search {
                                        if active_loading && global_search_results.is_empty() {
                                            div { class: "timeline-loading",
                                                div { class: "spinner" }
                                                span { "Buscando mensagens…" }
                                            }
                                        } else {
                                            GlobalSearchResults {
                                                query: message_query(),
                                                results: global_search_results,
                                                has_more: !global_offset().is_empty(),
                                                loading_more: global_loading_more(),
                                                on_open: {
                                                    let services = services.clone();
                                                    move |item: TdlibGlobalSearchMessage| {
                                                        let services = services.clone();
                                                        let chat_id = item.chat_id.clone();
                                                        let topic_id = item.message.topic_id;
                                                        let topic_kind = item.message.topic_kind.clone();
                                                        let message_id = item.message.id;
                                                        search_active.set(false);
                                                        message_query.set(String::new());
                                                        global_results.set(Vec::new());
                                                        global_offset.set(String::new());
                                                        global_loading_more.set(false);
                                                        ui.conversation.search_error.set(None);
                                                        ui.conversation.search_loading.set(false);
                                                        spawn(async move {
                                                            let chat = match chat_id.parse::<i64>() {
                                                                Ok(id) => services
                                                                    .telegram
                                                                    .get_chat(id)
                                                                    .await
                                                                    .unwrap_or_else(|_| {
                                                                        chat_from_global_result(&item)
                                                                    }),
                                                                Err(_) => chat_from_global_result(&item),
                                                            };
                                                            chat_list::open_chat(services.clone(), ui, chat);
                                                            let topic = topic_id.map(|id| {
                                                                let kind = topic_kind
                                                                    .clone()
                                                                    .unwrap_or_else(|| "forum".to_string());
                                                                TdlibForumTopicInfo {
                                                                    id,
                                                                    title: search_topic_title(&kind).to_string(),
                                                                    kind,
                                                                    is_closed: false,
                                                                    is_pinned: false,
                                                                    unread_count: 0,
                                                                }
                                                            });
                                                            ui.conversation.selected_topic.set(topic);
                                                            pending_search_jump.set(Some(PendingSearchJump {
                                                                chat_id,
                                                                topic_id,
                                                                topic_kind,
                                                                message_id,
                                                            }));
                                                        });
                                                    }
                                                },
                                                on_load_more: {
                                                    let services = services.clone();
                                                    move |_| {
                                                        run_global_search_more(
                                                            services.clone(),
                                                            ui,
                                                            global_results,
                                                            global_offset,
                                                            global_loading_more,
                                                            message_query(),
                                                            SearchWatch {
                                                                query: message_query,
                                                                epoch: search_epoch,
                                                                global: global_search,
                                                            },
                                                        );
                                                    }
                                                },
                                            }
                                        }
                                    } else if active_loading && display_messages.is_empty() {
                                        div { class: "timeline-loading",
                                            div { class: "spinner" }
                                            span {
                                                if in_search { "Buscando mensagens…" } else { "Carregando mensagens…" }
                                            }
                                        }
                                    } else {
                                        div { class: "timeline-shell",
                                            if !in_search && ui.conversation.messages_has_more() {
                                                div { class: "timeline-toolbar",
                                                    button {
                                                        class: "btn small soft",
                                                        disabled: loading_older(),
                                                        onclick: {
                                                            let services = services.clone();
                                                            move |_| {
                                                                let Some(chat) = ui.conversation.selected_chat() else { return; };
                                                                load_older(
                                                                    services.clone(),
                                                                    ui,
                                                                    chat,
                                                                    ui.conversation.selected_topic(),
                                                                    loading_older,
                                                                );
                                                            }
                                                        },
                                                        if loading_older() { "Carregando…" } else { "Carregar mensagens antigas" }
                                                    }
                                                    span { class: "hint", "{display_messages.len()} mensagens" }
                                                }
                                            }
                                            if in_search {
                                                div { class: "timeline-search-label",
                                                    span { class: "pill accent", "Busca: {message_query}" }
                                                    span { class: "hint", "{display_messages.len()} resultados" }
                                                }
                                            }
                                            div { class: "timeline-fill",
                                                MessageTimeline {
                                                    chat_id: timeline_chat_id,
                                                    messages: ui.conversation.messages,
                                                    search_results: ui.conversation.search_results,
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
                                                    jump_to,
                                                    on_jump_to_message: {
                                                        let services = services.clone();
                                                        let conversation_epoch =
                                                            conversation_epoch_timeline.clone();
                                                        move |message_id| {
                                                            jump_to_message(
                                                                services.clone(),
                                                                ui,
                                                                message_id,
                                                                jump_to,
                                                                search_active(),
                                                                loading_older,
                                                                conversation_epoch.clone(),
                                                            );
                                                        }
                                                    },
                                                    on_reach_start: {
                                                        let services = services.clone();
                                                        move |_| {
                                                            if in_search
                                                                || !ui.conversation.messages_has_more()
                                                                || loading_older()
                                                            {
                                                                return;
                                                            }
                                                            let Some(chat) = ui.conversation.selected_chat() else { return; };
                                                            load_older(
                                                                services.clone(),
                                                                ui,
                                                                chat,
                                                                ui.conversation.selected_topic(),
                                                                loading_older,
                                                            );
                                                        }
                                                    },
                                                }
                                            }
                                        }
                                    }
                                }

                                if !selected_messages().is_empty() {
                                    div { class: "selection-bar",
                                        div { class: "selection-bar-copy",
                                            span { class: "pill accent", "{selected_messages().len()} selecionada(s)" }
                                            span { class: "hint", "Mídias marcadas na conversa." }
                                        }
                                        div { class: "selection-bar-actions",
                                            button {
                                                class: "btn small primary",
                                                disabled: selection_duplicate_items.is_empty()
                                                    || cfg!(target_os = "android"),
                                                onclick: {
                                                    let items = selection_duplicate_items.clone();
                                                    move |_| {
                                                        if cfg!(target_os = "android") {
                                                            return;
                                                        }
                                                        ui.transfers.duplicate_search.set(Some(items.clone()));
                                                    }
                                                },
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

                                MessageComposer {
                                    input: ComposerInput {
                                        text: composer,
                                        sending,
                                        error: composer_error,
                                    },
                                    disabled: composer_disabled,
                                    media_disabled,
                                    topic_closed,
                                }
                            } else {
                                div { class: "empty-state",
                                    div { class: "empty-card",
                                        div { class: "empty-icon",
                                            Icon { name: IconName::BrandTelegram, size: 24, class: "icon" }
                                        }
                                        h2 { "Selecione uma conversa" }
                                        p { "Escolha qualquer conversa ou galeria à esquerda para ler mensagens e mídias." }
                                    }
                                }
                            }
                        }
                    },
                }
            }

            if drawer_open {
                aside { class: "drawer-pane", aria_label: "Painel contextual",
                    header { class: "drawer-header",
                        div { class: "drawer-tabs", role: "tablist",
                            for item in [
                                DrawerTab::Queue,
                                DrawerTab::Media,
                                DrawerTab::Topics,
                                DrawerTab::MassDownload,
                            ] {
                                button {
                                    key: "{item.label()}",
                                    role: "tab",
                                    aria_selected: "{drawer_tab == item}",
                                    class: if drawer_tab == item { "drawer-tab active" } else { "drawer-tab" },
                                    onclick: move |_| ui.shell.drawer_tab.set(item),
                                    "{item.label()}"
                                }
                            }
                        }
                        button {
                            class: "icon-button small",
                            title: "Fechar gaveta (Ctrl+J)",
                            aria_label: "Fechar gaveta",
                            onclick: {
                                let services = services.clone();
                                move |_| set_drawer_open(&services, ui, false)
                            },
                            Icon { name: IconName::Close, size: 15, class: "icon" }
                        }
                    }

                    div { class: "drawer-body",
                        match drawer_tab {
                            DrawerTab::Queue => rsx! { QuickQueueDrawer {} },
                            DrawerTab::Media => {
                                if let Some(target) = user_media_target {
                                    rsx! {
                                        MediaPanel {
                                            panel_id: "user-media-panel".to_string(),
                                            chat_id: target.chat_id,
                                            title: "Mídias enviadas".to_string(),
                                            subtitle: format!("por {}", target.display_name()),
                                            items: user_media_panel_items,
                                            loading: user_media_panel_loading,
                                            error: user_media_panel_error,
                                            empty_title: "Nenhuma mídia".to_string(),
                                            empty_message:
                                                "Não foram encontradas fotos ou vídeos deste usuário."
                                                    .to_string(),
                                            group_albums: true,
                                            bulk_download: true,
                                            bulk_disabled: user_media_loading_more(),
                                            on_back_to_all: Some(EventHandler::new(move |_| {
                                                ui.panels.user_media.set(None);
                                            })),
                                            on_close: {
                                                let services = services.clone();
                                                move |_| set_drawer_open(&services, ui, false)
                                            },
                                            on_open: {
                                                let target = target.clone();
                                                move |(item, album): (TdlibSharedMediaItem, Vec<LightboxMedia>)| {
                                                    let title = if item.sender_name.trim().is_empty() {
                                                        target.display_name().to_string()
                                                    } else {
                                                        item.sender_name.clone()
                                                    };
                                                    ui.panels.lightbox.set(Some(LightboxState::from_shared(
                                                        target.chat_id,
                                                        &item,
                                                        title,
                                                        album,
                                                    )));
                                                }
                                            },
                                        }
                                    }
                                } else if selected.is_some() {
                                    rsx! {
                                        MediaPanel {
                                            panel_id: "shared-media-panel".to_string(),
                                            chat_id: timeline_chat_id,
                                            title: "Mídias da conversa".to_string(),
                                            subtitle: selected_id
                                                .as_deref()
                                                .and_then(|id| ui.conversation.chats().into_iter().find(|chat| chat.id == id))
                                                .map(|chat| chat.title)
                                                .unwrap_or_else(|| "Conversa".to_string()),
                                            items: shared_items(),
                                            loading: shared_loading(),
                                            error: shared_error(),
                                            empty_title: "Nenhuma mídia".to_string(),
                                            empty_message:
                                                "Esta conversa ainda não tem fotos ou vídeos disponíveis."
                                                    .to_string(),
                                            group_albums: true,
                                            bulk_download: true,
                                            bulk_disabled: false,
                                            on_back_to_all: None,
                                            on_close: {
                                                let services = services.clone();
                                                move |_| set_drawer_open(&services, ui, false)
                                            },
                                            on_open: move |(item, album): (TdlibSharedMediaItem, Vec<LightboxMedia>)| {
                                                ui.panels.lightbox.set(Some(LightboxState::from_shared(
                                                    timeline_chat_id,
                                                    &item,
                                                    item.sender_name.clone(),
                                                    album,
                                                )));
                                            },
                                        }
                                    }
                                } else {
                                    rsx! {
                                        div { class: "empty-state",
                                            p { class: "hint", "Selecione uma conversa para explorar suas mídias." }
                                        }
                                    }
                                }
                            }
                            DrawerTab::Topics => {
                                if let Some(chat) = browser_chat {
                                    rsx! {
                                        TopicBrowser {
                                            chat,
                                            on_close: {
                                                let services = services.clone();
                                                move |_| set_drawer_open(&services, ui, false)
                                            },
                                        }
                                    }
                                } else {
                                    rsx! {
                                        div { class: "empty-state",
                                            p { class: "hint", "Selecione uma conversa para ver seus tópicos." }
                                        }
                                    }
                                }
                            }
                            DrawerTab::MassDownload => {
                                if let Some(chat) = mass_chat {
                                    rsx! {
                                        MassDownloadPanel {
                                            chat,
                                            topics: mass_topics,
                                            initial_topic: mass_initial_topic,
                                            on_close: {
                                                let services = services.clone();
                                                move |_| set_drawer_open(&services, ui, false)
                                            },
                                        }
                                    }
                                } else {
                                    rsx! {
                                        div { class: "empty-state",
                                            p { class: "hint", "Selecione uma conversa ou galeria para baixar em massa." }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            MobileBottomNav {}

            if let Some(state) = lightbox_state {
                MediaLightbox {
                    key: "{state.message_id}",
                    state,
                    on_close: move |_| ui.panels.lightbox.set(None),
                }
            }

            if let Some(state) = forward {
                ForwardDialog {
                    state,
                    chats: ui.conversation.chats(),
                    busy: forwarding(),
                    on_close: move |_| ui.panels.forward_state.set(None),
                    on_forward: {
                        let services = services.clone();
                        move |target: TdlibChatInfo| {
                            let Some(state) = ui.panels.forward_state() else { return; };
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
                                            ui.shell.toasts,
                                            format!("Mensagem encaminhada para {}.", target.title),
                                            ToastKind::Success,
                                        );
                                        ui.panels.forward_state.set(None);
                                    }
                                    Ok(result) => push_toast(
                                        ui.shell.toasts,
                                        result
                                            .error
                                            .unwrap_or_else(|| "Não foi possível encaminhar.".to_string()),
                                        ToastKind::Error,
                                    ),
                                    Err(error) => push_toast(ui.shell.toasts, error, ToastKind::Error),
                                }
                                forwarding.set(false);
                            });
                        }
                    },
                }
            }

            DropMediaDialog { drop, sending }

            if let Some(items) = duplicate_search_items {
                DuplicateSearchDialog {
                    items,
                    on_close: move |_| ui.transfers.duplicate_search.set(None),
                }
            }
        }
    }
}
