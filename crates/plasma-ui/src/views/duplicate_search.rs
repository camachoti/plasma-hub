//! "Procurar arquivo no sistema": finds copies of selected media in a folder.

use std::path::Path;

use crate::icons::{Icon, IconName};
use crate::state::{
    push_toast, AppServices, DuplicateSearchMedia, DuplicateSearchProgress, ToastKind, UiState,
};
use dioxus::prelude::*;
use plasma_duplicates::{SearchMatch, SearchMode, SearchSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchPhase {
    /// User fills folder + mode before starting.
    Setup,
    Preparing,
    Searching,
    Done,
    Canceled,
    Failed,
}

#[component]
pub fn DuplicateSearchDialog(
    items: Vec<DuplicateSearchMedia>,
    on_close: EventHandler<()>,
) -> Element {
    let services = use_context::<AppServices>();
    let ui = use_context::<UiState>();
    let toasts = ui.shell.toasts;
    let mut progress_signal = ui.transfers.duplicate_search_progress;
    let mut folder = use_signal(|| services.downloads.download_dir().display().to_string());
    let mut mode = use_signal(|| SearchMode::Exact);
    let mut phase = use_signal(|| SearchPhase::Setup);
    let mut error = use_signal(|| None::<String>);
    let mut run_id = use_signal(String::new);
    let mut start_token = use_signal(|| 0u64);

    // Run only when the user confirms from the setup form.
    let search_items = items.clone();
    let search_services = services.clone();
    use_effect(move || {
        let token = start_token();
        if token == 0 {
            return;
        }
        let services = search_services.clone();
        let items = search_items.clone();
        let folder_path = folder();
        let search_mode = mode();
        let run = format!(
            "ui-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        );
        run_id.set(run.clone());
        phase.set(SearchPhase::Preparing);
        error.set(None);
        // Reset so a previous run cannot leave stale hits for this id.
        progress_signal.set(Some(DuplicateSearchProgress {
            run_id: run.clone(),
            scanned: 0,
            analyzed: 0,
            matches: Vec::new(),
            unreadable: 0,
        }));
        spawn(async move {
            let mut sources: Vec<SearchSource> = Vec::new();
            for item in &items {
                match services
                    .telegram
                    .ensure_media_cached(item.chat_id, item.message_id)
                    .await
                {
                    Ok(meta) => {
                        if !meta.success {
                            error.set(Some(meta.error.unwrap_or_else(|| {
                                "Não foi possível baixar a mídia para o cache antes da busca."
                                    .to_string()
                            })));
                            phase.set(SearchPhase::Failed);
                            return;
                        }
                        let Some(path) = meta
                            .native_file_path
                            .filter(|path| Path::new(path).is_file())
                        else {
                            error.set(Some(
                                "A mídia ainda não está disponível no cache local.".to_string(),
                            ));
                            phase.set(SearchPhase::Failed);
                            return;
                        };
                        sources.push(SearchSource::from_path(
                            item.message_id.to_string(),
                            item.name.clone(),
                            path,
                            item.is_video,
                        ));
                    }
                    Err(message) => {
                        error.set(Some(message.to_string()));
                        phase.set(SearchPhase::Failed);
                        return;
                    }
                }
            }

            if sources.is_empty() {
                error.set(Some("Nenhuma mídia foi selecionada.".to_string()));
                phase.set(SearchPhase::Failed);
                return;
            }

            phase.set(SearchPhase::Searching);
            let result = services
                .telegram
                .search_duplicate_media(run.clone(), folder_path, sources, Some(search_mode))
                .await;
            match result {
                Ok(result) => {
                    // Authoritative final snapshot (progress already sends the
                    // cumulative list; this covers cancel / event-ordering gaps).
                    progress_signal.set(Some(DuplicateSearchProgress {
                        run_id: run,
                        scanned: result.scanned,
                        analyzed: result.analyzed,
                        matches: result.matches,
                        unreadable: result.unreadable,
                    }));
                    if result.canceled {
                        phase.set(SearchPhase::Canceled);
                    } else {
                        phase.set(SearchPhase::Done);
                    }
                }
                Err(message) => {
                    error.set(Some(message.to_string()));
                    phase.set(SearchPhase::Failed);
                }
            }
        });
    });

    let progress = use_context::<UiState>()
        .transfers
        .duplicate_search_progress();
    let current_run = run_id();
    let matches: Vec<SearchMatch> = progress
        .as_ref()
        .filter(|progress| progress.run_id == current_run)
        .map(|progress| progress.matches.clone())
        .unwrap_or_default();
    let scanned = progress
        .as_ref()
        .filter(|progress| progress.run_id == current_run)
        .map(|progress| progress.scanned)
        .unwrap_or(0);
    let analyzed = progress
        .as_ref()
        .filter(|progress| progress.run_id == current_run)
        .map(|progress| progress.analyzed)
        .unwrap_or(0);
    let unreadable = progress
        .as_ref()
        .filter(|progress| progress.run_id == current_run)
        .map(|progress| progress.unreadable)
        .unwrap_or(0);

    let busy = matches!(phase(), SearchPhase::Preparing | SearchPhase::Searching);
    let in_setup = phase() == SearchPhase::Setup;
    let status_text = match phase() {
        SearchPhase::Setup => "Escolha a pasta e o tipo de busca, depois inicie.".to_string(),
        SearchPhase::Preparing => "Preparando mídia...".to_string(),
        SearchPhase::Searching => {
            if mode() == SearchMode::Exact {
                format!("Busca exata nas subpastas · {scanned} arquivos verificados")
            } else {
                format!("Varredura profunda · {analyzed} mídias em {scanned} arquivos examinados")
            }
        }
        SearchPhase::Done => {
            if mode() == SearchMode::Exact {
                format!("Busca exata concluída · {scanned} arquivos verificados")
            } else {
                format!("Varredura profunda concluída · {analyzed} mídias em {scanned} arquivos")
            }
        }
        SearchPhase::Canceled => "Busca cancelada".to_string(),
        SearchPhase::Failed => error().unwrap_or_default(),
    };

    rsx! {
        div { class: "duplicate-search-backdrop",
            section { class: "duplicate-search-modal",
                header { class: "duplicate-search-header",
                    div {
                        strong { "Procurar arquivo no sistema" }
                        span { class: "hint",
                            if items.len() == 1 {
                                "{items[0].name}"
                            } else {
                                "{items.len()} mídias selecionadas"
                            }
                        }
                    }
                    button {
                        class: "icon-button",
                        title: "Fechar",
                        onclick: move |_| on_close.call(()),
                        Icon { name: IconName::Close, size: 16, class: "icon" }
                    }
                }
                div { class: "duplicate-search-body",
                    div { class: "duplicate-search-status",
                        if busy {
                            span { class: "spinner tiny" }
                        }
                        span { "{status_text}" }
                    }

                    div { class: "duplicate-search-field",
                        label { "Pasta de busca" }
                        div { class: "duplicate-search-folder-row",
                            input {
                                class: "input",
                                r#type: "text",
                                value: "{folder}",
                                disabled: busy,
                                oninput: move |event| folder.set(event.value()),
                            }
                            button {
                                class: "btn small ghost",
                                disabled: busy,
                                onclick: move |_| {
                                    spawn(async move {
                                        if let Some(path) = pick_search_folder().await {
                                            folder.set(path.to_string_lossy().to_string());
                                        }
                                    });
                                },
                                if cfg!(target_os = "android") {
                                    "Pasta do app"
                                } else {
                                    "Escolher…"
                                }
                            }
                        }
                    }

                    div { class: "duplicate-search-controls",
                        button {
                            class: if mode() == SearchMode::Exact { "btn small primary" } else { "btn small ghost" },
                            disabled: busy,
                            onclick: move |_| mode.set(SearchMode::Exact),
                            "Busca exata"
                        }
                        button {
                            class: if mode() == SearchMode::Visual { "btn small primary" } else { "btn small ghost" },
                            disabled: busy,
                            onclick: move |_| mode.set(SearchMode::Visual),
                            "Varredura profunda"
                        }
                    }

                    div { class: "duplicate-search-sources",
                        strong { "Mídias" }
                        for item in items.iter() {
                            div { class: "duplicate-search-item-head",
                                span { "{item.name}" }
                                if item.is_video {
                                    span { class: "pill", "vídeo" }
                                }
                            }
                        }
                    }

                    if unreadable > 0 {
                        p { class: "hint", "{unreadable} arquivos ou pastas não puderam ser lidos." }
                    }

                    if !in_setup {
                        for item in items.iter() {
                            {
                            let item_matches = matches_for(&matches, &item.message_id.to_string());
                            let item_status = match phase() {
                                SearchPhase::Done => "Nenhuma cópia encontrada nesta pasta.".to_string(),
                                SearchPhase::Failed => error().unwrap_or_default(),
                                _ => "Nenhuma correspondência encontrada até agora.".to_string(),
                            };
                            rsx! {
                            div { class: "duplicate-search-item",
                                div { class: "duplicate-search-item-head",
                                    strong { "{item.name}" }
                                    if item.is_video {
                                        span { class: "pill", "vídeo" }
                                    }
                                }
                                if item_matches.is_empty() {
                                    p { class: "hint", "{item_status}" }
                                } else {
                                    ul { class: "duplicate-search-matches",
                                        for entry in item_matches {
                                            li {
                                                span { title: "{entry.path}", "{entry.path}" }
                                                button {
                                                    class: "icon-button",
                                                    title: "Mostrar no gerenciador de arquivos",
                                                    onclick: {
                                                        let path = entry.path.clone();
                                                        move |_| {
                                                            if let Err(message) = plasma_platform::reveal_path(&path) {
                                                                push_toast(toasts, message, ToastKind::Error);
                                                            }
                                                        }
                                                    },
                                                    Icon { name: IconName::Folder, size: 16, class: "icon" }
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
                footer { class: "duplicate-search-footer",
                    if busy {
                        button {
                            class: "btn small ghost",
                            onclick: {
                                let services = services.clone();
                                let run = run_id();
                                move |_| {
                                    services.telegram.cancel_duplicate_search(&run);
                                }
                            },
                            "Cancelar busca"
                        }
                    } else if in_setup || matches!(phase(), SearchPhase::Done | SearchPhase::Canceled | SearchPhase::Failed) {
                        button {
                            class: "btn small primary",
                            disabled: folder().trim().is_empty() || items.is_empty(),
                            onclick: move |_| {
                                if folder().trim().is_empty() {
                                    push_toast(
                                        toasts,
                                        "Escolha uma pasta para procurar.",
                                        ToastKind::Info,
                                    );
                                    return;
                                }
                                let next = start_token().wrapping_add(1);
                                start_token.set(next);
                            },
                            if in_setup { "Iniciar busca" } else { "Buscar de novo" }
                        }
                    }
                    button {
                        class: "btn small ghost",
                        onclick: move |_| on_close.call(()),
                        "Fechar"
                    }
                }
            }
        }
    }
}

fn matches_for(matches: &[SearchMatch], source_id: &str) -> Vec<SearchMatch> {
    matches
        .iter()
        .filter(|entry| entry.source_id == source_id)
        .cloned()
        .collect()
}

async fn pick_search_folder() -> Option<std::path::PathBuf> {
    #[cfg(not(target_os = "android"))]
    {
        rfd::FileDialog::new()
            .set_title("Escolher pasta para procurar cópias")
            .pick_folder()
    }

    #[cfg(target_os = "android")]
    {
        plasma_platform::android_app_dirs()
            .ok()
            .map(|dirs| dirs.download_dir)
    }
}
