//! "Procurar arquivo no sistema": finds copies of the selected media in a folder.

use crate::icons::{Icon, IconName};
use crate::state::{push_toast, AppServices, ToastKind, UiState};
use dioxus::prelude::*;
use plasma_duplicates::{SearchMatch, SearchMode, SearchSource};

/// A media message picked by the user for the search.
#[derive(Clone, Debug, PartialEq)]
pub struct DuplicateSearchItem {
    pub chat_id: i64,
    pub message_id: i64,
    pub name: String,
    pub is_video: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchPhase {
    Preparing,
    Searching,
    Done,
    Canceled,
    Failed,
}

#[component]
pub fn DuplicateSearchDialog(
    items: Vec<DuplicateSearchItem>,
    on_close: EventHandler<()>,
) -> Element {
    let services = use_context::<AppServices>();
    let toasts = use_context::<UiState>().toasts;
    let mut folder = use_signal(|| services.downloads.download_dir().display().to_string());
    let mut mode = use_signal(|| SearchMode::Exact);
    let mut phase = use_signal(|| SearchPhase::Preparing);
    let mut error = use_signal(|| None::<String>);
    let mut run_id = use_signal(String::new);

    // Prepare the media and run the search once the dialog opens.
    let search_items = items.clone();
    let search_services = services.clone();
    use_effect(move || {
        let services = search_services.clone();
        let items = search_items.clone();
        let run = format!(
            "ui-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|value| value.as_nanos())
                .unwrap_or_default()
        );
        run_id.set(run.clone());
        spawn(async move {
            let mut sources: Vec<SearchSource> = Vec::new();
            for item in &items {
                match services
                    .telegram
                    .ensure_media_cached(item.chat_id, item.message_id)
                    .await
                {
                    Ok(meta) => {
                        let Some(path) = meta.native_file_path else {
                            continue;
                        };
                        sources.push(SearchSource::from_path(
                            item.message_id.to_string(),
                            item.name.clone(),
                            path,
                            item.is_video,
                        ));
                    }
                    Err(message) => {
                        error.set(Some(message));
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
                .search_duplicate_media(run.clone(), folder().clone(), sources, Some(mode()))
                .await;
            match result {
                Ok(result) => {
                    if result.canceled {
                        phase.set(SearchPhase::Canceled);
                    } else {
                        phase.set(SearchPhase::Done);
                    }
                }
                Err(message) => {
                    error.set(Some(message));
                    phase.set(SearchPhase::Failed);
                }
            }
        });
    });

    let progress = use_context::<UiState>().duplicate_search_progress();
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
    let status_text = match phase() {
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
                        span { class: "hint", "{folder}" }
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
                            "Trocar pasta"
                        }
                    }
                    if unreadable > 0 {
                        p { class: "hint", "{unreadable} arquivos ou pastas não puderam ser lidos." }
                    }
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
        None
    }
}
