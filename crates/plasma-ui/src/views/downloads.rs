//! Downloads queue and persisted history for the Rust shell.

use crate::downloads::{
    new_download_id, push_download, sanitize_filename, update_download, DownloadRecord,
};
use crate::state::{push_toast, AppServices, ToastKind, UiState};
use dioxus::prelude::*;

#[component]
pub fn DownloadsView() -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let mut url = use_signal(String::new);
    let mut file_name = use_signal(String::new);
    let error = use_signal(|| None::<String>);

    let download_dir = services.downloads.download_dir().display().to_string();
    let items = ui.downloads();
    let active = items.iter().filter(|item| item.is_active()).count();
    let completed = items
        .iter()
        .filter(|item| item.status == "completed")
        .count();

    let on_enter = {
        let services = services.clone();
        move |event: KeyboardEvent| {
            if event.key() == Key::Enter {
                submit_url(services.clone(), ui, url, file_name, error);
            }
        }
    };
    let on_enter_second = on_enter.clone();

    rsx! {
        section { class: "downloads-view",
            div { class: "downloads-inner",
                header { class: "downloads-heading",
                    div {
                        h1 { "Downloads" }
                        p { class: "muted",
                            "Fila nativa com retomada, cancelamento e histórico no SQLite. \
                             O histórico é o mesmo arquivo JSON usado pelo app React."
                        }
                    }
                    div { class: "downloads-stats",
                        span { class: "pill", "{active} ativo(s)" }
                        span { class: "pill", "{completed} concluído(s)" }
                    }
                }

                section { class: "download-card",
                    h2 { "Baixar URL" }
                    p { class: "hint",
                        "Cole um link direto de mídia (MP4, imagem, áudio ou arquivo). \
                         Use a aba X/Twitter para analisar posts e perfis."
                    }
                    div { class: "download-form",
                        input {
                            class: "download-input",
                            r#type: "url",
                            placeholder: "https://exemplo.com/video.mp4",
                            value: "{url}",
                            oninput: move |event| url.set(event.value()),
                            onkeydown: on_enter,
                        }
                        input {
                            class: "download-input download-name",
                            r#type: "text",
                            placeholder: "nome opcional (video.mp4)",
                            value: "{file_name}",
                            oninput: move |event| file_name.set(event.value()),
                            onkeydown: on_enter_second,
                        }
                        button {
                            class: "btn primary",
                            onclick: {
                                let services = services.clone();
                                move |_| submit_url(services.clone(), ui, url, file_name, error)
                            },
                            "Baixar"
                        }
                    }
                    if let Some(message) = error() {
                        p { class: "error-banner", "{message}" }
                    }
                    p { class: "download-destination",
                        strong { "Pasta: " }
                        code { "{download_dir}" }
                    }
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
                            p { "Nenhum download ainda. Cole uma URL acima para começar." }
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
fn DownloadRow(item: DownloadRecord) -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let id = item.id.clone();
    let can_retry =
        item.can_retry.unwrap_or(false) || matches!(item.status.as_str(), "failed" | "canceled");
    let can_cancel = item.is_active();
    let cancel_services = services.clone();
    let retry_services = services.clone();

    rsx! {
        article { class: "download-row {item.status}",
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
    dioxus::prelude::spawn(async move {
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
    dioxus::prelude::spawn(async move {
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
}
