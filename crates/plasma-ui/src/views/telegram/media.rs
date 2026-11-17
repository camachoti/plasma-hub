//! Shared media panel, lightbox and size formatting.

use crate::context::pick_save_folder;
use crate::downloads::{save_telegram_media, TelegramSaveBatch};
use crate::icons::{Icon, IconName};
use crate::state::{service_error, AppServices, LightboxMedia, LightboxState, UiState};
use crate::views::timeline::{format_timestamp, MessageThumbnail};
use dioxus::prelude::*;
use plasma_telegram::TdlibSharedMediaItem;
use std::collections::HashMap;

pub(super) struct MediaPanelGroup {
    album_id: Option<String>,
    items: Vec<TdlibSharedMediaItem>,
}

/// Groups loaded media into album sections while keeping the original order.
/// Album ids can appear in more than one page boundary, so items sharing an id
/// are folded into the section created by their first occurrence.
pub(super) fn group_media_by_album(items: &[TdlibSharedMediaItem]) -> Vec<MediaPanelGroup> {
    let mut groups: Vec<MediaPanelGroup> = Vec::new();
    let mut album_positions: HashMap<String, usize> = HashMap::new();
    for item in items {
        if let Some(album_id) = item.grouped_id.as_deref().filter(|id| !id.is_empty()) {
            if let Some(&index) = album_positions.get(album_id) {
                groups[index].items.push(item.clone());
            } else {
                album_positions.insert(album_id.to_string(), groups.len());
                groups.push(MediaPanelGroup {
                    album_id: Some(album_id.to_string()),
                    items: vec![item.clone()],
                });
            }
        } else if let Some(last) = groups.last_mut().filter(|group| group.album_id.is_none()) {
            last.items.push(item.clone());
        } else {
            groups.push(MediaPanelGroup {
                album_id: None,
                items: vec![item.clone()],
            });
        }
    }
    groups
}

pub(super) fn lightbox_album_for(
    item: &TdlibSharedMediaItem,
    album_groups: &HashMap<String, Vec<LightboxMedia>>,
) -> Vec<LightboxMedia> {
    item.grouped_id
        .as_deref()
        .and_then(|group| album_groups.get(group))
        .filter(|album| album.len() > 1)
        .cloned()
        .unwrap_or_default()
}

/// Saves media sequentially so TDLib is not flooded with parallel requests.
/// `dest_dir` optionally directs the files into a user-chosen folder.
pub(super) async fn save_media_batch(
    services: &AppServices,
    ui: UiState,
    chat_id: i64,
    items: &[TdlibSharedMediaItem],
    batch: Option<&TelegramSaveBatch>,
    mut progress: Signal<Option<(usize, usize)>>,
    dest_dir: Option<String>,
) -> plasma_core::Result<()> {
    let total = items.len();
    for (index, item) in items.iter().enumerate() {
        progress.set(Some((index + 1, total)));
        let result = save_telegram_media(
            services,
            ui,
            chat_id,
            item.id,
            format!("Mídia {}", item.id),
            item.media_size,
            batch.map(|batch| (batch, index + 1)),
            dest_dir.clone(),
        )
        .await?;
        if !result.success {
            return Err(service_error(
                result
                    .error
                    .unwrap_or_else(|| "Não foi possível salvar a mídia.".to_string()),
            ));
        }
    }
    let _ = services.enforce_media_cache_limit().await;
    Ok(())
}

#[component]
pub(super) fn SharedMediaCard(
    chat_id: i64,
    item: TdlibSharedMediaItem,
    album: Vec<LightboxMedia>,
    on_open: EventHandler<(TdlibSharedMediaItem, Vec<LightboxMedia>)>,
) -> Element {
    rsx! {
        div {
            class: "shared-item",
            onclick: {
                let item = item.clone();
                let album = album.clone();
                move |_| on_open.call((item.clone(), album.clone()))
            },
            MessageThumbnail {
                chat_id,
                message_id: item.id,
                is_video: item.is_video,
                on_open: {
                    let item = item.clone();
                    let album = album.clone();
                    move |_| on_open.call((item.clone(), album.clone()))
                },
            }
            div { class: "shared-meta",
                if !item.sender_name.trim().is_empty() {
                    strong { "{item.sender_name}" }
                } else {
                    strong {
                        if item.is_video {
                            "Vídeo"
                        } else if item.is_photo {
                            "Foto"
                        } else {
                            "Mídia"
                        }
                    }
                }
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

#[component]
pub(super) fn MediaPanel(
    panel_id: String,
    chat_id: i64,
    title: String,
    subtitle: String,
    items: Vec<TdlibSharedMediaItem>,
    loading: bool,
    error: Option<String>,
    empty_title: String,
    empty_message: String,
    group_albums: bool,
    bulk_download: bool,
    bulk_disabled: bool,
    on_close: EventHandler<()>,
    on_open: EventHandler<(TdlibSharedMediaItem, Vec<LightboxMedia>)>,
) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let mut bulk_saving = use_signal(|| false);
    let mut bulk_progress = use_signal(|| None::<(usize, usize)>);
    let mut bulk_error = use_signal(|| None::<String>);
    let mut saving_album = use_signal(|| None::<String>);

    let count = items.len();
    let download_batch = TelegramSaveBatch {
        id: format!(
            "telegram-user-{chat_id}-{}",
            items
                .first()
                .map(|item| item.sender_id.as_str())
                .unwrap_or("all")
        ),
        title: format!("{title} · {subtitle}"),
        total: count,
    };
    let subtitle_line = match count {
        0 => subtitle,
        1 => format!("{subtitle} · 1 mídia"),
        count => format!("{subtitle} · {count} mídias"),
    };
    // Group the items once so opening any item that belongs to an album can
    // feed the lightbox carousel.
    let album_groups: HashMap<String, Vec<LightboxMedia>> = {
        let mut groups: HashMap<String, Vec<LightboxMedia>> = HashMap::new();
        for item in &items {
            let Some(group) = item.grouped_id.as_deref().filter(|group| !group.is_empty()) else {
                continue;
            };
            groups
                .entry(group.to_string())
                .or_default()
                .push(LightboxMedia::from_shared(item));
        }
        groups
    };
    let groups = group_media_by_album(&items);
    let bulk_items = items.clone();
    let bulk_error_message = bulk_error();

    // Focus the panel so Escape closes it without leaking a global listener.
    let focus_id = panel_id.clone();
    use_effect(move || {
        let focus_id = focus_id.clone();
        spawn(async move {
            let id = serde_json::to_string(&focus_id).unwrap_or_else(|_| "\"\"".to_string());
            let script = format!(
                r#"
                const panel = document.getElementById({id});
                if (!panel) {{ return; }}
                panel.focus();
                panel.addEventListener("keydown", (event) => {{
                    if (event.key === "Escape") {{
                        event.preventDefault();
                        dioxus.send("close");
                    }}
                }});
                "#
            );
            let mut eval = document::eval(&script);
            while let Ok(command) = eval.recv::<String>().await {
                if command == "close" {
                    on_close.call(());
                    break;
                }
            }
        });
    });

    rsx! {
        div { class: "media-panel-backdrop", onclick: move |_| on_close.call(()),
            section {
                id: "{panel_id}",
                class: "media-panel",
                tabindex: "-1",
                onclick: move |event| event.stop_propagation(),
                header { class: "media-panel-header",
                    div {
                        strong { "{title}" }
                        span { class: "hint", "{subtitle_line}" }
                    }
                    div { class: "media-panel-actions",
                        if bulk_download && !items.is_empty() {
                            button {
                                class: "icon-button",
                                title: if bulk_disabled {
                                    "Aguarde o carregamento das mídias"
                                } else {
                                    "Baixar todas as mídias"
                                },
                                aria_label: "Baixar todas as mídias",
                                disabled: bulk_disabled || bulk_saving() || saving_album().is_some(),
                                onclick: {
                                    let services = services.clone();
                                    let items = bulk_items.clone();
                                    let batch = download_batch.clone();
                                    move |_| {
                                        if bulk_saving() || bulk_disabled {
                                            return;
                                        }
                                        bulk_saving.set(true);
                                        bulk_progress.set(None);
                                        bulk_error.set(None);
                                        let services = services.clone();
                                        let items = items.clone();
                                        let batch = batch.clone();
                                        spawn(async move {
                                            let dest_dir = pick_save_folder("Salvar todas as mídias como")
                                                .await
                                                .map(|path| path.to_string_lossy().to_string());
                                            if dest_dir.is_none() {
                                                bulk_progress.set(None);
                                                bulk_saving.set(false);
                                                return;
                                            }
                                            match save_media_batch(
                                                &services,
                                                ui,
                                                chat_id,
                                                &items,
                                                Some(&batch),
                                                bulk_progress,
                                                dest_dir,
                                            )
                                            .await
                                            {
                                                Ok(()) => {}
                                                Err(error) => bulk_error.set(Some(error.to_string())),
                                            }
                                            bulk_progress.set(None);
                                            bulk_saving.set(false);
                                        });
                                    }
                                },
                                Icon { name: IconName::Download, size: 16, class: "icon" }
                                if bulk_saving() {
                                    if let Some((done, total)) = bulk_progress() {
                                        span { class: "hint", "{done}/{total}" }
                                    }
                                }
                            }
                        }
                        button {
                            class: "btn small ghost",
                            onclick: move |_| on_close.call(()),
                            "Fechar"
                        }
                    }
                }
                if let Some(error) = error {
                    p { class: "error-banner", "{error}" }
                }
                if let Some(error) = bulk_error_message {
                    p { class: "error-banner", "{error}" }
                }
                if loading {
                    div { class: "shared-scroll",
                        div { class: "shared-grid",
                            for index in 0..9 {
                                div {
                                    key: "skeleton-{index}",
                                    class: "shared-item shared-skeleton",
                                    div { class: "skeleton-thumb" }
                                    div { class: "shared-meta",
                                        div { class: "skeleton-line" }
                                        div { class: "skeleton-line short" }
                                    }
                                }
                            }
                        }
                    }
                } else if items.is_empty() {
                    div { class: "empty-state",
                        div { class: "empty-card",
                            div { class: "empty-icon", "▣" }
                            h2 { "{empty_title}" }
                            p { "{empty_message}" }
                        }
                    }
                } else {
                    div { class: "shared-scroll",
                        if group_albums {
                            for (index, group) in groups.iter().enumerate() {
                                if let Some(album_id) =
                                    group.album_id.as_ref().filter(|_| group.items.len() > 1)
                                {
                                    section {
                                        key: "album-{album_id}",
                                        class: "shared-album",
                                        header { class: "shared-album-header",
                                            div {
                                                strong { "Álbum" }
                                                span { class: "hint",
                                                    "{group.items.len()} mídias · {format_timestamp(group.items[0].date)}"
                                                }
                                            }
                                            button {
                                                class: "icon-button",
                                                title: if bulk_disabled {
                                                    "Aguarde o carregamento das mídias"
                                                } else {
                                                    "Baixar álbum"
                                                },
                                                aria_label: "Baixar álbum",
                                                disabled: bulk_disabled
                                                    || saving_album().is_some()
                                                    || bulk_saving(),
                                                onclick: {
                                                    let album_id = album_id.clone();
                                                    let group_items = group.items.clone();
                                                    let services = services.clone();
                                                    let album_batch = TelegramSaveBatch {
                                                        id: format!(
                                                            "{}-album-{album_id}",
                                                            download_batch.id
                                                        ),
                                                        title: format!(
                                                            "{} · álbum",
                                                            download_batch.title
                                                        ),
                                                        total: group_items.len(),
                                                    };
                                                    move |_| {
                                                        if bulk_disabled
                                                            || saving_album().is_some()
                                                            || bulk_saving()
                                                        {
                                                            return;
                                                        }
                                                        saving_album.set(Some(album_id.clone()));
                                                        bulk_progress.set(None);
                                                        bulk_error.set(None);
                                                        let services = services.clone();
                                                        let group_items = group_items.clone();
                                                        let album_batch = album_batch.clone();
                                                        spawn(async move {
                                                            let dest_dir =
                                                                pick_save_folder("Salvar álbum como")
                                                                    .await
                                                                    .map(|path| {
                                                                        path.to_string_lossy()
                                                                            .to_string()
                                                                    });
                                                            if dest_dir.is_none() {
                                                                bulk_progress.set(None);
                                                                saving_album.set(None);
                                                                return;
                                                            }
                                                            match save_media_batch(
                                                                &services,
                                                                ui,
                                                                chat_id,
                                                                &group_items,
                                                                Some(&album_batch),
                                                                bulk_progress,
                                                                dest_dir,
                                                            )
                                                            .await
                                                            {
                                                                Ok(()) => {}
                                                                Err(error) => bulk_error.set(Some(error.to_string())),
                                                            }
                                                            bulk_progress.set(None);
                                                            saving_album.set(None);
                                                        });
                                                    }
                                                },
                                                Icon { name: IconName::Download, size: 15, class: "icon" }
                                            }
                                        }
                                        div { class: "shared-grid",
                                            for item in group.items.iter().cloned() {
                                                {
                                                    let album = lightbox_album_for(&item, &album_groups);
                                                    rsx! {
                                                        SharedMediaCard {
                                                            key: "{item.id}",
                                                            chat_id,
                                                            item,
                                                            album,
                                                            on_open,
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else {
                                    div { key: "singles-{index}", class: "shared-grid",
                                        for item in group.items.iter().cloned() {
                                            {
                                                let album = lightbox_album_for(&item, &album_groups);
                                                rsx! {
                                                    SharedMediaCard {
                                                        key: "{item.id}",
                                                        chat_id,
                                                        item,
                                                        album,
                                                        on_open,
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        } else {
                            div { class: "shared-grid",
                                for item in items.iter().cloned() {
                                    {
                                        let album = lightbox_album_for(&item, &album_groups);
                                        rsx! {
                                            SharedMediaCard {
                                                key: "{item.id}",
                                                chat_id,
                                                item,
                                                album,
                                                on_open,
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        if bulk_disabled {
                            div { class: "shared-loading-more",
                                div { class: "spinner" }
                                span { "Carregando mais mídias..." }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub(super) fn MediaLightbox(state: LightboxState, on_close: EventHandler<()>) -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let save_services = services.clone();
    let resource_state = state.clone();
    let resource = use_resource(move || {
        let services = services.clone();
        let state = resource_state.clone();
        async move {
            if let Some((_chat, message)) =
                crate::twitter_fake::find_twitter_fake_message_by_numeric(
                    &services.settings,
                    state.chat_id,
                    state.message_id,
                )
            {
                let url = message.url.clone();
                let mime_type = if message.is_video {
                    Some("video/mp4".to_string())
                } else {
                    Some("image/jpeg".to_string())
                };
                return Ok(plasma_telegram::NativePlaybackSource {
                    success: url.is_some(),
                    kind: if message.is_video {
                        "video".into()
                    } else {
                        "photo".into()
                    },
                    url,
                    file_path: None,
                    mime_type,
                    cache_state: "remote".into(),
                    total_bytes: message.media_size,
                    downloaded_bytes: None,
                    file_name: None,
                    error: if message.url.is_none() {
                        Some("Mídia sem URL.".into())
                    } else {
                        None
                    },
                });
            }
            services
                .telegram
                .prepare_playback(state.chat_id, state.message_id)
                .await
        }
    });
    let mut saving = use_signal(|| false);
    let mut saving_album = use_signal(|| false);
    let mut save_error = use_signal(|| None::<String>);

    let (source, mime_type, total_bytes, load_error) = match &*resource.read_unchecked() {
        Some(Ok(playback)) if playback.success => (
            playback.url.clone(),
            playback.mime_type.clone(),
            playback.total_bytes,
            None,
        ),
        Some(Ok(playback)) => (
            None,
            playback.mime_type.clone(),
            playback.total_bytes,
            playback
                .error
                .clone()
                .or_else(|| Some("Não foi possível preparar a mídia.".to_string())),
        ),
        Some(Err(error)) => (None, None, None, Some(error.to_string())),
        None => (None, None, None, None),
    };
    let save_error_message = save_error();
    let has_album = state.album.len() > 1;
    let subtitle = lightbox_subtitle(&state);
    let meta_parts = lightbox_meta_parts(&state, mime_type.as_deref(), total_bytes);
    let previous_album = state.previous_album();
    let next_album = state.next_album();
    let frame_class = if has_album {
        "lightbox-frame has-album"
    } else {
        "lightbox-frame"
    };
    let stage_class = if state.is_video {
        "lightbox-stage is-video"
    } else if state.is_photo {
        "lightbox-stage is-photo"
    } else {
        "lightbox-stage"
    };

    rsx! {
        div { class: "lightbox-backdrop", onclick: move |_| on_close.call(()),
            div {
                class: "{frame_class}",
                role: "dialog",
                aria_modal: "true",
                aria_label: "Visualizar mídia",
                onclick: move |event| event.stop_propagation(),
                header { class: "lightbox-header",
                    div { class: "lightbox-title",
                        strong { "{state.title}" }
                        if !subtitle.is_empty() {
                            span { class: "hint", "{subtitle}" }
                        }
                    }
                    div { class: "lightbox-actions",
                        button {
                            class: "icon-button",
                            title: "Baixar mídia",
                            aria_label: "Baixar mídia",
                            disabled: saving() || saving_album(),
                            onclick: {
                                let services = save_services.clone();
                                move |_| {
                                    if saving() || saving_album() {
                                        return;
                                    }
                                    saving.set(true);
                                    save_error.set(None);
                                    let services = services.clone();
                                    let chat_id = state.chat_id;
                                    let message_id = state.message_id;
                                    spawn(async move {
                                        let dest_dir = pick_save_folder("Salvar mídia como")
                                            .await
                                            .map(|path| path.to_string_lossy().to_string());
                                        if dest_dir.is_none() {
                                            saving.set(false);
                                            return;
                                        }
                                        match save_telegram_media(
                                            &services,
                                            ui,
                                            chat_id,
                                            message_id,
                                            format!("Mídia {message_id}"),
                                            None,
                                            None,
                                            dest_dir,
                                        )
                                        .await
                                        {
                                            Ok(result) if result.success => {
                                                let _ = services.enforce_media_cache_limit().await;
                                            }
                                            Ok(result) => save_error.set(
                                                result.error.or_else(|| {
                                                    Some("Não foi possível salvar a mídia.".into())
                                                }),
                                            ),
                                            Err(error) => save_error.set(Some(error.to_string())),
                                        }
                                        saving.set(false);
                                    });
                                }
                            },
                            Icon { name: IconName::FileDownload, size: 16, class: "icon" }
                        }
                        if has_album {
                            button {
                                class: "icon-button",
                                title: "Baixar álbum",
                                aria_label: "Baixar álbum",
                                disabled: saving() || saving_album(),
                                onclick: {
                                    let services = save_services.clone();
                                    let album = state.album.clone();
                                    let batch = TelegramSaveBatch {
                                        id: format!(
                                            "telegram-album-{}-{}",
                                            state.chat_id,
                                            album
                                                .first()
                                                .map(|item| item.message_id)
                                                .unwrap_or(state.message_id)
                                        ),
                                        title: format!("Álbum · {}", state.title),
                                        total: album.len(),
                                    };
                                    move |_| {
                                        if saving() || saving_album() {
                                            return;
                                        }
                                        saving_album.set(true);
                                        save_error.set(None);
                                        let services = services.clone();
                                        let chat_id = state.chat_id;
                                        let album = album.clone();
                                        let batch = batch.clone();
                                        spawn(async move {
                                            let dest_dir = pick_save_folder("Salvar álbum como")
                                                .await
                                                .map(|path| path.to_string_lossy().to_string());
                                            if dest_dir.is_none() {
                                                saving_album.set(false);
                                                return;
                                            };
                                            let mut failure = None;
                                            for (index, item) in album.iter().enumerate() {
                                                let result = save_telegram_media(
                                                    &services,
                                                    ui,
                                                    chat_id,
                                                    item.message_id,
                                                    format!("Mídia {}", item.message_id),
                                                    None,
                                                    Some((&batch, index + 1)),
                                                    dest_dir.clone(),
                                                )
                                                .await;
                                                match result {
                                                    Ok(result) if result.success => {}
                                                    Ok(result) => {
                                                        failure = result.error.or_else(|| {
                                                            Some("Não foi possível salvar o álbum.".into())
                                                        });
                                                        break;
                                                    }
                                                    Err(error) => {
                                                        failure = Some(error.to_string());
                                                        break;
                                                    }
                                                }
                                            }
                                            if failure.is_none() {
                                                let _ = services.enforce_media_cache_limit().await;
                                            }
                                            if let Some(error) = failure {
                                                save_error.set(Some(error));
                                            }
                                            saving_album.set(false);
                                        });
                                    }
                                },
                                Icon { name: IconName::Download, size: 16, class: "icon" }
                            }
                        }
                        button {
                            class: "icon-button lightbox-close",
                            title: "Fechar",
                            aria_label: "Fechar",
                            onclick: move |_| on_close.call(()),
                            Icon { name: IconName::Close, size: 16, class: "icon" }
                        }
                    }
                }
                div {
                    class: "{stage_class}",
                    onclick: move |_| on_close.call(()),
                    if let Some(error) = load_error {
                        p { class: "error-banner", "{error}" }
                    } else if let Some(url) = source {
                        if state.is_photo {
                            img {
                                class: "lightbox-ambient",
                                src: "{url}",
                                alt: "",
                                aria_hidden: "true",
                            }
                        }
                        div {
                            class: "lightbox-media-shell",
                            onclick: move |event| event.stop_propagation(),
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
                        }
                    } else {
                        div {
                            class: "timeline-loading lightbox-loading",
                            onclick: move |event| event.stop_propagation(),
                            div { class: "spinner" }
                            span { "Preparando mídia..." }
                        }
                    }
                    if let Some(error) = save_error_message {
                        p {
                            class: "error-banner lightbox-save-error",
                            onclick: move |event| event.stop_propagation(),
                            "{error}"
                        }
                    }
                    if let Some(previous) = previous_album {
                        button {
                            class: "lightbox-nav lightbox-nav-prev",
                            title: "Anterior",
                            aria_label: "Mídia anterior",
                            onclick: move |event| {
                                event.stop_propagation();
                                ui.panels.lightbox.set(Some(previous.clone()));
                            },
                            Icon { name: IconName::ChevronLeft, size: 22, class: "icon" }
                        }
                    }
                    if let Some(next) = next_album {
                        button {
                            class: "lightbox-nav lightbox-nav-next",
                            title: "Próxima",
                            aria_label: "Próxima mídia",
                            onclick: move |event| {
                                event.stop_propagation();
                                ui.panels.lightbox.set(Some(next.clone()));
                            },
                            Icon { name: IconName::ChevronRight, size: 22, class: "icon" }
                        }
                    }
                }
                div {
                    class: "lightbox-bottom",
                    onclick: move |event| event.stop_propagation(),
                    if has_album {
                        div { class: "lightbox-carousel-row",
                            div {
                                class: "lightbox-carousel",
                                role: "group",
                                aria_label: "Mídias do álbum",
                                for (index, item) in state.album.iter().cloned().enumerate() {
                                    {
                                        let is_active = item.message_id == state.message_id;
                                        let class = if is_active {
                                            "lightbox-carousel-thumb active"
                                        } else {
                                            "lightbox-carousel-thumb"
                                        };
                                        let next_state = state.with_album_index(index);
                                        let active_message_id = state.message_id;
                                        rsx! {
                                            div {
                                                key: "{item.message_id}",
                                                class: "{class}",
                                                MessageThumbnail {
                                                    chat_id: state.chat_id,
                                                    message_id: item.message_id,
                                                    is_video: item.is_video,
                                                    on_open: {
                                                        let next_state = next_state.clone();
                                                        move |_| {
                                                            if let Some(next_state) = next_state.clone() {
                                                                if next_state.message_id != active_message_id {
                                                                    ui.panels.lightbox.set(Some(next_state));
                                                                }
                                                            }
                                                        }
                                                    },
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !meta_parts.is_empty() {
                        footer { class: "lightbox-footer",
                            for part in meta_parts.iter() {
                                span { class: "lightbox-meta", "{part}" }
                            }
                        }
                    }
                }
            }
        }
    }
}

fn lightbox_subtitle(state: &LightboxState) -> String {
    let mut parts = Vec::new();
    let date = format_timestamp(state.date);
    if !date.is_empty() {
        parts.push(date);
    }
    if let Some((position, total)) = state.album_position() {
        parts.push(format!("{position} de {total}"));
        parts.push("← →".to_string());
    }
    parts.join(" · ")
}

fn lightbox_meta_parts(
    state: &LightboxState,
    mime_type: Option<&str>,
    total_bytes: Option<i64>,
) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(duration) = state.video_duration.filter(|value| *value > 0) {
        parts.push(format_duration(duration));
    }
    if let Some(bytes) = total_bytes.or(state.media_size).filter(|value| *value > 0) {
        parts.push(format_bytes(bytes));
    }
    if let Some(mime) = mime_type.map(str::trim).filter(|value| !value.is_empty()) {
        parts.push(mime.to_string());
    }
    parts
}

fn format_duration(seconds: i32) -> String {
    let seconds = seconds.max(0) as u32;
    let minutes = seconds / 60;
    let rest = seconds % 60;
    format!("{minutes}:{rest:02}")
}

pub(super) fn format_bytes(bytes: i64) -> String {
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

    #[test]
    fn groups_album_items_across_page_boundaries() {
        fn media(id: i64, album: Option<&str>) -> TdlibSharedMediaItem {
            TdlibSharedMediaItem {
                id,
                date: 0,
                sender_id: "42".into(),
                sender_name: String::new(),
                has_media: true,
                is_photo: true,
                is_video: false,
                grouped_id: album.map(str::to_string),
                media_size: None,
            }
        }

        let items = vec![
            media(1, None),
            media(2, Some("a")),
            media(3, Some("a")),
            media(4, None),
            media(5, Some("b")),
            media(6, Some("a")),
        ];
        let groups = group_media_by_album(&items);
        assert_eq!(groups.len(), 4);
        assert_eq!(groups[1].album_id.as_deref(), Some("a"));
        assert_eq!(
            groups[1]
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            vec![2, 3, 6]
        );
        assert_eq!(groups[2].items[0].id, 4);
        assert_eq!(groups[3].album_id.as_deref(), Some("b"));
    }
}
