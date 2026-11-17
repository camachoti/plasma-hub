//! Downloads queue, unified URL analysis and persisted history.

use crate::context::{ContextMenuState, ContextTarget};
use crate::downloads::{
    complete_download_record, new_download_id, push_download, sanitize_filename, update_download,
    DownloadRecord,
};
use crate::state::{AppServices, UiState};
use crate::views::select::{Select, SelectOption};
use dioxus::prelude::*;
use plasma_extract::{FormatOption, MediaInfo};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DownloadFilter {
    All,
    Active,
    Completed,
    Issues,
}

impl DownloadFilter {
    const ALL: [Self; 4] = [Self::All, Self::Active, Self::Completed, Self::Issues];

    fn label(self) -> &'static str {
        match self {
            Self::All => "Todos",
            Self::Active => "Ativos",
            Self::Completed => "Concluídos",
            Self::Issues => "Falhas",
        }
    }

    fn matches(self, item: &DownloadRecord) -> bool {
        match self {
            Self::All => true,
            Self::Active => item.is_active(),
            Self::Completed => item.status == "completed",
            Self::Issues => matches!(item.status.as_str(), "failed" | "canceled"),
        }
    }
}

#[derive(Clone)]
enum ListEntry {
    Single(Box<DownloadRecord>),
    Batch {
        id: String,
        items: Vec<DownloadRecord>,
    },
}

#[component]
pub fn DownloadsView() -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let mut url = use_signal(String::new);
    let mut file_name = use_signal(String::new);
    let mut error = use_signal(|| None::<String>);
    let mut analysis_error = use_signal(|| None::<String>);
    let mut analysis = use_signal(|| None::<MediaInfo>);
    let mut selected_format = use_signal(String::new);
    let analyzing = use_signal(|| false);
    let analysis_generation = use_signal(|| 0u64);
    let mut filter = use_signal(|| DownloadFilter::All);
    let expanded_batches = use_signal(HashSet::<String>::new);

    let mut shared_url = ui.shell.shared_url;
    let shared_services = services.clone();
    use_effect(move || {
        if let Some(value) = shared_url() {
            shared_url.set(None);
            let normalized = value.trim().to_string();
            url.set(normalized);
            analyze_current(
                shared_services.clone(),
                url,
                analysis,
                analyzing,
                analysis_error,
                analysis_generation,
                selected_format,
            );
        }
    });

    let download_dir = services.downloads.download_dir().display().to_string();
    let items = ui.transfers.downloads();
    let active = items.iter().filter(|item| item.is_active()).count();
    let completed = items
        .iter()
        .filter(|item| item.status == "completed")
        .count();
    let failed = items.iter().filter(|item| item.status == "failed").count();
    let canceled = items
        .iter()
        .filter(|item| item.status == "canceled")
        .count();
    let issues = failed + canceled;
    let overall = (active > 0).then(|| {
        let sum: f64 = items
            .iter()
            .filter(|item| item.is_active())
            .map(|item| item.progress)
            .sum();
        sum / active as f64
    });
    let current_analysis = analysis();
    let current_filter = filter();
    let filtered: Vec<DownloadRecord> = items
        .iter()
        .filter(|item| current_filter.matches(item))
        .cloned()
        .collect();
    let entries = build_list_entries(&filtered);
    let retryable_failed: Vec<_> = items
        .iter()
        .filter(|item| {
            matches!(item.status.as_str(), "failed" | "canceled") && can_retry_record(item)
        })
        .cloned()
        .collect();
    let cancelable_active: Vec<_> = items
        .iter()
        .filter(|item| item.is_active() && item.can_cancel.unwrap_or(true))
        .cloned()
        .collect();
    let has_finished = items.iter().any(|item| !item.is_active());

    let analyze_enter = {
        let services = services.clone();
        move |event: KeyboardEvent| {
            if event.key() == Key::Enter {
                analyze_current(
                    services.clone(),
                    url,
                    analysis,
                    analyzing,
                    analysis_error,
                    analysis_generation,
                    selected_format,
                );
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
                            "Importe links, acompanhe lotes e retome o histórico salvo no SQLite."
                        }
                    }
                    div { class: "downloads-heading-actions",
                        button {
                            class: "btn small ghost",
                            onclick: {
                                let dir = download_dir.clone();
                                move |_| {
                                    let _ = plasma_platform::reveal_path(&dir);
                                }
                            },
                            "Pasta"
                        }
                        if !cancelable_active.is_empty() {
                            button {
                                class: "btn small ghost",
                                onclick: {
                                    let services = services.clone();
                                    let batch = cancelable_active.clone();
                                    move |_| {
                                        for item in batch.iter() {
                                            if services.downloads.cancel(&item.id) {
                                                update_download(
                                                    services.clone(),
                                                    ui,
                                                    &item.id,
                                                    |record| {
                                                        record.status = "canceled".to_string();
                                                        record.can_cancel = Some(false);
                                                        record.can_retry = Some(true);
                                                    },
                                                    true,
                                                );
                                            }
                                        }
                                    }
                                },
                                "Cancelar ativos ({cancelable_active.len()})"
                            }
                        }
                        if has_finished {
                            button {
                                class: "btn small ghost",
                                onclick: {
                                    let services = services.clone();
                                    move |_| {
                                        let mut next = ui.transfers.downloads();
                                        next.retain(|item| item.is_active());
                                        ui.transfers.downloads.set(next.clone());
                                        let downloads = services.downloads.clone();
                                        dioxus::prelude::spawn(async move {
                                            let _ = downloads.save_history(next).await;
                                        });
                                    }
                                },
                                "Limpar finalizados"
                            }
                        }
                    }
                }

                div { class: "downloads-stats",
                    span {
                        class: if active > 0 { "pill busy" } else { "pill" },
                        "{active} ativo(s)"
                    }
                    if issues > 0 {
                        span {
                            class: "pill failed",
                            aria_label: "Downloads com falha ou cancelados",
                            "{issues} com problema"
                        }
                    }
                    span { class: "pill", "{completed} concluído(s)" }
                    span { class: "pill", "{items.len()} no histórico" }
                }

                if let Some(progress) = overall {
                    div { class: "downloads-status",
                        div { class: "downloads-status-track",
                            div {
                                class: "downloads-status-fill",
                                style: "width: {progress:.1}%",
                            }
                        }
                        span {
                            class: "downloads-status-label",
                            "{progress:.0}% médio · {active} em andamento"
                        }
                    }
                }

                section { class: "download-card",
                    h2 { "Importar link" }
                    p { class: "hint",
                        "Cole uma URL de post/vídeo para analisar formatos, ou use “Baixar URL” \
                         para um arquivo HTTPS direto."
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
                                selected_format.set(String::new());
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
                                        analysis_generation,
                                        selected_format,
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
                    ImportPreview {
                        info,
                        selected_format,
                        url,
                        file_name,
                        analysis,
                        analysis_error,
                        error,
                    }
                }

                section { class: "download-card downloads-list-card",
                    div { class: "download-list-heading",
                        div {
                            h2 { "Atividade" }
                            p { class: "hint",
                                if active > 0 {
                                    "{active} em andamento · recentes primeiro"
                                } else {
                                    "Downloads recentes aparecem primeiro."
                                }
                            }
                        }
                        if !retryable_failed.is_empty() {
                            button {
                                class: "btn small",
                                onclick: {
                                    let services = services.clone();
                                    let batch = retryable_failed.clone();
                                    move |_| {
                                        for record in batch.iter().cloned() {
                                            retry_record(services.clone(), ui, record);
                                        }
                                    }
                                },
                                "Tentar todos ({retryable_failed.len()})"
                            }
                        }
                    }

                    div { class: "download-filter-tabs", role: "tablist",
                        for option in DownloadFilter::ALL {
                            {
                                let count = items.iter().filter(|item| option.matches(item)).count();
                                let selected = current_filter == option;
                                rsx! {
                                    button {
                                        key: "{option.label()}",
                                        class: if selected { "download-filter-tab active" } else { "download-filter-tab" },
                                        role: "tab",
                                        aria_selected: selected,
                                        onclick: move |_| filter.set(option),
                                        "{option.label()}"
                                        span { class: "download-filter-count", "{count}" }
                                    }
                                }
                            }
                        }
                    }

                    if filtered.is_empty() {
                        div { class: "downloads-empty",
                            div { class: "empty-icon", "↓" }
                            p {
                                if items.is_empty() {
                                    "Nenhum download ainda. Importe um link para começar."
                                } else {
                                    "Nenhum item neste filtro."
                                }
                            }
                        }
                    } else {
                        div { class: "downloads-list",
                            for entry in entries {
                                match entry {
                                    ListEntry::Single(item) => rsx! {
                                        DownloadRow {
                                            key: "{item.id}",
                                            item: *item,
                                            compact: false,
                                        }
                                    },
                                    ListEntry::Batch { id, items } => rsx! {
                                        DownloadBatchCard {
                                            key: "{id}",
                                            batch_id: id,
                                            items,
                                            expanded_batches,
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
}

#[component]
fn ImportPreview(
    info: MediaInfo,
    mut selected_format: Signal<String>,
    mut url: Signal<String>,
    mut file_name: Signal<String>,
    mut analysis: Signal<Option<MediaInfo>>,
    mut analysis_error: Signal<Option<String>>,
    error: Signal<Option<String>>,
) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let formats = downloadable_formats(&info);
    let options: Vec<SelectOption> = formats
        .iter()
        .map(|(option, is_audio)| {
            let mut label = option.label.clone();
            if option.best.unwrap_or(false) {
                label.push_str(" · melhor");
            }
            if !*is_audio && !option.has_audio && !is_image_option(option) {
                label.push_str(" · sem áudio");
            }
            if option.size != "—" {
                label.push_str(&format!(" · {}", option.size));
            }
            SelectOption::new(option.id.clone(), label)
        })
        .collect();
    let selected = selected_format();
    let selected_pair = formats
        .iter()
        .find(|(option, _)| option.id == selected)
        .cloned();
    let can_confirm = selected_pair
        .as_ref()
        .is_some_and(|(option, _)| can_download_format(&info, option));
    let platform = platform_label(Some(info.platform.as_str()));
    let thumbnail = info.thumbnail_url.clone();

    rsx! {
        article { class: "download-card import-preview",
            div { class: "import-preview-head",
                if let Some(thumbnail) = thumbnail {
                    img { class: "import-thumb", src: "{thumbnail}", alt: "" }
                } else {
                    div {
                        class: "import-thumb fallback",
                        style: "--thumb-hue: {info.thumb_hue}; --thumb-hue2: {info.thumb_hue2};",
                    }
                }
                div { class: "import-preview-copy",
                    div { class: "import-preview-badges",
                        span { class: "pill", "{platform}" }
                        if !info.duration.trim().is_empty() {
                            span { class: "pill", "{info.duration}" }
                        }
                    }
                    h2 { "{info.title}" }
                    p { class: "muted",
                        span { "{info.author}" }
                        if let Some(full) = info.author_full.as_ref() {
                            span { " · {full}" }
                        }
                        if let Some(views) = info.views.as_ref() {
                            span { " · {views} visualizações" }
                        }
                    }
                    a {
                        class: "twitter-original",
                        href: "{info.original_url}",
                        target: "_blank",
                        "Abrir original"
                    }
                }
                button {
                    class: "btn small ghost",
                    onclick: move |_| {
                        analysis.set(None);
                        selected_format.set(String::new());
                        analysis_error.set(None);
                    },
                    "Limpar"
                }
            }
            if options.is_empty() {
                p { class: "hint", "Nenhum formato de mídia foi encontrado." }
            } else {
                div { class: "import-preview-actions",
                    label { class: "import-format-field",
                        span { "Formato" }
                        Select {
                            value: selected.clone(),
                            options: options.clone(),
                            placeholder: "Escolha um formato".to_string(),
                            disabled: false,
                            on_change: move |value| selected_format.set(value),
                        }
                    }
                    button {
                        class: "btn primary",
                        disabled: !can_confirm,
                        onclick: {
                            let info = info.clone();
                            let services = services.clone();
                            move |_| {
                                let Some((option, is_audio)) = selected_pair.clone() else {
                                    return;
                                };
                                download_media(
                                    services.clone(),
                                    ui,
                                    &info,
                                    &option,
                                    is_audio,
                                    error,
                                );
                                url.set(String::new());
                                file_name.set(String::new());
                                analysis.set(None);
                                selected_format.set(String::new());
                                analysis_error.set(None);
                            }
                        },
                        "Confirmar download"
                    }
                }
            }
        }
    }
}

#[component]
fn DownloadBatchCard(
    batch_id: String,
    items: Vec<DownloadRecord>,
    mut expanded_batches: Signal<HashSet<String>>,
) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let expanded = expanded_batches().contains(&batch_id);
    let summary = batch_summary(&items);
    let title = items
        .iter()
        .find_map(|item| item.batch_title.clone())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("Lote ({})", items.len()));
    let platform = platform_label(items.first().and_then(|item| item.platform.as_deref()));
    let total_size = items.iter().filter_map(|item| item.file_size).sum::<i64>();
    let progress = batch_progress(&items, &summary);
    let artwork = items
        .iter()
        .find(|item| {
            item.thumbnail_url
                .as_ref()
                .is_some_and(|url| !url.is_empty())
        })
        .cloned()
        .or_else(|| items.first().cloned());
    let cancelable: Vec<_> = items
        .iter()
        .filter(|item| item.is_active() && item.can_cancel.unwrap_or(true))
        .cloned()
        .collect();
    let retryable: Vec<_> = items
        .iter()
        .filter(|item| {
            matches!(item.status.as_str(), "failed" | "canceled") && can_retry_record(item)
        })
        .cloned()
        .collect();

    rsx! {
        article { class: "download-batch {summary.status}",
            div {
                class: "download-batch-overview",
                onclick: {
                    let batch_id = batch_id.clone();
                    move |_| {
                        let mut next = expanded_batches();
                        if next.contains(&batch_id) {
                            next.remove(&batch_id);
                        } else {
                            next.insert(batch_id.clone());
                        }
                        expanded_batches.set(next);
                    }
                },
                DownloadArtwork { item: artwork, status: summary.status.clone(), group: true }
                div { class: "download-batch-copy",
                    div { class: "download-row-title",
                        strong { "{title}" }
                        span { class: "pill", "{platform}" }
                        span { class: "download-status {summary.status}", "{status_label(&summary.status)}" }
                    }
                    div { class: "download-row-meta",
                        span { "{items.len()} itens" }
                        span { "{summary.completed} ok" }
                        if summary.skipped > 0 {
                            span { "{summary.skipped} existentes" }
                        }
                        if summary.failed > 0 {
                            span { "{summary.failed} falhas" }
                        }
                        if summary.running > 0 {
                            span { "{summary.running} ativos" }
                        }
                        span { "{format_bytes(Some(total_size).filter(|value| *value > 0))}" }
                    }
                    if summary.running > 0 || summary.completed + summary.failed < items.len() {
                        div {
                            class: "download-progress",
                            style: "--progress: {progress}%;",
                        }
                    }
                }
                span { class: "download-batch-chevron", if expanded { "▾" } else { "▸" } }
            }
            div { class: "download-batch-actions",
                if !cancelable.is_empty() {
                    button {
                        class: "btn small ghost",
                        onclick: {
                            let services = services.clone();
                            let batch = cancelable.clone();
                            move |_| {
                                for item in batch.iter() {
                                    if services.downloads.cancel(&item.id) {
                                        update_download(
                                            services.clone(),
                                            ui,
                                            &item.id,
                                            |record| {
                                                record.status = "canceled".to_string();
                                                record.can_cancel = Some(false);
                                                record.can_retry = Some(true);
                                            },
                                            true,
                                        );
                                    }
                                }
                            }
                        },
                        "Cancelar ativos"
                    }
                }
                if !retryable.is_empty() {
                    button {
                        class: "btn small",
                        onclick: {
                            let services = services.clone();
                            let batch = retryable.clone();
                            move |_| {
                                for record in batch.iter().cloned() {
                                    retry_record(services.clone(), ui, record);
                                }
                            }
                        },
                        "Retomar ({retryable.len()})"
                    }
                }
            }
            if expanded {
                div { class: "download-batch-children",
                    for item in items {
                        DownloadRow {
                            key: "{item.id}",
                            item,
                            compact: true,
                        }
                    }
                }
            }
        }
    }
}

#[component]
fn DownloadRow(item: DownloadRecord, compact: bool) -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let id = item.id.clone();
    let can_retry = can_retry_record(&item);
    let can_cancel = item.can_cancel.unwrap_or_else(|| item.is_active());
    let can_remove = !item.is_active();
    let completed = item.status == "completed";
    let path = item.file_path.clone();
    let cancel_services = services.clone();
    let retry_services = services.clone();
    let row_class = if compact {
        format!("download-row compact {}", item.status)
    } else {
        format!("download-row {}", item.status)
    };

    rsx! {
        article {
            class: "{row_class}",
            oncontextmenu: {
                let record = item.clone();
                move |event: Event<MouseData>| {
                    event.prevent_default();
                    let point = event.data().client_coordinates();
                    ui.panels.context_menu.set(Some(ContextMenuState::new(
                        point.x,
                        point.y,
                        ContextTarget::Download {
                            record: Box::new(record.clone()),
                        },
                    )));
                }
            },
            onclick: {
                let path = path.clone();
                move |_| {
                    if completed {
                        if let Some(path) = path.as_ref() {
                            let _ = plasma_platform::reveal_path(path);
                        }
                    }
                }
            },
            div { class: "download-row-main",
                DownloadArtwork {
                    item: Some(item.clone()),
                    status: item.status.clone(),
                    group: false,
                }
                div { class: "download-row-body",
                    div { class: "download-row-head",
                        div { class: "download-row-title",
                            strong { "{item.title()}" }
                            span { class: "pill", "{platform_label(item.platform.as_deref())}" }
                            span { class: "download-status {item.status}", "{status_label(&item.status)}" }
                        }
                        span { class: "download-percent", "{item.percent().round() as i64}%" }
                    }
                    if item.status != "completed" {
                        div {
                            class: "download-progress",
                            style: "--progress: {item.percent()}%;",
                        }
                    }
                    div { class: "download-row-meta",
                        span { "{format_bytes(item.file_size)}" }
                        if let Some(label) = item.source_label.as_ref() {
                            span { "{label}" }
                        }
                        if let Some(title) = item.batch_title.as_ref().filter(|_| !compact) {
                            span { "{title}" }
                        }
                        if let Some(chat_id) = item.chat_id.as_ref().filter(|id| !id.is_empty()) {
                            span { title: "Chat {chat_id}", "chat {chat_id}" }
                        }
                        if let Some(message_id) = item.message_id {
                            span { "msg {message_id}" }
                        }
                        if let (Some(total), Some(done)) = (item.batch_total, item.batch_downloaded) {
                            if compact {
                                span { "{done}/{total}" }
                            }
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
                }
            }
            div {
                class: "download-actions",
                onclick: move |event| event.stop_propagation(),
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
                        if item.error.as_deref().is_some_and(|msg| msg.contains("interrompido")) {
                            "Retomar"
                        } else {
                            "Tentar de novo"
                        }
                    }
                }
                if completed {
                    if let Some(path) = item.file_path.clone() {
                        button {
                            class: "btn small",
                            onclick: {
                                let path = path.clone();
                                move |_| {
                                    let _ = plasma_platform::open_path(&path);
                                }
                            },
                            "Abrir"
                        }
                        button {
                            class: "btn small ghost",
                            onclick: {
                                let path = path.clone();
                                move |_| {
                                    if cfg!(target_os = "android") {
                                        let _ = plasma_platform::share_path(&path);
                                    } else {
                                        let _ = plasma_platform::reveal_path(&path);
                                    }
                                }
                            },
                            if cfg!(target_os = "android") {
                                "Compartilhar"
                            } else {
                                "Mostrar"
                            }
                        }
                    }
                }
                if can_remove {
                    button {
                        class: "btn small ghost",
                        onclick: {
                            let id = id.clone();
                            move |_| {
                                let mut next = ui.transfers.downloads();
                                next.retain(|entry| entry.id != id);
                                ui.transfers.downloads.set(next.clone());
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

#[component]
fn DownloadArtwork(item: Option<DownloadRecord>, status: String, group: bool) -> Element {
    let class = if group {
        "download-art group"
    } else {
        "download-art"
    };
    let thumb = item
        .as_ref()
        .and_then(|record| record.thumbnail_url.clone())
        .filter(|url| !url.trim().is_empty());
    let glyph = match status.as_str() {
        "completed" => "✓",
        "failed" => "!",
        "canceled" => "–",
        "downloading" => "↓",
        _ => "·",
    };

    rsx! {
        div { class: "{class} {status}",
            if let Some(thumb) = thumb {
                img { src: "{thumb}", alt: "" }
            } else {
                span { class: "download-art-fallback", "{glyph}" }
            }
        }
    }
}

#[derive(Clone)]
struct BatchSummary {
    status: String,
    completed: usize,
    failed: usize,
    canceled: usize,
    running: usize,
    skipped: usize,
}

fn batch_summary(items: &[DownloadRecord]) -> BatchSummary {
    let mut summary = BatchSummary {
        status: "pending".into(),
        completed: 0,
        failed: 0,
        canceled: 0,
        running: 0,
        skipped: 0,
    };
    for item in items {
        match item.status.as_str() {
            "completed" => summary.completed += 1,
            "skipped" => summary.skipped += 1,
            "failed" => summary.failed += 1,
            "canceled" | "stopped" => summary.canceled += 1,
            "downloading" | "pending" => summary.running += 1,
            _ => {}
        }
        if item.status == "completed"
            && (item
                .error
                .as_deref()
                .is_some_and(|msg| msg.to_ascii_lowercase().contains("já exist"))
                || item
                    .extra
                    .get("skipped")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true))
        {
            summary.completed = summary.completed.saturating_sub(1);
            summary.skipped += 1;
        }
    }
    summary.status = if summary.running > 0 {
        "downloading".into()
    } else if summary.failed > 0 {
        "failed".into()
    } else if summary.canceled > 0 && summary.completed + summary.skipped == 0 {
        "canceled".into()
    } else if summary.completed + summary.skipped > 0 {
        "completed".into()
    } else {
        "pending".into()
    };
    summary
}

fn batch_progress(items: &[DownloadRecord], summary: &BatchSummary) -> f64 {
    let counted = summary.completed + summary.skipped + summary.failed + summary.canceled;
    if let Some(total) = items
        .iter()
        .find_map(|item| item.batch_total)
        .filter(|value| *value > 0)
    {
        let done = items
            .iter()
            .find_map(|item| item.batch_downloaded)
            .unwrap_or(counted as i64)
            .max(counted as i64);
        return ((done as f64 / total as f64) * 100.0).clamp(0.0, 100.0);
    }
    if items.is_empty() {
        return 0.0;
    }
    let sum: f64 = items.iter().map(|item| item.percent()).sum();
    (sum / items.len() as f64).clamp(0.0, 100.0)
}

fn build_list_entries(filtered: &[DownloadRecord]) -> Vec<ListEntry> {
    let mut groups: HashMap<String, Vec<DownloadRecord>> = HashMap::new();
    let mut entries: Vec<ListEntry> = Vec::new();

    for item in filtered.iter().rev() {
        if let Some(batch_id) = item
            .batch_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            groups.entry(batch_id).or_default().push(item.clone());
        } else {
            entries.push(ListEntry::Single(Box::new(item.clone())));
        }
    }

    for (id, mut items) in groups {
        items.sort_by_key(|item| std::cmp::Reverse(item.created_at));
        entries.push(ListEntry::Batch { id, items });
    }

    entries.sort_by(|a, b| {
        let a_ts = entry_timestamp(a);
        let b_ts = entry_timestamp(b);
        b_ts.cmp(&a_ts)
    });
    entries
}

fn entry_timestamp(entry: &ListEntry) -> i64 {
    match entry {
        ListEntry::Single(item) => item.created_at.unwrap_or(0),
        ListEntry::Batch { items, .. } => items
            .iter()
            .filter_map(|item| item.created_at)
            .max()
            .unwrap_or(0),
    }
}

fn analyze_current(
    services: AppServices,
    url: Signal<String>,
    mut analysis: Signal<Option<MediaInfo>>,
    mut analyzing: Signal<bool>,
    mut error: Signal<Option<String>>,
    mut generation: Signal<u64>,
    mut selected_format: Signal<String>,
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
    selected_format.set(String::new());
    let current = generation().wrapping_add(1);
    generation.set(current);
    spawn(async move {
        let result = services.extract.analyze(raw_url).await;
        if generation() != current {
            return;
        }
        match result {
            Ok(info) => {
                let formats = downloadable_formats(&info);
                let initial = formats
                    .iter()
                    .find(|(option, _)| option.best.unwrap_or(false))
                    .or_else(|| formats.first())
                    .map(|(option, _)| option.id.clone())
                    .unwrap_or_default();
                selected_format.set(initial);
                analysis.set(Some(info));
            }
            Err(message) => error.set(Some(message.to_string())),
        }
        analyzing.set(false);
    });
}

fn downloadable_formats(info: &MediaInfo) -> Vec<(FormatOption, bool)> {
    let mut formats = Vec::new();
    for option in &info.formats.video {
        if can_download_format(info, option) {
            formats.push((option.clone(), false));
        }
    }
    for option in &info.formats.audio {
        if can_download_format(info, option) {
            formats.push((option.clone(), true));
        }
    }
    formats
}

fn can_download_format(info: &MediaInfo, option: &FormatOption) -> bool {
    if option.id == "na" || option.id == "web-limit" {
        return false;
    }
    if info.platform == "youtube" && cfg!(target_os = "android") && !option.has_audio {
        return false;
    }
    if option.url.is_some() {
        return true;
    }
    matches!(info.platform.as_str(), "youtube" | "tiktok")
}

fn download_media(
    services: AppServices,
    ui: UiState,
    info: &MediaInfo,
    option: &FormatOption,
    is_audio: bool,
    mut error: Signal<Option<String>>,
) {
    if option.url.is_none() && info.platform != "youtube" && info.platform != "tiktok" {
        error.set(Some(
            "Este formato não tem URL direta para download.".to_string(),
        ));
        return;
    }
    error.set(None);
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

fn can_retry_record(item: &DownloadRecord) -> bool {
    if !matches!(item.status.as_str(), "failed" | "canceled")
        || item.source_url.is_none()
        || item.file_path.is_none()
    {
        return false;
    }
    match item.platform.as_deref() {
        None | Some("web") => true,
        Some("youtube" | "twitter" | "tiktok") => {
            item.source_page_url.is_some() && item.source_format_id.is_some()
        }
        Some(_) => true,
    }
}

fn retry_record(services: AppServices, ui: UiState, item: DownloadRecord) {
    let Some(url) = item.source_url.clone() else {
        mark_retry_error(services, ui, &item, "Este item não tem URL de origem.");
        return;
    };
    let Some(file_path) = item.file_path.clone() else {
        mark_retry_error(services, ui, &item, "Este item não tem destino salvo.");
        return;
    };
    let id = if item.id.is_empty() {
        new_download_id()
    } else {
        item.id.clone()
    };
    let original = item.clone();
    let mut record = item;
    record.id = id.clone();
    record.status = "downloading".to_string();
    record.progress = 0.0;
    record.error = None;
    record.can_cancel = Some(true);
    record.can_retry = Some(false);
    push_download(services.clone(), ui, record);
    spawn(async move {
        let result = retry_download(&services, &original, &id, &file_path, &url).await;
        finish_url_download(services, ui, id, result);
    });
}

fn mark_retry_error(services: AppServices, ui: UiState, item: &DownloadRecord, message: &str) {
    if item.id.is_empty() {
        return;
    }
    update_download(
        services,
        ui,
        &item.id,
        |record| {
            record.status = "failed".to_string();
            record.error = Some(message.to_string());
            record.can_cancel = Some(false);
            record.can_retry = Some(false);
        },
        true,
    );
}

/// Retries a failed item. Native extractors re-analyze the original page when
/// their direct media URL may have expired; direct web downloads keep using the
/// original file destination so the partial file can be resumed.
async fn retry_download(
    services: &AppServices,
    item: &DownloadRecord,
    id: &str,
    file_path: &str,
    source_url: &str,
) -> plasma_core::Result<()> {
    let platform = item.platform.as_deref().unwrap_or("web");
    let file_name = Path::new(file_path)
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Destino de download inválido.".to_string())?
        .to_string();

    match platform {
        "youtube" | "twitter" | "tiktok" => {
            let page_url = item
                .source_page_url
                .clone()
                .unwrap_or_else(|| source_url.to_string());
            let format_id = item
                .source_format_id
                .clone()
                .ok_or_else(|| "Este download não tem formato salvo.".to_string())?;
            let info = services.extract.analyze(page_url.clone()).await?;
            let option = info
                .formats
                .video
                .iter()
                .chain(info.formats.audio.iter())
                .find(|option| option.id == format_id)
                .or_else(|| {
                    info.formats
                        .video
                        .iter()
                        .chain(info.formats.audio.iter())
                        .find(|option| option.best.unwrap_or(false))
                })
                .cloned()
                .ok_or_else(|| {
                    "O formato original não está mais disponível. Analise o link novamente."
                        .to_string()
                })?;
            services
                .extract
                .download(
                    id.to_string(),
                    page_url,
                    option.id.clone(),
                    option.url.clone(),
                    file_name,
                    option.has_audio,
                )
                .await
        }
        "instagram" | "reddit" => {
            let page_url = item
                .source_page_url
                .clone()
                .unwrap_or_else(|| source_url.to_string());
            let format_id = item
                .source_format_id
                .clone()
                .unwrap_or_else(|| "best".to_string());
            let has_audio = item.source_mode.as_deref() == Some("audio");
            services
                .extract
                .download(
                    id.to_string(),
                    page_url,
                    format_id,
                    Some(source_url.to_string()),
                    file_name,
                    has_audio,
                )
                .await
        }
        _ => {
            services
                .downloads
                .download_url(
                    id.to_string(),
                    source_url.to_string(),
                    file_path.to_string(),
                    None,
                )
                .await
        }
    }
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

fn finish_url_download(
    services: AppServices,
    ui: UiState,
    id: String,
    result: plasma_core::Result<()>,
) {
    match result {
        Ok(()) => {
            update_download(services, ui, &id, complete_download_record, true);
        }
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
            update_download(
                services,
                ui,
                &id,
                |item| {
                    item.status = "failed".to_string();
                    item.error = Some(error.to_string());
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
    fn filters_match_expected_statuses() {
        let mut item = DownloadRecord::new_url(
            "1".into(),
            "a.mp4".into(),
            "/tmp/a.mp4".into(),
            "https://cdn.example.com/a.mp4".into(),
        );
        assert!(DownloadFilter::Active.matches(&item));
        item.status = "failed".into();
        assert!(DownloadFilter::Issues.matches(&item));
        item.status = "completed".into();
        assert!(DownloadFilter::Completed.matches(&item));
    }

    #[test]
    fn groups_batch_items_together() {
        let mut a = DownloadRecord::new_url(
            "1".into(),
            "a.mp4".into(),
            "/tmp/a.mp4".into(),
            "https://cdn.example.com/a.mp4".into(),
        );
        a.batch_id = Some("batch-1".into());
        a.created_at = Some(2);
        let mut b = DownloadRecord::new_url(
            "2".into(),
            "b.mp4".into(),
            "/tmp/b.mp4".into(),
            "https://cdn.example.com/b.mp4".into(),
        );
        b.batch_id = Some("batch-1".into());
        b.created_at = Some(3);
        let mut c = DownloadRecord::new_url(
            "3".into(),
            "c.mp4".into(),
            "/tmp/c.mp4".into(),
            "https://cdn.example.com/c.mp4".into(),
        );
        c.created_at = Some(4);
        let entries = build_list_entries(&[a, b, c]);
        assert_eq!(entries.len(), 2);
        assert!(matches!(entries[0], ListEntry::Single(_)));
        assert!(matches!(entries[1], ListEntry::Batch { .. }));
    }

    #[test]
    fn retries_failed_native_items_only_when_enough_source_data_exists() {
        let mut item = DownloadRecord::new_url(
            "1".into(),
            "video.mp4".into(),
            "/tmp/video.mp4".into(),
            "https://cdn.example.com/video.mp4".into(),
        );
        item.status = "failed".into();
        item.platform = Some("youtube".into());
        assert!(!can_retry_record(&item));

        item.source_page_url = Some("https://youtube.com/watch?v=abc".into());
        item.source_format_id = Some("137".into());
        assert!(can_retry_record(&item));

        item.status = "completed".into();
        assert!(!can_retry_record(&item));
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
