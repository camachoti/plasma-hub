//! Unified vertical player for Reddit, YouTube Shorts, X/Twitter, Instagram Reels and pasted links.
//!
//! Renders a single bounded slide at a time (one `<video>` element in the DOM,
//! zero oversized scroll containers or `document::eval` hooks) so WebKitGTK on
//! Wayland/NVIDIA never exceeds compositor surface limits or spawns competing
//! GStreamer pipelines.

use crate::downloads::{
    complete_download_record, new_download_id, push_download, sanitize_filename, update_download,
    DownloadRecord,
};
use crate::icons::{Icon, IconName};
use crate::state::{
    push_toast, push_toast_with_action, AppServices, Tab, ToastAction, ToastKind, UiState,
};
use crate::twitter_fake::read_twitter_fake_chats;
use dioxus::prelude::*;
use plasma_extract::{ReelClip, ResolvedReelStream};
use plasma_store::SettingKey;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(600);
const WHEEL_COOLDOWN: Duration = Duration::from_millis(280);

struct CachedReelFeed {
    fingerprint: String,
    items: Vec<ReelClip>,
    cursor: Option<String>,
    saved_at: Instant,
}

fn feed_cache() -> &'static Mutex<Option<CachedReelFeed>> {
    static CACHE: OnceLock<Mutex<Option<CachedReelFeed>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReelFilter {
    All,
    Reddit,
    YouTube,
    Twitter,
    Instagram,
    TikTok,
}

impl ReelFilter {
    const ALL: [Self; 6] = [
        Self::All,
        Self::Reddit,
        Self::YouTube,
        Self::Twitter,
        Self::Instagram,
        Self::TikTok,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::All => "Todos",
            Self::Reddit => "Reddit",
            Self::YouTube => "YouTube",
            Self::Twitter => "X / Twitter",
            Self::Instagram => "Instagram",
            Self::TikTok => "TikTok",
        }
    }

    fn matches(self, clip: &ReelClip) -> bool {
        match self {
            Self::All => true,
            Self::Reddit => clip.platform == "reddit",
            Self::YouTube => clip.platform == "youtube",
            Self::Twitter => clip.platform == "twitter",
            Self::Instagram => clip.platform == "instagram",
            Self::TikTok => clip.platform == "tiktok",
        }
    }
}

#[derive(Clone, Default)]
struct ReelSourceConfig {
    interests: Vec<String>,
    extra_subs: Vec<String>,
    twitter_users: Vec<String>,
    instagram_users: Vec<String>,
    youtube_sources: Vec<String>,
    tiktok_sources: Vec<String>,
}

impl ReelSourceConfig {
    fn reddit_subreddits(&self) -> Vec<String> {
        let mut subs = plasma_extract::subreddits_for_interests(&self.interests);
        for extra in &self.extra_subs {
            if !subs.iter().any(|item| item.eq_ignore_ascii_case(extra)) {
                subs.push(extra.clone());
            }
        }
        subs
    }

    fn effective_twitter_users(&self) -> Vec<String> {
        if !self.twitter_users.is_empty() {
            return self.twitter_users.clone();
        }
        plasma_extract::default_twitter_for_interests(&self.interests)
    }

    fn effective_instagram_users(&self) -> Vec<String> {
        if !self.instagram_users.is_empty() {
            return self.instagram_users.clone();
        }
        plasma_extract::default_instagram_for_interests(&self.interests)
    }

    fn fingerprint(&self) -> String {
        format!(
            "i={}|r={}|tw={}|ig={}|yt={}|tt={}",
            self.interests.join(","),
            self.extra_subs.join(","),
            self.twitter_users.join(","),
            self.instagram_users.join(","),
            self.youtube_sources.join(","),
            self.tiktok_sources.join(","),
        )
    }
}

#[component]
pub fn ReelsView() -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let services_boot = services.clone();
    let services_more = services.clone();
    let services_prefetch = services.clone();
    let mut queue = use_signal(Vec::<ReelClip>::new);
    let mut cursor = use_signal(|| None::<String>);
    let mut saved_config = use_signal(ReelSourceConfig::default);
    let mut selected_interests = use_signal(Vec::<String>::new);
    let mut subs_input = use_signal(String::new);
    let mut twitter_input = use_signal(String::new);
    let mut instagram_input = use_signal(String::new);
    let mut youtube_input = use_signal(String::new);
    let mut tiktok_input = use_signal(String::new);
    let mut platform_filter = use_signal(|| ReelFilter::All);
    let mut paste = use_signal(String::new);
    let mut pasting = use_signal(|| false);
    let mut sources_open = use_signal(|| false);
    let mut loading = use_signal(|| false);
    let mut loading_more = use_signal(|| false);
    let mut error = use_signal(|| None::<String>);
    let mut active = use_signal(|| 0usize);
    let mut sound = use_signal(|| false);
    let mut volume = use_signal(|| 85u8);
    let mut booted = use_signal(|| false);
    let mut resolved_streams = use_signal(HashMap::<String, ResolvedReelStream>::new);
    let mut resolve_failures = use_signal(HashMap::<String, String>::new);
    let mut resolving_keys = use_signal(HashSet::<String>::new);
    let mut last_wheel = use_signal(|| None::<Instant>);
    let mut touch_start_y = use_signal(|| None::<f64>);

    use_effect(move || {
        if *booted.peek() {
            return;
        }
        booted.set(true);
        let config = read_source_config(&services_boot.settings);
        selected_interests.set(config.interests.clone());
        subs_input.set(config.extra_subs.join(", "));
        twitter_input.set(
            config
                .twitter_users
                .iter()
                .map(|u| format!("@{u}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        instagram_input.set(
            config
                .instagram_users
                .iter()
                .map(|u| format!("@{u}"))
                .collect::<Vec<_>>()
                .join(", "),
        );
        youtube_input.set(config.youtube_sources.join(", "));
        tiktok_input.set(config.tiktok_sources.join(", "));
        saved_config.set(config.clone());

        let services = services_boot.clone();
        spawn(async move {
            load_unified_queue(
                services,
                config,
                ReelQueue {
                    items: queue,
                    cursor,
                    loading,
                    error,
                },
                ui,
                false,
            )
            .await;
            active.set(0);
        });
    });

    // Background stream resolution for the active clip and the next one, plus
    // pagination when nearing the end of the queue — all in Tokio without extra DOM nodes.
    use_effect(move || {
        // Subscribe to all three signals unconditionally BEFORE any early return!
        let current_filter = platform_filter();
        let raw_active = active();
        let all = queue();
        let visible: Vec<ReelClip> = all
            .into_iter()
            .filter(|clip| current_filter.matches(clip))
            .collect();
        if visible.is_empty() {
            return;
        }
        let idx = raw_active.min(visible.len().saturating_sub(1));

        for candidate in visible.iter().skip(idx).take(5).cloned() {
            if !candidate.media_url.is_empty() {
                continue;
            }
            let key = format!("{}:{}", candidate.platform, candidate.id);
            if resolved_streams.peek().contains_key(&key)
                || resolve_failures.peek().contains_key(&key)
                || resolving_keys.peek().contains(&key)
            {
                continue;
            }
            resolving_keys.write().insert(key.clone());
            let services = services_prefetch.clone();
            spawn(async move {
                match services.extract.resolve_reel_stream(candidate).await {
                    Ok(stream) => {
                        resolved_streams.write().insert(key.clone(), stream);
                    }
                    Err(err) => {
                        resolve_failures
                            .write()
                            .insert(key.clone(), err.to_string());
                    }
                }
                resolving_keys.write().remove(&key);
            });
        }

        let after = cursor.peek().clone();
        if idx + 3 >= visible.len()
            && after.is_some()
            && !*loading_more.peek()
            && !*loading.peek()
            && matches!(current_filter, ReelFilter::All | ReelFilter::Reddit)
        {
            if let Some(after) = after {
                let subs = saved_config.peek().reddit_subreddits();
                if !subs.is_empty() {
                    loading_more.set(true);
                    let services = services_more.clone();
                    spawn(async move {
                        match services.extract.list_reddit_reels(subs, Some(after)).await {
                            Ok(page) => {
                                let mut items = queue();
                                for clip in page.items {
                                    if !items.iter().any(|item| {
                                        item.platform == clip.platform && item.id == clip.id
                                    }) {
                                        items.push(clip);
                                    }
                                }
                                queue.set(items);
                                cursor.set(page.after);
                            }
                            Err(err) => {
                                cursor.set(None);
                                push_toast(ui.shell.toasts, err.to_string(), ToastKind::Info);
                            }
                        }
                        loading_more.set(false);
                    });
                }
            }
        }
    });

    let apply_sources = {
        let services = services.clone();
        move |_| {
            let services = services.clone();
            let mut ids = selected_interests();
            if ids.is_empty() {
                ids = plasma_extract::default_reel_interests();
                selected_interests.set(ids.clone());
            }

            let raw_subs = split_csv(&subs_input());
            let extra_subs = if raw_subs.is_empty() {
                Vec::new()
            } else {
                match plasma_extract::normalize_reels_subreddits(&raw_subs) {
                    Ok(list) => list,
                    Err(msg) => {
                        push_toast(ui.shell.toasts, msg, ToastKind::Error);
                        return;
                    }
                }
            };

            let raw_twitter = split_csv(&twitter_input());
            let twitter_users = match plasma_extract::normalize_reels_twitter(&raw_twitter) {
                Ok(list) => list,
                Err(msg) => {
                    push_toast(ui.shell.toasts, msg, ToastKind::Error);
                    return;
                }
            };

            let raw_ig = split_csv(&instagram_input());
            let instagram_users = if raw_ig.is_empty() {
                Vec::new()
            } else {
                match plasma_extract::normalize_reels_instagram(&raw_ig) {
                    Ok(list) => list,
                    Err(msg) => {
                        push_toast(ui.shell.toasts, msg, ToastKind::Error);
                        return;
                    }
                }
            };

            let raw_yt = split_csv(&youtube_input());
            let youtube_sources = match plasma_extract::normalize_reels_youtube(&raw_yt) {
                Ok(list) => list,
                Err(msg) => {
                    push_toast(ui.shell.toasts, msg, ToastKind::Error);
                    return;
                }
            };

            let raw_tt = split_csv(&tiktok_input());
            let tiktok_sources = match plasma_extract::normalize_reels_tiktok(&raw_tt) {
                Ok(list) => list,
                Err(msg) => {
                    push_toast(ui.shell.toasts, msg, ToastKind::Error);
                    return;
                }
            };

            let _ = services
                .settings
                .set_json(SettingKey::REELS_INTERESTS, &ids);
            let _ = services
                .settings
                .set_json(SettingKey::REELS_SUBREDDITS, &extra_subs);
            let _ = services
                .settings
                .set_json(SettingKey::REELS_TWITTER, &twitter_users);
            let _ = services
                .settings
                .set_json(SettingKey::REELS_INSTAGRAM, &instagram_users);
            let _ = services
                .settings
                .set_json(SettingKey::REELS_YOUTUBE, &youtube_sources);
            let _ = services
                .settings
                .set_json(SettingKey::REELS_TIKTOK, &tiktok_sources);

            let config = ReelSourceConfig {
                interests: ids,
                extra_subs,
                twitter_users,
                instagram_users,
                youtube_sources,
                tiktok_sources,
            };
            saved_config.set(config.clone());
            error.set(None);
            sources_open.set(false);
            loading.set(true);
            queue.set(Vec::new());
            cursor.set(None);
            active.set(0);
            spawn(async move {
                load_unified_queue(
                    services,
                    config,
                    ReelQueue {
                        items: queue,
                        cursor,
                        loading,
                        error,
                    },
                    ui,
                    true,
                )
                .await;
            });
        }
    };

    let refresh_feed = {
        let services = services.clone();
        move |_| {
            if loading() {
                return;
            }
            let config = saved_config();
            let services = services.clone();
            error.set(None);
            loading.set(true);
            queue.set(Vec::new());
            cursor.set(None);
            resolve_failures.write().clear();
            active.set(0);
            spawn(async move {
                load_unified_queue(
                    services,
                    config,
                    ReelQueue {
                        items: queue,
                        cursor,
                        loading,
                        error,
                    },
                    ui,
                    true,
                )
                .await;
            });
        }
    };

    let mut paste_link = {
        let services = services.clone();
        move |_| {
            if pasting() {
                return;
            }
            let raw = paste().trim().to_string();
            if raw.is_empty() {
                return;
            }
            if !raw.starts_with("https://") {
                push_toast(
                    ui.shell.toasts,
                    "Link inválido. Use uma URL iniciando com https://.",
                    ToastKind::Error,
                );
                return;
            }
            pasting.set(true);
            let services = services.clone();
            let insert_at = active();
            spawn(async move {
                match services.extract.resolve_clip(raw).await {
                    Ok(clip) => {
                        if !platform_filter().matches(&clip) {
                            platform_filter.set(ReelFilter::All);
                        }
                        let mut items = queue();
                        let index = insert_at.min(items.len());
                        items.insert(index, clip);
                        queue.set(items);
                        active.set(index);
                        paste.set(String::new());
                        error.set(None);
                    }
                    Err(err) => {
                        push_toast(
                            ui.shell.toasts,
                            format!("Não foi possível abrir o link. {err}"),
                            ToastKind::Error,
                        );
                    }
                }
                pasting.set(false);
            });
        }
    };

    let all_items = queue();
    let current_filter = platform_filter();
    let visible_items: Vec<ReelClip> = all_items
        .iter()
        .filter(|clip| current_filter.matches(clip))
        .cloned()
        .collect();
    let total_visible = visible_items.len();
    let current = active().min(total_visible.saturating_sub(1));
    let current_clip = visible_items.get(current).cloned();
    let audible = sound();
    let synced_twitter_count = read_twitter_fake_chats(&services.settings).len();

    let mut step_reel = move |delta: isize| {
        if total_visible == 0 {
            return;
        }
        let next = if delta > 0 {
            (current + 1).min(total_visible - 1)
        } else {
            current.saturating_sub(1)
        };
        if next != current {
            active.set(next);
        }
    };

    rsx! {
        section {
            class: "reels-view",
            tabindex: "0",
            onkeydown: move |event: KeyboardEvent| {
                match event.key() {
                    Key::ArrowDown | Key::PageDown => {
                        event.prevent_default();
                        step_reel(1);
                    }
                    Key::ArrowUp | Key::PageUp => {
                        event.prevent_default();
                        step_reel(-1);
                    }
                    Key::Character(ref ch) if ch.eq_ignore_ascii_case("m") => {
                        event.prevent_default();
                        sound.set(!sound());
                    }
                    Key::Character(ref ch) if ch == " " || ch.eq_ignore_ascii_case("k") => {
                        event.prevent_default();
                        let _ = document::eval(
                            r#"(() => { const v = document.querySelector(".reel-video"); const a = document.querySelector(".reel-sidecar-audio"); if (!v) return; if (v.paused) { v.play().catch(() => {}); if (a) a.play().catch(() => {}); } else { v.pause(); if (a) a.pause(); } })()"#,
                        );
                    }
                    _ => {}
                }
            },
            header { class: "reels-bar",
                div { class: "reels-bar-title",
                    if !ui.shell.sidebar_open() {
                        button {
                            class: "icon-button",
                            title: "Mostrar conversas",
                            aria_label: "Mostrar conversas",
                            onclick: move |_| ui.shell.sidebar_open.set(true),
                            Icon { name: IconName::LayoutSidebar, size: 16, class: "icon" }
                        }
                    }
                    h1 { "Reels" }
                }
                form {
                    class: "reels-paste",
                    onsubmit: move |event| {
                        event.prevent_default();
                        paste_link(());
                    },
                    input {
                        class: "download-input",
                        r#type: "text",
                        placeholder: "Colar link do YouTube, X, Instagram, Reddit ou TikTok…",
                        value: "{paste}",
                        onkeydown: move |e: KeyboardEvent| e.stop_propagation(),
                        oninput: move |event| paste.set(event.value()),
                    }
                    button {
                        class: "btn small primary",
                        r#type: "submit",
                        disabled: pasting() || paste().trim().is_empty(),
                        if pasting() { "Abrindo…" } else { "Assistir" }
                    }
                }
                button {
                    class: "icon-button",
                    title: "Recarregar feed",
                    aria_label: "Recarregar feed",
                    disabled: loading(),
                    onclick: refresh_feed,
                    Icon { name: IconName::Refresh, size: 16, class: "icon" }
                }
                button {
                    class: if sources_open() { "btn small active" } else { "btn small soft" },
                    onclick: move |_| sources_open.set(!sources_open()),
                    "Fontes"
                }
            }

            div { class: "reels-platforms", role: "tablist",
                for option in ReelFilter::ALL {
                    {
                        let count = all_items.iter().filter(|clip| option.matches(clip)).count();
                        let selected = current_filter == option;
                        rsx! {
                            button {
                                key: "{option.label()}",
                                class: if selected { "reel-platform-tab active" } else { "reel-platform-tab" },
                                role: "tab",
                                aria_selected: selected,
                                onclick: move |_| {
                                    platform_filter.set(option);
                                    active.set(0);
                                },
                                "{option.label()}"
                                span { class: "reel-platform-count", "{count}" }
                            }
                        }
                    }
                }
            }

            if sources_open() {
                div {
                    class: "reels-config-panel",
                    onkeydown: move |e: KeyboardEvent| e.stop_propagation(),
                    div { class: "reels-interests",
                        span { class: "reels-config-label", "Interesses públicos (Reddit, Shorts, X e Instagram):" }
                        for interest in plasma_extract::REEL_INTERESTS {
                            {
                                let id = interest.id.to_string();
                                let is_active = selected_interests().iter().any(|item| item == interest.id);
                                let class = if is_active { "reel-interest active" } else { "reel-interest" };
                                rsx! {
                                    button {
                                        key: "{id}",
                                        class: "{class}",
                                        onclick: move |_| {
                                            let mut ids = selected_interests();
                                            if let Some(index) = ids.iter().position(|item| item == &id) {
                                                ids.remove(index);
                                            } else {
                                                ids.push(id.clone());
                                            }
                                            selected_interests.set(ids);
                                        },
                                        "{interest.label}"
                                    }
                                }
                            }
                        }
                    }
                    div { class: "reels-sources-grid",
                        label {
                            "Subreddits extras (Reddit)"
                            input {
                                class: "download-input",
                                r#type: "text",
                                placeholder: "videos, Unexpected, MadeMeSmile",
                                value: "{subs_input}",
                                oninput: move |e| subs_input.set(e.value()),
                            }
                        }
                        label {
                            if synced_twitter_count > 0 {
                                "Perfis do X / Twitter (+{synced_twitter_count} galerias salvas)"
                            } else {
                                "Perfis do X / Twitter"
                            }
                            input {
                                class: "download-input",
                                r#type: "text",
                                placeholder: "@nasa, @IGN",
                                value: "{twitter_input}",
                                oninput: move |e| twitter_input.set(e.value()),
                            }
                        }
                        label {
                            "Perfis públicos do Instagram"
                            input {
                                class: "download-input",
                                r#type: "text",
                                placeholder: "@natgeo, @9gag",
                                value: "{instagram_input}",
                                oninput: move |e| instagram_input.set(e.value()),
                            }
                        }
                        label {
                            "Canais ou temas do YouTube Shorts"
                            input {
                                class: "download-input",
                                r#type: "text",
                                placeholder: "@kurzgesagt, ciência, música",
                                value: "{youtube_input}",
                                oninput: move |e| youtube_input.set(e.value()),
                            }
                        }
                        label {
                            "Perfis ou temas públicos do TikTok"
                            input {
                                class: "download-input",
                                r#type: "text",
                                placeholder: "@khaby.lame, humor, jogos",
                                value: "{tiktok_input}",
                                oninput: move |e| tiktok_input.set(e.value()),
                            }
                        }
                    }
                    div { class: "reels-config-actions",
                        span { class: "hint",
                            "Modo público sem login: Reddit, Shorts, X, Instagram e TikTok entram automaticamente pelos interesses ou pelas fontes acima."
                        }
                        button {
                            class: "btn small primary",
                            onclick: apply_sources,
                            "Salvar e atualizar fila"
                        }
                    }
                }
            }

            if let Some(message) = error() {
                p { class: "error-banner reels-error", "{message}" }
            }

            div {
                class: "reels-stage",
                onwheel: move |event: WheelEvent| {
                    let dy = event.delta().strip_units().y;
                    if dy.abs() < 12.0 {
                        return;
                    }
                    let now = Instant::now();
                    if last_wheel()
                        .is_some_and(|prev| now.duration_since(prev) < WHEEL_COOLDOWN)
                    {
                        return;
                    }
                    last_wheel.set(Some(now));
                    step_reel(if dy > 0.0 { 1 } else { -1 });
                },
                ontouchstart: move |event: TouchEvent| {
                    if let Some(touch) = event.touches().first() {
                        touch_start_y.set(Some(touch.client_coordinates().y));
                    }
                },
                ontouchend: move |event: TouchEvent| {
                    let Some(start_y) = touch_start_y() else {
                        return;
                    };
                    touch_start_y.set(None);
                    if let Some(touch) = event.touches_changed().first() {
                        let dy = start_y - touch.client_coordinates().y;
                        if dy.abs() > 36.0 {
                            step_reel(if dy > 0.0 { 1 } else { -1 });
                        }
                    }
                },
                if loading() && current_clip.is_none() {
                    div { class: "reels-empty",
                        p { "Carregando vídeos públicos de Reddit, YouTube, X e Instagram…" }
                    }
                } else if let Some(item) = current_clip {
                    {
                        let key = format!("{}:{}", item.platform, item.id);
                        let stream = if !item.media_url.is_empty() {
                            Some(ResolvedReelStream {
                                video_url: item.media_url.clone(),
                                audio_url: None,
                                author: None,
                                title: None,
                            })
                        } else {
                            resolved_streams.read().get(&key).cloned()
                        };
                        let is_resolving = resolving_keys.read().contains(&key);
                        let item_error = resolve_failures.read().get(&key).cloned();
                        rsx! {
                            ReelSlide {
                                key: "{item.platform}-{item.id}",
                                clip: item,
                                resolved_stream: stream,
                                resolving: is_resolving,
                                resolve_error: item_error,
                                audible,
                                volume: volume(),
                                on_toggle_sound: move |_| sound.set(!sound()),
                                on_change_volume: move |v: u8| {
                                    volume.set(v);
                                    if v > 0 && !sound() {
                                        sound.set(true);
                                    } else if v == 0 && sound() {
                                        sound.set(false);
                                    }
                                },
                            }
                            div { class: "reels-stepper",
                                button {
                                    class: "reel-step-btn",
                                    r#type: "button",
                                    title: "Vídeo anterior (↑)",
                                    aria_label: "Vídeo anterior",
                                    disabled: current == 0,
                                    onclick: move |e| {
                                        e.stop_propagation();
                                        step_reel(-1);
                                    },
                                    "↑"
                                }
                                span { class: "reel-step-counter",
                                    "{current + 1} / {total_visible}"
                                }
                                button {
                                    class: "reel-step-btn",
                                    r#type: "button",
                                    title: "Próximo vídeo (↓)",
                                    aria_label: "Próximo vídeo",
                                    disabled: current + 1 >= total_visible,
                                    onclick: move |e| {
                                        e.stop_propagation();
                                        step_reel(1);
                                    },
                                    "↓"
                                }
                            }
                        }
                    }
                } else {
                    div { class: "reels-empty",
                        h2 {
                            if all_items.is_empty() {
                                "Nenhum vídeo na fila"
                            } else {
                                "Nenhum vídeo de {current_filter.label()} carregado"
                            }
                        }
                        p {
                            if matches!(current_filter, ReelFilter::Instagram | ReelFilter::Twitter) {
                                "Abra “Fontes” para adicionar @perfis desta rede ou cole um link direto acima."
                            } else {
                                "Escolha interesses/perfis em “Fontes” ou cole um link acima para assistir."
                            }
                        }
                        button {
                            class: "btn small soft",
                            onclick: move |_| sources_open.set(true),
                            "Configurar fontes"
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn ReelSlide(
    clip: ReelClip,
    resolved_stream: Option<ResolvedReelStream>,
    resolving: bool,
    resolve_error: Option<String>,
    audible: bool,
    volume: u8,
    on_toggle_sound: EventHandler<()>,
    on_change_volume: EventHandler<u8>,
) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let mut video_paused = use_signal(|| false);

    let display_author = resolved_stream
        .as_ref()
        .and_then(|s| s.author.clone())
        .filter(|a| !a.trim().is_empty())
        .unwrap_or_else(|| clip.author.clone());
    let display_title = resolved_stream
        .as_ref()
        .and_then(|s| s.title.clone())
        .filter(|t| !t.trim().is_empty() && clip.title == "YouTube Short")
        .unwrap_or_else(|| clip.title.clone());

    let direct_media_url = resolved_stream.as_ref().map(|s| s.video_url.clone());
    let playback = direct_media_url
        .as_deref()
        .and_then(plasma_telegram::remote_media_url);

    let separate_audio = if audible {
        resolved_stream
            .as_ref()
            .and_then(|s| s.audio_url.clone())
            .or_else(|| {
                if clip.platform == "reddit" {
                    direct_media_url
                        .as_deref()
                        .filter(|u| u.contains("/DASH_"))
                        .and_then(plasma_extract_reddit_audio)
                } else {
                    None
                }
            })
            .and_then(|u| plasma_telegram::remote_media_url(&u))
    } else {
        None
    };

    let has_sidecar_audio = separate_audio.is_some();
    let platform = platform_label(&clip.platform);
    let mut download_target = clip.clone();
    download_target.author = display_author.clone();
    download_target.title = display_title.clone();
    if download_target.media_url.is_empty()
        && resolved_stream
            .as_ref()
            .is_some_and(|s| s.audio_url.is_none())
    {
        if let Some(url) = direct_media_url.clone() {
            download_target.media_url = url;
        }
    }
    let page_url = clip.page_url.clone();
    let page_url_fallback = clip.page_url.clone();
    let preview_thumb = clip
        .thumbnail_url
        .clone()
        .filter(|u| u.starts_with("https://"))
        .and_then(|u| plasma_telegram::remote_media_url(&u).or(Some(u)));

    rsx! {
        article { class: "reel-slide",
            if let Some(src) = playback {
                video {
                    key: "{clip.platform}-{clip.id}",
                    class: "reel-video",
                    src: "{src}",
                    autoplay: true,
                    muted: !audible || has_sidecar_audio,
                    playsinline: true,
                    r#loop: true,
                    preload: "auto",
                    onplay: move |_| video_paused.set(false),
                    onpause: move |_| video_paused.set(true),
                    onloadeddata: move |_| {
                        let vol = (volume as f64 / 100.0).clamp(0.0, 1.0);
                        let _ = document::eval(&format!(
                            r#"(() => {{ const v = document.querySelector(".reel-video"); const a = document.querySelector(".reel-sidecar-audio"); if (v) {{ v.volume = {vol}; if (v.paused) {{ v.play().catch(() => {{ v.muted = true; v.play().catch(() => {{}}); }}); }} }} if (a) a.volume = {vol}; }})()"#,
                        ));
                    },
                    ontimeupdate: move |_| {
                        let _ = document::eval(
                            r#"(() => { const v = document.querySelector(".reel-video"); if (!v) return; const bar = document.querySelector(".reel-playback-fill"); if (bar && v.duration > 0) { bar.style.width = ((v.currentTime / v.duration) * 100).toFixed(1) + "%"; } const a = document.querySelector(".reel-sidecar-audio"); if (a && Math.abs(v.currentTime - a.currentTime) > 0.35) { a.currentTime = v.currentTime; if (!v.paused && a.paused) a.play().catch(() => {}); } })()"#,
                        );
                    },
                    onclick: move |_| {
                        let _ = document::eval(
                            r#"(() => { const v = document.querySelector(".reel-video"); const a = document.querySelector(".reel-sidecar-audio"); if (!v) return; if (v.paused) { v.play().catch(() => {}); if (a) a.play().catch(() => {}); } else { v.pause(); if (a) a.pause(); } })()"#,
                        );
                    },
                }
                if let Some(ref audio_src) = separate_audio {
                    audio {
                        key: "{clip.id}-sidecar-audio",
                        class: "reel-sidecar-audio",
                        src: "{audio_src}",
                        autoplay: !video_paused(),
                        r#loop: true,
                    }
                }
                if video_paused() {
                    div { class: "reel-paused-badge", aria_hidden: "true", "▶" }
                }
                div {
                    class: "reel-playback-track",
                    title: "Clique para avançar ou voltar no vídeo",
                    onclick: move |event: MouseEvent| {
                        event.stop_propagation();
                        let x = event.element_coordinates().x;
                        let _ = document::eval(&format!(
                            r#"(() => {{ const track = document.querySelector(".reel-playback-track"); const v = document.querySelector(".reel-video"); const a = document.querySelector(".reel-sidecar-audio"); if (!track || !v || !(v.duration > 0)) return; const ratio = Math.max(0, Math.min(1, {x} / (track.clientWidth || 1))); const t = ratio * v.duration; v.currentTime = t; if (a) a.currentTime = t; const bar = document.querySelector(".reel-playback-fill"); if (bar) bar.style.width = (ratio * 100).toFixed(1) + "%"; }})()"#,
                        ));
                    },
                    div { class: "reel-playback-fill" }
                }
            } else if resolving || (direct_media_url.is_none() && resolve_error.is_none()) {
                div { class: "reels-empty",
                    if let Some(thumb) = preview_thumb {
                        img { class: "reel-poster-wait", src: "{thumb}", alt: "" }
                    }
                    p { "Preparando stream público do {platform}…" }
                }
            } else {
                div { class: "reels-empty",
                    if let Some(thumb) = preview_thumb {
                        img { class: "reel-poster-wait", src: "{thumb}", alt: "" }
                    }
                    p {
                        if let Some(err) = resolve_error {
                            "{err}"
                        } else {
                            "Este vídeo não pôde ser reproduzido diretamente sem login."
                        }
                    }
                    if page_url_fallback.starts_with("https://") {
                        button {
                            class: "btn small soft",
                            onclick: move |_| {
                                let _ = plasma_platform::open_url(&page_url_fallback);
                            },
                            Icon { name: IconName::External, size: 14, class: "icon" }
                            "Abrir no navegador"
                        }
                    }
                }
            }
            div { class: "reel-overlay",
                span { class: "reel-platform {clip.platform}", "{platform}" }
                strong { "{display_author}" }
                p { "{display_title}" }
                div { class: "reel-actions",
                    button {
                        class: "btn small soft",
                        onclick: move |event| {
                            event.stop_propagation();
                            on_toggle_sound.call(());
                        },
                        if audible { "Som ({volume}%)" } else { "Ativar som" }
                    }
                    if audible {
                        input {
                            class: "reel-volume-slider",
                            r#type: "range",
                            min: "0",
                            max: "100",
                            value: "{volume}",
                            aria_label: "Volume",
                            onclick: move |e| e.stop_propagation(),
                            oninput: move |e| {
                                if let Ok(v) = e.value().parse::<u8>() {
                                    on_change_volume.call(v);
                                    let vol = (v as f64 / 100.0).clamp(0.0, 1.0);
                                    let _ = document::eval(&format!(
                                        r#"(() => {{ const v = document.querySelector(".reel-video"); const a = document.querySelector(".reel-sidecar-audio"); if (v) v.volume = {vol}; if (a) a.volume = {vol}; }})()"#,
                                    ));
                                }
                            },
                        }
                    }
                    button {
                        class: "btn small primary",
                        onclick: move |event| {
                            event.stop_propagation();
                            download_clip(services.clone(), ui, download_target.clone());
                        },
                        Icon { name: IconName::Download, size: 14, class: "icon" }
                        "Baixar"
                    }
                    if page_url.starts_with("https://") {
                        button {
                            class: "btn small ghost",
                            onclick: move |event| {
                                event.stop_propagation();
                                let _ = plasma_platform::open_url(&page_url);
                            },
                            Icon { name: IconName::External, size: 14, class: "icon" }
                            "Origem"
                        }
                    }
                }
            }
        }
    }
}

struct ReelQueue {
    items: Signal<Vec<ReelClip>>,
    cursor: Signal<Option<String>>,
    loading: Signal<bool>,
    error: Signal<Option<String>>,
}

async fn load_unified_queue(
    services: AppServices,
    config: ReelSourceConfig,
    mut queue: ReelQueue,
    ui: UiState,
    force_refresh: bool,
) {
    let fingerprint = config.fingerprint();
    if !force_refresh {
        if let Ok(guard) = feed_cache().lock() {
            if let Some(cached) = guard.as_ref() {
                if cached.fingerprint == fingerprint
                    && cached.saved_at.elapsed() < CACHE_TTL
                    && !cached.items.is_empty()
                {
                    queue.items.set(cached.items.clone());
                    queue.cursor.set(cached.cursor.clone());
                    queue.error.set(None);
                    queue.loading.set(false);
                    return;
                }
            }
        }
    }

    queue.loading.set(true);
    queue.error.set(None);

    let reddit_subs = config.reddit_subreddits();
    let yt_sources = config.youtube_sources.clone();
    let yt_interests = config.interests.clone();
    let tt_sources = config.tiktok_sources.clone();
    let tt_interests = config.interests.clone();
    let twitter_users = config.effective_twitter_users();
    let instagram_users = config.effective_instagram_users();
    let custom_twitter = !config.twitter_users.is_empty();
    let custom_instagram = !config.instagram_users.is_empty();
    let custom_tiktok = !config.tiktok_sources.is_empty();

    let extract_reddit = services.extract.clone();
    let extract_yt = services.extract.clone();
    let extract_tw = services.extract.clone();
    let extract_ig = services.extract.clone();
    let extract_tt = services.extract.clone();

    let local_twitter = clips_from_fake_twitter(&services);

    let reddit_fut = async move {
        if reddit_subs.is_empty() {
            Ok(None)
        } else {
            extract_reddit
                .list_reddit_reels(reddit_subs, None)
                .await
                .map(Some)
        }
    };

    let youtube_fut = async move {
        extract_yt
            .list_youtube_reels(yt_sources, yt_interests)
            .await
    };

    let twitter_fut = async move {
        let mut clips = local_twitter;
        let mut last_err = None;
        for user in twitter_users.into_iter().take(4) {
            match extract_tw.list_twitter_reels(user).await {
                Ok(found) => clips.extend(found),
                Err(err) => last_err = Some(err.to_string()),
            }
        }
        (clips, last_err)
    };

    let instagram_fut = async move {
        let mut clips = Vec::new();
        let mut last_err = None;
        for user in instagram_users.into_iter().take(4) {
            match extract_ig.list_instagram_reels(user).await {
                Ok(found) => clips.extend(found),
                Err(err) => last_err = Some(err.to_string()),
            }
        }
        (clips, last_err)
    };

    let tiktok_fut = async move { extract_tt.list_tiktok_reels(tt_sources, tt_interests).await };

    let (reddit_res, youtube_res, (twitter_clips, tw_err), (ig_clips, ig_err), tiktok_res) = tokio::join!(
        reddit_fut,
        youtube_fut,
        twitter_fut,
        instagram_fut,
        tiktok_fut
    );

    let mut failures = Vec::new();
    let mut next_cursor = None;
    let mut reddit_clips = Vec::new();
    match reddit_res {
        Ok(Some(page)) => {
            next_cursor = page.after;
            reddit_clips = page.items;
        }
        Ok(None) => {}
        Err(err) => failures.push(format!("Reddit: {err}")),
    }

    let youtube_clips = match youtube_res {
        Ok(list) => list,
        Err(err) => {
            failures.push(format!("YouTube: {err}"));
            Vec::new()
        }
    };

    if custom_twitter {
        if let Some(err) = tw_err {
            failures.push(format!("X/Twitter: {err}"));
        }
    }
    if custom_instagram {
        if let Some(err) = ig_err {
            failures.push(format!("Instagram: {err}"));
        }
    }

    let tiktok_clips = match tiktok_res {
        Ok(list) => list,
        Err(err) => {
            if custom_tiktok {
                failures.push(format!("TikTok: {err}"));
            }
            Vec::new()
        }
    };

    let merged = interleave_buckets(vec![
        reddit_clips,
        youtube_clips,
        twitter_clips,
        ig_clips,
        tiktok_clips,
    ]);

    if !merged.is_empty() {
        if let Ok(mut guard) = feed_cache().lock() {
            *guard = Some(CachedReelFeed {
                fingerprint,
                items: merged.clone(),
                cursor: next_cursor.clone(),
                saved_at: Instant::now(),
            });
        }
    }

    if merged.is_empty() {
        queue.error.set(Some(failures.pop().unwrap_or_else(|| {
            "Nenhum vídeo público encontrado nas fontes selecionadas.".to_string()
        })));
    } else if let Some(message) = failures.first() {
        push_toast(ui.shell.toasts, message.clone(), ToastKind::Info);
    }

    queue.items.set(merged);
    queue.cursor.set(next_cursor);
    queue.loading.set(false);
}

fn plasma_extract_reddit_audio(video_url: &str) -> Option<String> {
    let base = video_url.rsplit_once('/')?.0;
    Some(format!("{base}/DASH_audio.mp4"))
}

fn clips_from_fake_twitter(services: &AppServices) -> Vec<ReelClip> {
    let chats = read_twitter_fake_chats(&services.settings);
    let mut clips = Vec::new();
    for chat in chats {
        let author = format!("@{}", chat.username.trim_start_matches('@'));
        let fallback_title = if chat.title.trim().is_empty() {
            author.clone()
        } else {
            chat.title.clone()
        };
        for msg in chat.messages.iter().rev().filter(|m| m.is_video).take(6) {
            let Some(url) = msg.url.as_ref().filter(|u| u.starts_with("https://")) else {
                continue;
            };
            let title = if msg.text.trim().is_empty() {
                fallback_title.clone()
            } else {
                msg.text.clone()
            };
            clips.push(ReelClip {
                platform: "twitter".to_string(),
                id: format!("{}-{}", chat.username, msg.id),
                title,
                author: author.clone(),
                page_url: chat.original_url.clone(),
                media_url: url.clone(),
                thumbnail_url: msg
                    .thumbnail_url
                    .clone()
                    .or_else(|| chat.thumbnail_url.clone()),
                format_id: "twitter-video".to_string(),
                has_audio: true,
            });
        }
    }
    clips
}

fn interleave_buckets(buckets: Vec<Vec<ReelClip>>) -> Vec<ReelClip> {
    let mut iters: Vec<std::vec::IntoIter<ReelClip>> =
        buckets.into_iter().map(Vec::into_iter).collect();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    loop {
        let mut progressed = false;
        for iter in &mut iters {
            if let Some(clip) = iter.next() {
                progressed = true;
                let dedup_key = if clip.media_url.is_empty() {
                    format!("{}:{}", clip.platform, clip.id)
                } else {
                    clip.media_url.clone()
                };
                if seen.insert(dedup_key) {
                    out.push(clip);
                }
            }
        }
        if !progressed {
            break;
        }
    }
    out
}

fn read_source_config(settings: &plasma_store::SettingsStore) -> ReelSourceConfig {
    let interests = settings
        .get_json::<Vec<String>>(SettingKey::REELS_INTERESTS)
        .ok()
        .flatten()
        .filter(|ids| !ids.is_empty())
        .unwrap_or_else(plasma_extract::default_reel_interests);
    let extra_subs = settings
        .get_json::<Vec<String>>(SettingKey::REELS_SUBREDDITS)
        .ok()
        .flatten()
        .unwrap_or_default();
    let twitter_users = settings
        .get_json::<Vec<String>>(SettingKey::REELS_TWITTER)
        .ok()
        .flatten()
        .unwrap_or_default();
    let instagram_users = settings
        .get_json::<Vec<String>>(SettingKey::REELS_INSTAGRAM)
        .ok()
        .flatten()
        .unwrap_or_default();
    let youtube_sources = settings
        .get_json::<Vec<String>>(SettingKey::REELS_YOUTUBE)
        .ok()
        .flatten()
        .unwrap_or_default();
    let tiktok_sources = settings
        .get_json::<Vec<String>>(SettingKey::REELS_TIKTOK)
        .ok()
        .flatten()
        .unwrap_or_default();

    ReelSourceConfig {
        interests,
        extra_subs,
        twitter_users,
        instagram_users,
        youtube_sources,
        tiktok_sources,
    }
}

fn split_csv(raw: &str) -> Vec<String> {
    raw.split([',', '\n', ';'])
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn download_clip(services: AppServices, ui: UiState, clip: ReelClip) {
    let stem = sanitize_filename(&format!("{}-{}", clip.author, clip.id));
    let stem: String = stem.chars().take(80).collect();
    let name = format!("{stem}.mp4");
    let destination = services.downloads.destination_for(&name);
    let file_name = Path::new(&destination)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&name)
        .to_string();
    let id = format!("reel-{}", new_download_id());
    let record_source = if clip.media_url.is_empty() {
        clip.page_url.clone()
    } else {
        clip.media_url.clone()
    };
    let mut record =
        DownloadRecord::new_url(id.clone(), file_name.clone(), destination, record_source);
    record.platform = Some(clip.platform.clone());
    record.source_page_url = Some(clip.page_url.clone());
    record.source_format_id = Some(clip.format_id.clone());
    record.source_label = Some(clip.author.clone());
    record.thumbnail_url = clip.thumbnail_url.clone();
    record.extra.insert(
        "hasAudio".to_string(),
        serde_json::Value::Bool(clip.has_audio),
    );
    let page_url = clip.page_url.clone();
    let format_id = clip.format_id.clone();
    let direct_url = (!clip.media_url.is_empty()).then_some(clip.media_url);
    let has_audio = clip.has_audio;
    push_download(services.clone(), ui, record);
    push_toast_with_action(
        ui.shell.toasts,
        "Download iniciado. Acompanhe o progresso em Atividade.",
        ToastKind::Success,
        "Ver fila",
        ToastAction::OpenTab(Tab::Downloads),
    );
    spawn(async move {
        let result = services
            .extract
            .download(
                id.clone(),
                page_url,
                format_id,
                direct_url,
                file_name,
                has_audio,
            )
            .await;
        match result {
            Ok(()) => update_download(services, ui, &id, complete_download_record, true),
            Err(error) if error.is_cancelled() => {
                update_download(
                    services,
                    ui,
                    &id,
                    |item| {
                        item.status = "canceled".to_string();
                        item.error = None;
                        item.can_cancel = Some(false);
                        item.can_retry = Some(true);
                    },
                    true,
                );
            }
            Err(error) => {
                let message = error.to_string();
                update_download(
                    services,
                    ui,
                    &id,
                    |item| {
                        item.status = "failed".to_string();
                        item.error = Some(message.clone());
                        item.can_cancel = Some(false);
                        item.can_retry = Some(true);
                    },
                    true,
                );
                push_toast_with_action(
                    ui.shell.toasts,
                    format!("Falha no download. {message}"),
                    ToastKind::Error,
                    "Tentar de novo",
                    ToastAction::RetryDownload(id),
                );
            }
        }
    });
}

fn platform_label(platform: &str) -> &'static str {
    match platform {
        "reddit" => "Reddit",
        "instagram" => "Instagram",
        "twitter" => "X / Twitter",
        "tiktok" => "TikTok",
        "youtube" => "YouTube Shorts",
        _ => "Link",
    }
}
