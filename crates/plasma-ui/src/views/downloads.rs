//! Downloads queue, unified URL analysis and persisted history.

use crate::context::{ContextMenuState, ContextTarget};
use crate::downloads::{
    new_download_id, push_download, sanitize_filename, update_download, DownloadRecord,
};
use crate::state::{push_toast, AppServices, ToastKind, UiState};
use dioxus::prelude::*;
use plasma_extract::{FormatOption, MediaInfo};
use std::path::Path;

#[component]
pub fn DownloadsView() -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let mut url = use_signal(String::new);
    let mut file_name = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut analysis_error = use_signal(|| None::<String>);
    let mut analysis = use_signal(|| None::<MediaInfo>);
    let analyzing = use_signal(|| false);

    let mut shared_url = ui.shared_url;
    let shared_services = services.clone();
    use_effect(move || {
        if let Some(value) = shared_url() {
            shared_url.set(None);
            url.set(value.clone());
            analyze_current(
                shared_services.clone(),
                url,
                analysis,
                analyzing,
                analysis_error,
            );
        }
    });

    let download_dir = services.downloads.download_dir().display().to_string();
    let items = ui.downloads();
    let active = items.iter().filter(|item| item.is_active()).count();
    let completed = items
        .iter()
        .filter(|item| item.status == "completed")
        .count();
    let current_analysis = analysis();

    let analyze_enter = {
        let services = services.clone();
        move |event: KeyboardEvent| {
            if event.key() == Key::Enter {
                analyze_current(services.clone(), url, analysis, analyzing, analysis_error);
            }
        }
    };
    let direct_enter = {
        let services = services.clone();
        move |event: KeyboardEvent| {
            if event.key() == Key::Enter {
                submit_url(services.clone(), ui, url, file_name, error);
            }
        }
    };

    rsx! {
        section { class: "downloads-view",
            div { class: "downloads-inner",
                header { class: "downloads-heading",
                    div {
                        h1 { "Downloads" }
                        p { class: "muted",
                            "Analisador nativo para YouTube, TikTok, Instagram, Reddit e X, \
                             downloads diretos por HTTPS, retomada e histórico no SQLite."
                        }
                    }
                    div { class: "downloads-stats",
                        span { class: "pill", "{active} ativo(s)" }
                        span { class: "pill", "{completed} concluído(s)" }
                    }
                }

                section { class: "download-card",
                    h2 { "Analisar link" }
                    p { class: "hint",
                        "Cole uma URL de post/vídeo. A plataforma é detectada automaticamente \
                         e os formatos disponíveis aparecem abaixo. Para um arquivo direto, \
                         use “Baixar URL”."
                    }
                    div { class: "download-form",
                        input {
                            class: "download-input",
                            r#type: "url",
                            placeholder: "https://youtube.com/watch?v=... ou link direto",
                            value: "{url}",
                            oninput: move |event| {
                                url.set(event.value());
                                analysis.set(None);
                                analysis_error.set(None);
                                error.set(None);
                            },
                            onkeydown: analyze_enter,
                        }
                        input {
                            class: "download-input download-name",
                            r#type: "text",
                            placeholder: "nome opcional (baixar direto)",
                            value: "{file_name}",
                            oninput: move |event| file_name.set(event.value()),
                            onkeydown: direct_enter,
                        }
                        div { class: "download-form-actions",
                            button {
                                class: "btn primary",
                                disabled: analyzing(),
                                onclick: {
                                    let services = services.clone();
                                    move |_| analyze_current(
                                        services.clone(),
                                        url,
                                        analysis,
                                        analyzing,
                                        analysis_error,
                                    )
                                },
                                if analyzing() { "Analisando..." } else { "Analisar" }
                            }
                            button {
                                class: "btn small ghost",
                                onclick: {
                                    let services = services.clone();
                                    move |_| submit_url(services.clone(), ui, url, file_name, error)
                                },
                                "Baixar URL"
                            }
                        }
                    }
                    if let Some(message) = error() {
                        p { class: "error-banner", "{message}" }
                    }
                    if let Some(message) = analysis_error() {
                        p { class: "error-banner", "{message}" }
                    }
                    p { class: "download-destination",
                        strong { "Pasta: " }
                        code { "{download_dir}" }
                    }
                }

                if let Some(info) = current_analysis {
                    div { class: "analysis-heading",
                        h2 { "Resultado da análise" }
                        button {
                            class: "btn small ghost",
                            onclick: move |_| analysis.set(None),
                            "Limpar"
                        }
                    }
                    AnalysisCard { info }
                }

                section { class: "download-card downloads-list-card",
                    div { class: "download-list-heading",
                        div {
                            h2 { "Fila e histórico" }
                            p { class: "hint", "Downloads recentes aparecem primeiro." }
                        }
                        button {
                            class: "btn small ghost",
                            disabled: items.iter().all(|item| item.is_active()),
                            onclick: move |_| {
                                let mut next = ui.downloads();
                                next.retain(|item| item.is_active());
                                ui.downloads.set(next.clone());
                                let downloads = services.downloads.clone();
                                dioxus::prelude::spawn(async move {
                                    let _ = downloads.save_history(next).await;
                                });
                                push_toast(ui.toasts, "Itens finalizados removidos do histórico.", ToastKind::Info);
                            },
                            "Limpar finalizados"
                        }
                    }
                    if items.is_empty() {
                        div { class: "downloads-empty",
                            div { class: "empty-icon", "↓" }
                            p { "Nenhum download ainda. Analise um link ou cole uma URL direta." }
                        }
                    } else {
                        div { class: "downloads-list",
                            for item in items.iter().rev().cloned() {
                                DownloadRow {
                                    key: "{item.id}",
                                    item,
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
fn AnalysisCard(info: MediaInfo) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let mut formats = info.formats.video.clone();
    formats.extend(info.formats.audio.clone());
    let thumbnail = info.thumbnail_url.clone();
    let platform = platform_label(Some(info.platform.as_str()));
    let has_formats = !formats.is_empty();

    rsx! {
        article { class: "twitter-result analysis-result",
            div { class: "twitter-result-head",
                if let Some(thumbnail) = thumbnail {
                    img { class: "twitter-thumb", src: "{thumbnail}", alt: "" }
                } else {
                    div {
                        class: "twitter-thumb fallback",
                        style: "--thumb-hue: {info.thumb_hue}; --thumb-hue2: {info.thumb_hue2};",
                    }
                }
                div { class: "twitter-result-copy",
                    h2 { "{info.title}" }
                    p { class: "muted",
                        span { class: "pill", "{platform}" }
                        span { " {info.author}" }
                        if let Some(full) = info.author_full.as_ref() {
                            span { " · {full}" }
                        }
                        if !info.duration.trim().is_empty() {
                            span { " · {info.duration}" }
                        }
                        if let Some(views) = info.views.as_ref() {
                            span { " · {views} visualizações" }
                        }
                        if let Some(likes) = info.likes.as_ref() {
                            span { " · {likes} curtidas" }
                        }
                    }
                    a {
                        class: "twitter-original",
                        href: "{info.original_url}",
                        target: "_blank",
                        "Abrir original"
                    }
                }
            }
            if !has_formats {
                p { class: "hint", "Nenhum formato de mídia foi encontrado." }
            } else {
                div { class: "twitter-formats",
                    for option in formats {
                        {
                            let is_audio = info.formats.audio.iter().any(|audio| audio.id == option.id);
                            let services_for_click = services.clone();
                            let info_for_click = info.clone();
                            let option_for_click = option.clone();
                            rsx! {
                                div { class: "twitter-format", key: "{option.id}",
                                    div { class: "twitter-format-copy",
                                        strong {
                                            "{option.label}"
                                            if option.best.unwrap_or(false) {
                                                span { class: "pill", "melhor" }
                                            }
                                            if !is_audio
                                                && !option.has_audio
                                                && !is_image_option(&option)
                                            {
                                                span { class: "pill warn", "sem áudio" }
                                            }
                                        }
                                        span { "{option.sub}" }
                                    }
                                    span { class: "muted", "{option.size}" }
                                    button {
                                        class: "btn small primary",
                                        onclick: move |_| {
                                            download_media(
                                                services_for_click.clone(),
                                                ui,
                                                &info_for_click,
                                                &option_for_click,
                                                is_audio,
                                            );
                                        },
                                        "Baixar"
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
fn DownloadRow(item: DownloadRecord) -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let id = item.id.clone();
    let can_retry = matches!(item.platform.as_deref(), None | Some("web"))
        && (item.can_retry.unwrap_or(false)
            || matches!(item.status.as_str(), "failed" | "canceled"));
    let can_cancel = item.is_active();
    let cancel_services = services.clone();
    let retry_services = services.clone();

    rsx! {
        article { class: "download-row {item.status}",
            oncontextmenu: {
                let record = item.clone();
                move |event: Event<MouseData>| {
                    event.prevent_default();
                    let point = event.data().client_coordinates();
                    ui.context_menu.set(Some(ContextMenuState::new(
                        point.x,
                        point.y,
                        ContextTarget::Download {
                            record: Box::new(record.clone()),
                        },
                    )));
                }
            },
            div { class: "download-row-head",
                div { class: "download-row-title",
                    strong { "{item.title()}" }
                    span { class: "pill", "{platform_label(item.platform.as_deref())}" }
                    span { class: "download-status {item.status}", "{status_label(&item.status)}" }
                }
                span { class: "download-percent", "{item.percent().round() as i64}%" }
            }
            div {
                class: "download-progress",
                style: "--progress: {item.percent()}%;",
            }
            div { class: "download-row-meta",
                span { "{format_bytes(item.file_size)}" }
                if let (Some(total), Some(done)) = (item.batch_total, item.batch_downloaded) {
                    span { "{done}/{total} do lote" }
                }
                if let Some(source) = item.source_url.as_ref() {
                    span { class: "download-source", title: "{source}", "{source}" }
                }
                if let Some(path) = item.file_path.as_ref() {
                    span { class: "download-path", title: "{path}", "{path}" }
                }
            }
            if let Some(message) = item.error.as_ref() {
                p { class: "error-banner", "{message}" }
            }
            div { class: "download-actions",
                if can_cancel {
                    button {
                        class: "btn small ghost",
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                if cancel_services.downloads.cancel(&id) {
                                    update_download(
                                        cancel_services.clone(),
                                        ui,
                                        &id,
                                        |item| {
                                            item.status = "canceled".to_string();
                                            item.can_cancel = Some(false);
                                            item.can_retry = Some(true);
                                        },
                                        true,
                                    );
                                    push_toast(ui.toasts, "Download cancelado.", ToastKind::Info);
                                }
                            }
                        },
                        "Cancelar"
                    }
                }
                if can_retry {
                    button {
                        class: "btn small",
                        onclick: {
                            let item = item.clone();
                            move |_| retry_record(retry_services.clone(), ui, item.clone())
                        },
                        "Tentar de novo"
                    }
                }
                if item.status == "completed" {
                    if let Some(path) = item.file_path.clone() {
                        button {
                            class: "btn small",
                            onclick: {
                                let path = path.clone();
                                move |_| {
                                    if let Err(message) = plasma_platform::open_path(&path) {
                                        push_toast(ui.toasts, message, ToastKind::Error);
                                    }
                                }
                            },
                            "Abrir"
                        }
                        button {
                            class: "btn small ghost",
                            onclick: {
                                let path = path.clone();
                                move |_| {
                                    if let Err(message) = plasma_platform::reveal_path(&path) {
                                        push_toast(ui.toasts, message, ToastKind::Error);
                                    }
                                }
                            },
                            "Mostrar"
                        }
                    }
                    button {
                        class: "btn small ghost",
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                let mut next = ui.downloads();
                                next.retain(|entry| entry.id != id);
                                ui.downloads.set(next.clone());
                                let downloads = services.downloads.clone();
                                dioxus::prelude::spawn(async move {
                                    let _ = downloads.save_history(next).await;
                                });
                            }
                        },
                        "Remover"
                    }
                }
            }
        }
    }
}

fn analyze_current(
    services: AppServices,
    url: Signal<String>,
    mut analysis: Signal<Option<MediaInfo>>,
    mut analyzing: Signal<bool>,
    mut error: Signal<Option<String>>,
) {
    let raw_url = url().trim().to_string();
    if raw_url.is_empty() {
        error.set(Some("Informe uma URL para analisar.".to_string()));
        return;
    }
    if !raw_url.starts_with("https://") {
        error.set(Some("Use uma URL HTTPS.".to_string()));
        return;
    }
    analyzing.set(true);
    error.set(None);
    analysis.set(None);
    spawn(async move {
        match services.extract.analyze(raw_url).await {
            Ok(info) => analysis.set(Some(info)),
            Err(message) => error.set(Some(message)),
        }
        analyzing.set(false);
    });
}

fn download_media(
    services: AppServices,
    ui: UiState,
    info: &MediaInfo,
    option: &FormatOption,
    is_audio: bool,
) {
    if option.url.is_none() && info.platform != "youtube" {
        push_toast(
            ui.toasts,
            "Este formato não tem URL direta para download.",
            ToastKind::Error,
        );
        return;
    }
    let extension = media_extension(option, is_audio);
    let title = sanitize_filename(&info.title.chars().take(60).collect::<String>());
    let label = sanitize_filename(&option.label.chars().take(40).collect::<String>());
    let name = format!("{title}-{label}.{extension}");
    let destination = services.downloads.destination_for(&name);
    let file_name = Path::new(&destination)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(&name)
        .to_string();
    let id = format!("{}-{}", info.platform, new_download_id());
    let mut record = DownloadRecord::new_url(
        id.clone(),
        file_name.clone(),
        destination,
        option
            .url
            .clone()
            .unwrap_or_else(|| info.original_url.clone()),
    );
    record.platform = Some(info.platform.clone());
    record.source_page_url = Some(info.original_url.clone());
    record.source_mode = Some(if is_audio { "audio" } else { "video" }.to_string());
    record.source_format_id = Some(option.id.clone());
    record.source_label = Some(info.author.clone());
    record.thumbnail_url = info.thumbnail_url.clone();
    let original_url = info.original_url.clone();
    let format_id = option.id.clone();
    let direct_url = option.url.clone();
    let has_audio = option.has_audio;
    push_download(services.clone(), ui, record);
    spawn(async move {
        let result = services
            .extract
            .download(
                id.clone(),
                original_url,
                format_id,
                direct_url,
                file_name,
                has_audio,
            )
            .await;
        finish_url_download(services, ui, id, result);
    });
}

fn submit_url(
    services: AppServices,
    ui: UiState,
    mut url: Signal<String>,
    mut file_name: Signal<String>,
    mut error: Signal<Option<String>>,
) {
    let raw_url = url().trim().to_string();
    if raw_url.is_empty() {
        error.set(Some("Informe uma URL HTTPS para baixar.".to_string()));
        return;
    }
    if !raw_url.starts_with("https://") {
        error.set(Some(
            "Use uma URL HTTPS. Redirecionamentos para HTTP são bloqueados.".to_string(),
        ));
        return;
    }
    let name = file_name().trim().to_string();
    let name = if name.is_empty() {
        filename_from_url(&raw_url)
    } else {
        sanitize_filename(&name)
    };
    let id = new_download_id();
    let destination = services.downloads.destination_for(&name);
    let record = DownloadRecord::new_url(id, name, destination, raw_url);
    start_record(services, ui, record);
    url.set(String::new());
    file_name.set(String::new());
    error.set(None);
}

fn retry_record(services: AppServices, ui: UiState, item: DownloadRecord) {
    let Some(url) = item.source_url.clone() else {
        push_toast(
            ui.toasts,
            "Este item não tem URL de origem.",
            ToastKind::Error,
        );
        return;
    };
    let Some(file_path) = item.file_path.clone() else {
        push_toast(
            ui.toasts,
            "Este item não tem destino salvo.",
            ToastKind::Error,
        );
        return;
    };
    let id = if item.id.is_empty() {
        new_download_id()
    } else {
        item.id.clone()
    };
    let mut record = item;
    record.id = id.clone();
    record.status = "downloading".to_string();
    record.progress = 0.0;
    record.error = None;
    record.can_cancel = Some(true);
    record.can_retry = Some(false);
    push_download(services.clone(), ui, record);
    spawn(async move {
        let result = services
            .downloads
            .download_url(id.clone(), url, file_path, None)
            .await;
        finish_url_download(services, ui, id, result);
    });
}

fn start_record(services: AppServices, ui: UiState, record: DownloadRecord) {
    let id = record.id.clone();
    let url = record.source_url.clone().unwrap_or_default();
    let file_path = record.file_path.clone().unwrap_or_default();
    push_download(services.clone(), ui, record);
    spawn(async move {
        let result = services
            .downloads
            .download_url(id.clone(), url, file_path, None)
            .await;
        finish_url_download(services, ui, id, result);
    });
}

fn finish_url_download(services: AppServices, ui: UiState, id: String, result: Result<(), String>) {
    match result {
        Ok(()) => {
            update_download(
                services.clone(),
                ui,
                &id,
                |item| {
                    item.progress = 100.0;
                    item.status = "completed".to_string();
                    item.error = None;
                    item.file_size = item
                        .file_path
                        .as_ref()
                        .and_then(|path| std::fs::metadata(path).ok())
                        .map(|metadata| metadata.len().min(i64::MAX as u64) as i64)
                        .or(item.file_size);
                    item.can_cancel = Some(false);
                    item.can_retry = Some(false);
                },
                true,
            );
        }
        Err(error) if error == plasma_download::DOWNLOAD_CANCELED_ERROR => {
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
            push_toast(
                ui.toasts,
                format!("Download falhou: {error}"),
                ToastKind::Error,
            );
            update_download(
                services,
                ui,
                &id,
                |item| {
                    item.status = "failed".to_string();
                    item.error = Some(error);
                    item.can_cancel = Some(false);
                    item.can_retry = Some(true);
                },
                true,
            );
        }
    }
}

fn filename_from_url(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let path = match without_query.split_once("://") {
        Some((_, rest)) => match rest.split_once('/') {
            Some((_, path)) => path,
            None => return "download".to_string(),
        },
        None => without_query,
    };
    let candidate = path
        .rsplit('/')
        .find(|part| !part.trim().is_empty())
        .unwrap_or("download");
    sanitize_filename(candidate)
}

fn media_extension(option: &FormatOption, is_audio: bool) -> String {
    if let Some(url) = option.url.as_deref() {
        let path = url.split(['?', '#']).next().unwrap_or(url);
        if let Some(extension) = Path::new(path)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase())
        {
            if matches!(
                extension.as_str(),
                "jpg"
                    | "jpeg"
                    | "png"
                    | "webp"
                    | "gif"
                    | "mp4"
                    | "mov"
                    | "m4a"
                    | "mp3"
                    | "webm"
                    | "opus"
                    | "aac"
            ) {
                return if extension == "jpeg" {
                    "jpg".to_string()
                } else {
                    extension
                };
            }
        }
    }
    let sub = option.sub.to_ascii_lowercase();
    if is_audio {
        if sub.contains("webm") || sub.contains("opus") {
            "webm".to_string()
        } else if sub.contains("mp3") {
            "mp3".to_string()
        } else if sub.contains("aac") || sub.contains("m4a") || sub.contains("mp4") {
            "m4a".to_string()
        } else {
            "mp4".to_string()
        }
    } else if sub.contains("webm") {
        "webm".to_string()
    } else if sub.contains("jpg")
        || sub.contains("jpeg")
        || sub.contains("imagem")
        || sub.contains("image")
    {
        "jpg".to_string()
    } else if sub.contains("png") {
        "png".to_string()
    } else {
        "mp4".to_string()
    }
}

fn is_image_option(option: &FormatOption) -> bool {
    let sub = option.sub.to_ascii_lowercase();
    sub.contains("imagem")
        || sub.contains("image")
        || sub.contains("jpg")
        || sub.contains("jpeg")
        || sub.contains("png")
        || sub.contains("gif")
}

fn status_label(status: &str) -> &'static str {
    match status {
        "downloading" => "Baixando",
        "completed" => "Concluído",
        "failed" => "Falhou",
        "canceled" => "Cancelado",
        _ => "Pendente",
    }
}

fn platform_label(platform: Option<&str>) -> &'static str {
    match platform.unwrap_or("web") {
        "twitter" => "X/Twitter",
        "telegram" => "Telegram",
        "youtube" => "YouTube",
        "tiktok" => "TikTok",
        "instagram" => "Instagram",
        "reddit" => "Reddit",
        _ => "Web",
    }
}

fn format_bytes(size: Option<i64>) -> String {
    let Some(size) = size.filter(|value| *value > 0) else {
        return "tamanho desconhecido".to_string();
    };
    let size = size as f64;
    if size >= 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GB", size / (1024.0 * 1024.0 * 1024.0))
    } else if size >= 1024.0 * 1024.0 {
        format!("{:.1} MB", size / (1024.0 * 1024.0))
    } else if size >= 1024.0 {
        format!("{:.1} KB", size / 1024.0)
    } else {
        format!("{size:.0} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derives_a_safe_name_from_url() {
        assert_eq!(
            filename_from_url("https://cdn.example.com/path/video.mp4?token=1"),
            "video.mp4"
        );
        assert_eq!(filename_from_url("https://example.com/"), "download");
    }

    #[test]
    fn labels_platforms_and_statuses() {
        assert_eq!(platform_label(Some("twitter")), "X/Twitter");
        assert_eq!(platform_label(None), "Web");
        assert_eq!(status_label("failed"), "Falhou");
    }

    #[test]
    fn picks_extensions_from_url_or_format() {
        let mut option = FormatOption::new("1", "Video", "video/mp4");
        option.url = Some("https://cdn.example.com/video.webm?x=1".to_string());
        assert_eq!(media_extension(&option, false), "webm");
        option.url = None;
        assert_eq!(media_extension(&option, true), "m4a");
    }
}
