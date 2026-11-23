//! Mass download panel for one chat or topic.

use crate::icons::{Icon, IconName};
use crate::state::{AppServices, MassDownloadProgress, UiState};
use crate::views::select::{Select, SelectOption};
use dioxus::prelude::*;
use plasma_core::AppEvent;
use plasma_telegram::{
    TdlibChatInfo, TdlibDownloadItem, TdlibForumTopicInfo, TdlibMassDownloadRequest,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::state::{refresh_topics_with_new_media, topic_download_key};

use super::composer::pick_download_folder;
use super::conversation::parse_chat_id;
use super::media::format_bytes;

/// Shared abort flag for Twitter/X gallery mass downloads. The download loop
/// runs on the Tokio runtime; the UI only flips this and cancels the active id.
static TWITTER_MASS_ABORTED: AtomicBool = AtomicBool::new(false);
static TWITTER_MASS_ACTIVE_ID: Mutex<Option<String>> = Mutex::new(None);

#[component]
pub(super) fn MassDownloadPanel(
    chat: TdlibChatInfo,
    topics: Vec<TdlibForumTopicInfo>,
    initial_topic: Option<TdlibForumTopicInfo>,
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
    let initial_topic_id = initial_topic
        .as_ref()
        .map(|topic| topic.id.to_string())
        .unwrap_or_else(|| "all".to_string());
    let mut selected_topic_id = use_signal(|| initial_topic_id.clone());
    use_effect(use_reactive!(|initial_topic_id| {
        selected_topic_id.set(initial_topic_id);
    }));
    let mut split_by_user = use_signal(|| false);
    let mut split_by_album = use_signal(|| false);
    let mut album_split_mode = use_signal(|| "separator".to_string());
    let mut panel_error = use_signal(|| None::<String>);

    let topics_with_new_media = ui.conversation.topics_with_new_media();
    let topic_options = {
        let mut options = vec![SelectOption::new("all", "Todos os tópicos")];
        options.extend(topics.iter().map(|topic| {
            let key = topic_download_key(topic.id, &topic.kind);
            let title = if topics_with_new_media.contains(&key) {
                format!("• {} (mídia nova)", topic.title)
            } else {
                topic.title.clone()
            };
            SelectOption::new(topic.id.to_string(), title)
        }));
        options
    };

    let running = ui.transfers.mass_download_running();
    let stopping = ui.transfers.mass_download_stopping();
    let progress = ui.transfers.mass_download_progress();

    let chat_for_start = chat.clone();
    let topics_for_start = topics.clone();
    let start_services = services.clone();
    let start = move |_| {
        if ui.transfers.mass_download_running() {
            return;
        }
        let folder_path = folder().trim().to_string();
        if folder_path.is_empty() {
            panel_error.set(Some("Escolha uma pasta de destino.".to_string()));
            return;
        }

        if crate::twitter_fake::is_twitter_fake_chat_id(&chat_for_start.id) {
            ui.transfers.mass_download_running.set(true);
            ui.transfers.mass_download_stopping.set(false);
            ui.transfers.mass_download_progress.set(None);
            panel_error.set(None);
            TWITTER_MASS_ABORTED.store(false, Ordering::Release);
            if let Ok(mut guard) = TWITTER_MASS_ACTIVE_ID.lock() {
                *guard = None;
            }
            let services = start_services.clone();
            let chat = chat_for_start.clone();
            spawn(async move {
                // Schedules work on Tokio and returns. Running/stopping flags are
                // cleared by the TelegramDownloadProgress "Concluído"/"Interrompido"
                // event so a cross-runtime JoinHandle await cannot leave the button stuck.
                if let Some(error) =
                    start_twitter_fake_mass_download(services, chat, folder_path).await
                {
                    panel_error.set(Some(error));
                    ui.transfers.mass_download_running.set(false);
                    ui.transfers.mass_download_stopping.set(false);
                }
            });
            return;
        }

        let Some(chat_id) = parse_chat_id(&chat_for_start) else {
            panel_error.set(Some("Chat inválido para download em massa.".to_string()));
            return;
        };
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

        ui.transfers.mass_download_running.set(true);
        ui.transfers.mass_download_stopping.set(false);
        ui.transfers.mass_download_progress.set(None);
        panel_error.set(None);
        let services = start_services.clone();
        let topic_for_refresh = topic.clone();
        let chat_id_for_refresh = chat_id;
        spawn(async move {
            let outcome = services.telegram.start_mass_download(request).await;
            match outcome {
                Ok(result) if result.success => {
                    if let Some(topic) = topic_for_refresh {
                        let key = topic_download_key(topic.id, &topic.kind);
                        ui.conversation.topics_with_new_media.write().remove(&key);
                    }
                    refresh_topics_with_new_media(services.clone(), ui, chat_id_for_refresh);
                }
                Ok(result) => panel_error.set(Some(
                    result
                        .error
                        .unwrap_or_else(|| "Falha no download em massa.".to_string()),
                )),
                Err(error) => panel_error.set(Some(error.to_string())),
            }
            ui.transfers.mass_download_running.set(false);
            ui.transfers.mass_download_stopping.set(false);
        });
    };

    let stop_services = services.clone();
    let stop_chat = chat.clone();
    let stop = move |_| {
        if !ui.transfers.mass_download_running() || ui.transfers.mass_download_stopping() {
            return;
        }
        ui.transfers.mass_download_stopping.set(true);
        if crate::twitter_fake::is_twitter_fake_chat_id(&stop_chat.id) {
            TWITTER_MASS_ABORTED.store(true, Ordering::Release);
            if let Ok(guard) = TWITTER_MASS_ACTIVE_ID.lock() {
                if let Some(id) = guard.as_ref() {
                    let _ = plasma_download::cancel_download(id);
                }
            }
            return;
        }
        let services = stop_services.clone();
        spawn(async move {
            if let Err(error) = services.telegram.stop_mass_download().await {
                panel_error.set(Some(error.to_string()));
                ui.transfers.mass_download_stopping.set(false);
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

    let scanning = progress
        .as_ref()
        .is_some_and(|progress| progress.is_scanning);
    let percent = progress
        .as_ref()
        .filter(|progress| !progress.is_scanning && progress.total > 0)
        .map(|progress| (progress.downloaded / progress.total as f64 * 100.0).clamp(0.0, 100.0))
        .unwrap_or(0.0);
    let percent_label = format!("{percent:.0}%");
    let bar_style = if scanning {
        String::new()
    } else {
        format!("width: {percent:.1}%")
    };
    let folder_path = folder();
    let (folder_name, folder_detail) = folder_caption(&folder_path);
    let folder_empty = folder_path.trim().is_empty();

    let _ = &on_close;
    rsx! {
        section {
            class: "mass-panel-docked",
            header { class: "mass-panel-header",
                div { class: "mass-panel-mark",
                    Icon { name: IconName::CloudDownload, size: 18, class: "icon" }
                }
                div { class: "mass-panel-title",
                    strong { "Download em massa" }
                    p { class: "hint", "{chat.title}" }
                }
            }
            div { class: "mass-panel-scroll",
                if let Some(message) = panel_error() {
                    p { class: "error-banner", "{message}" }
                }
                section { class: "mass-card",
                    span { class: "mass-card-label", "Pasta de destino" }
                    div { class: "mass-folder",
                        span { class: "mass-folder-icon",
                            Icon { name: IconName::Folder, size: 16, class: "icon" }
                        }
                        span { class: "mass-folder-copy",
                            strong {
                                class: if folder_empty { "is-empty" } else { "" },
                                "{folder_name}"
                            }
                            span { "{folder_detail}" }
                        }
                        if cfg!(target_os = "android") {
                            button {
                                class: "btn small ghost",
                                disabled: running,
                                onclick: choose_folder,
                                title: "Usa a pasta Downloads do app",
                                "App"
                            }
                        } else {
                            button {
                                class: "btn small soft",
                                disabled: running,
                                onclick: choose_folder,
                                "Escolher"
                            }
                        }
                    }
                    if cfg!(target_os = "android") {
                        span { class: "hint",
                            "No Android os arquivos vão para a pasta Downloads do app."
                        }
                    }
                }
                if !topics.is_empty() {
                    section { class: "mass-card",
                        span { class: "mass-card-label", "Tópico" }
                        Select {
                            value: selected_topic_id(),
                            options: topic_options.clone(),
                            placeholder: "Todos os tópicos",
                            disabled: running,
                            on_change: move |value| selected_topic_id.set(value),
                        }
                    }
                }
                section { class: "mass-card mass-options",
                    span { class: "mass-card-label", "Organização" }
                    div { class: "mass-toggles",
                        label {
                            class: if running { "mass-toggle is-disabled" } else { "mass-toggle" },
                            span { class: "mass-toggle-copy",
                                strong { "Por usuário" }
                                span { "Uma subpasta para cada remetente" }
                            }
                            span { class: "mass-switch",
                                input {
                                    r#type: "checkbox",
                                    checked: split_by_user(),
                                    disabled: running,
                                    onchange: move |event| split_by_user.set(event.checked()),
                                }
                                span { class: "mass-switch-track" }
                            }
                        }
                        label {
                            class: if running { "mass-toggle is-disabled" } else { "mass-toggle" },
                            span { class: "mass-toggle-copy",
                                strong { "Por álbum" }
                                span { "Separa as mídias de cada álbum" }
                            }
                            span { class: "mass-switch",
                                input {
                                    r#type: "checkbox",
                                    checked: split_by_album(),
                                    disabled: running,
                                    onchange: move |event| split_by_album.set(event.checked()),
                                }
                                span { class: "mass-switch-track" }
                            }
                        }
                    }
                    if split_by_album() {
                        div { class: "segmented mass-album-mode",
                            button {
                                class: if album_split_mode() == "separator" { "active" } else { "" },
                                disabled: running,
                                onclick: move |_| album_split_mode.set("separator".to_string()),
                                "Separador"
                            }
                            button {
                                class: if album_split_mode() == "comment" { "active" } else { "" },
                                disabled: running,
                                onclick: move |_| album_split_mode.set("comment".to_string()),
                                "Comentário"
                            }
                        }
                    }
                }
                if let Some(progress) = progress {
                    section { class: "mass-card mass-progress",
                        div { class: "mass-progress-top",
                            span { class: "hint", "{progress.current_file}" }
                            strong { class: "mass-progress-count tabular",
                                if progress.is_scanning {
                                    "Escaneando…"
                                } else {
                                    "{progress.downloaded.floor()} / {progress.total} · {percent_label}"
                                }
                            }
                        }
                        div { class: "progress-track",
                            div {
                                class: if scanning { "progress-fill indeterminate" } else { "progress-fill" },
                                style: "{bar_style}",
                            }
                        }
                        if !progress.is_scanning && progress.total > 0 {
                            div { class: "downloads-stats",
                                span { class: "pill good", "OK {mass_status_count(&progress, \"completed\")}" }
                                span { class: "pill", "Pulados {mass_status_count(&progress, \"skipped\")}" }
                                if mass_status_count(&progress, "failed") > 0 {
                                    span { class: "pill failed", "Falhas {mass_status_count(&progress, \"failed\")}" }
                                }
                            }
                        }
                        if !progress.items.is_empty() {
                            div { class: "mass-items-log",
                                for item in progress.items.iter().rev().take(500) {
                                    div {
                                        key: "{item.message_id}",
                                        class: mass_item_class(&item.status),
                                        div { class: "download-row-title",
                                            strong { "{item.name}" }
                                            if item.size > 0 {
                                                span { class: "hint", "{format_bytes(item.size)}" }
                                            }
                                        }
                                        span { class: mass_status_class(&item.status),
                                            "{mass_status_label(&item.status)}"
                                            if item.status == "downloading" {
                                                " {item.progress}%"
                                            }
                                        }
                                    }
                                }
                                if progress.items.len() > 500 {
                                    p { class: "hint",
                                        "Mostrando os 500 mais recentes de {progress.items.len()} arquivos."
                                    }
                                }
                            }
                        }
                    }
                }
            }
            div { class: "mass-actions",
                if running {
                    button {
                        class: "btn danger",
                        disabled: stopping,
                        onclick: stop,
                        Icon { name: IconName::Stop, size: 15, class: "icon" }
                        if stopping { "Parando…" } else { "Parar download" }
                    }
                } else {
                    button {
                        class: "btn primary",
                        disabled: folder_empty,
                        onclick: start,
                        Icon { name: IconName::Download, size: 16, class: "icon" }
                        "Iniciar download em massa"
                    }
                }
            }
        }
    }
}

fn folder_caption(path: &str) -> (String, String) {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return (
            "Nenhuma pasta escolhida".to_string(),
            "Escolha onde salvar os arquivos".to_string(),
        );
    }
    let name = trimmed
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(trimmed)
        .to_string();
    (name, trimmed.to_string())
}

fn mass_item_class(status: &str) -> &'static str {
    match status {
        "downloading" => "mass-item-row is-active",
        "failed" => "mass-item-row is-failed",
        _ => "mass-item-row",
    }
}

fn mass_status_class(status: &str) -> &'static str {
    match status {
        "completed" => "pill good",
        "failed" | "stopped" => "pill failed",
        "downloading" => "pill busy",
        _ => "pill",
    }
}

async fn start_twitter_fake_mass_download(
    services: AppServices,
    chat: TdlibChatInfo,
    folder_path: String,
) -> Option<String> {
    use std::collections::HashMap;
    use std::path::PathBuf;

    let Some(fake) = crate::twitter_fake::get_twitter_fake_chat(&services.settings, &chat.id)
    else {
        return Some("Galeria Twitter/X não encontrada.".into());
    };
    let media = fake
        .messages
        .iter()
        .filter(|message| message.url.as_ref().is_some_and(|url| !url.is_empty()))
        .cloned()
        .collect::<Vec<_>>();
    let total = media.len();
    if total == 0 {
        return Some("Nenhuma mídia para baixar nesta galeria.".into());
    }

    let folder = PathBuf::from(&folder_path);
    if let Err(error) = std::fs::create_dir_all(&folder) {
        return Some(format!("Não foi possível criar a pasta: {error}"));
    }

    // `load_cookies` returns the raw Netscape file; CDN requests need a
    // normalized `Cookie` header (or none). Sending the file verbatim makes
    // reqwest reject the header and every item fails.
    let raw_cookies = services.twitter.load_cookies().await.unwrap_or_default();
    let cookie_header = plasma_twitter::normalize_cookie_input(Some(&raw_cookies))
        .ok()
        .flatten();

    let numeric_id = crate::twitter_fake::fake_chat_numeric_id(&chat.id);
    let events = services.events.clone();
    let env = services.env.clone();
    let runtime = services.telegram.runtime_handle();

    // Do not await this JoinHandle on the Dioxus executor — that can leave the
    // UI stuck on "Parar download" after the work already finished. Completion
    // is signaled via the final TelegramDownloadProgress event instead.
    runtime.spawn(async move {
        use std::time::{Duration, Instant};

        let mut items: Vec<TdlibDownloadItem> = Vec::with_capacity(total);
        let mut downloaded = 0.0f64;
        let mut failed = 0usize;
        let mut last_error: Option<String> = None;
        let mut last_publish = Instant::now()
            .checked_sub(Duration::from_secs(1))
            .unwrap_or_else(Instant::now);
        let publish_interval = Duration::from_millis(250);

        let publish = |downloaded: f64,
                       current_file: &str,
                       is_scanning: bool,
                       items: &[TdlibDownloadItem],
                       force: bool,
                       last_publish: &mut Instant| {
            if !force && last_publish.elapsed() < publish_interval {
                return;
            }
            publish_twitter_mass_progress(
                &events,
                numeric_id,
                total,
                downloaded,
                current_file,
                is_scanning,
                items,
            );
            *last_publish = Instant::now();
        };

        publish(
            downloaded,
            "Iniciando...",
            true,
            &items,
            true,
            &mut last_publish,
        );

        for (index, message) in media.into_iter().enumerate() {
            if TWITTER_MASS_ABORTED.load(Ordering::Acquire) {
                break;
            }
            let Some(url) = message.url.clone() else {
                continue;
            };
            let ext = if message.is_video { ".mp4" } else { ".jpg" };
            let file_name = format!("media_{}{ext}", message.id);
            let destination = folder.join(&file_name);
            let mut item = TdlibDownloadItem {
                name: file_name.clone(),
                message_id: message.id,
                status: "pending".into(),
                progress: 0,
                size: message.media_size.unwrap_or(0),
                file_path: Some(destination.to_string_lossy().to_string()),
                thumbnail_path: message.thumbnail_url.clone(),
            };

            if destination.exists() {
                item.status = "skipped".into();
                item.progress = 100;
                downloaded += 1.0;
                items.push(item);
                publish(
                    downloaded,
                    &file_name,
                    false,
                    &items,
                    false,
                    &mut last_publish,
                );
                if index % 32 == 0 {
                    tokio::task::yield_now().await;
                }
                continue;
            }

            item.status = "downloading".into();
            items.push(item.clone());
            let download_id = format!("twitter-mass-{numeric_id}-{}", message.id);
            if let Ok(mut guard) = TWITTER_MASS_ACTIVE_ID.lock() {
                *guard = Some(download_id.clone());
            }
            // Throttled: forcing every file floods the 256-slot event bus and
            // the terminal "Concluído" event can be lagged away.
            publish(
                downloaded,
                &file_name,
                false,
                &items,
                false,
                &mut last_publish,
            );

            let mut headers = HashMap::new();
            headers.insert("Referer".into(), "https://x.com/".into());
            headers.insert(
                "User-Agent".into(),
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36".into(),
            );
            if let Some(cookie) = cookie_header.as_ref() {
                headers.insert("Cookie".into(), cookie.clone());
            }

            let outcome = plasma_download::download_url_to_file(
                &env,
                &download_id,
                &url,
                &destination.to_string_lossy(),
                Some(headers),
            )
            .await;

            if let Ok(mut guard) = TWITTER_MASS_ACTIVE_ID.lock() {
                *guard = None;
            }

            if TWITTER_MASS_ABORTED.load(Ordering::Acquire) {
                item.status = "stopped".into();
                if let Some(last) = items.last_mut() {
                    *last = item;
                }
                break;
            }

            match outcome {
                Ok(()) => {
                    item.status = "completed".into();
                    item.progress = 100;
                    downloaded += 1.0;
                }
                Err(error) => {
                    failed += 1;
                    last_error = Some(error);
                    item.status = "failed".into();
                }
            }
            if let Some(last) = items.last_mut() {
                *last = item;
            }
            publish(
                downloaded,
                &file_name,
                false,
                &items,
                false,
                &mut last_publish,
            );
        }

        if let Ok(mut guard) = TWITTER_MASS_ACTIVE_ID.lock() {
            *guard = None;
        }
        let aborted = TWITTER_MASS_ABORTED.load(Ordering::Acquire);
        let summary = if aborted {
            format!("Interrompido ({downloaded:.0} de {total})")
        } else if failed > 0 {
            match last_error {
                Some(error) => format!("Concluído ({failed} falharam: {error})"),
                None => format!("Concluído ({failed} falharam)"),
            }
        } else {
            "Concluído".to_string()
        };
        // Lightweight terminal ping first so the UI releases "Parar download"
        // even if a later full payload is lagged on the event bus.
        publish_twitter_mass_progress(
            &events,
            numeric_id,
            total,
            downloaded,
            if aborted { "Interrompido" } else { "Concluído" },
            false,
            &[],
        );
        publish(
            downloaded,
            &summary,
            false,
            &items,
            true,
            &mut last_publish,
        );
    });

    None
}

fn publish_twitter_mass_progress(
    events: &plasma_core::EventBus,
    chat_id: i64,
    total: usize,
    downloaded: f64,
    current_file: impl Into<String>,
    is_scanning: bool,
    items: &[TdlibDownloadItem],
) {
    let items = items
        .iter()
        .filter_map(|item| serde_json::to_value(item).ok())
        .collect::<Vec<_>>();
    events.publish(AppEvent::TelegramDownloadProgress {
        chat_id,
        total,
        downloaded,
        current_file: current_file.into(),
        is_scanning,
        items,
    });
}

pub(super) fn mass_status_count(progress: &MassDownloadProgress, status: &str) -> usize {
    progress
        .items
        .iter()
        .filter(|item| item.status == status)
        .count()
}

pub(super) fn mass_status_label(status: &str) -> &str {
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
