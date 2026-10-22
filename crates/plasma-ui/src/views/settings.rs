//! Settings view for the Rust shell (appearance, session, cache, migration).

use crate::state::{apply_density, apply_palette, push_toast, AppServices, ToastKind, UiState};
use crate::theme::{Density, Palette};
use crate::views::select::{Select, SelectOption};
use dioxus::prelude::*;
use plasma_store::SettingKey;
use plasma_telegram::NativeMediaCacheStats;

/// Cache limits offered to the user, in bytes (`0` means unlimited).
const CACHE_LIMITS: &[(i64, &str)] = &[
    (0, "Sem limite"),
    (500 * 1024 * 1024, "500 MB"),
    (1024 * 1024 * 1024, "1 GB"),
    (2 * 1024 * 1024 * 1024, "2 GB"),
    (5 * 1024 * 1024 * 1024, "5 GB"),
    (10 * 1024 * 1024 * 1024, "10 GB"),
];

const LEGACY_KEYS: &[&str] = &[
    "skip_login",
    "telegram_tdlib_session",
    "plasma_appearance_palette",
    "plasma_appearance_density",
    "plasma_info_panel_open",
    "plasma_chat_media_size",
    "plasma_twitter_optional_cookies",
    "clear_twitter_groups_on_clear_cache",
    "plasma_debug_logs",
];

#[component]
pub fn SettingsView() -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();
    let mut skip_login = use_signal(|| {
        services
            .settings
            .get_bool(SettingKey::SKIP_LOGIN)
            .unwrap_or(false)
    });
    let palette = ui.palette();
    let density = ui.density();

    let mut cache_stats = use_signal(|| None::<NativeMediaCacheStats>);
    let mut cache_limit = use_signal(|| {
        services
            .settings
            .get(SettingKey::MEDIA_CACHE_LIMIT)
            .ok()
            .flatten()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(0)
    });
    let mut clearing_cache = use_signal(|| false);
    let mut confirm_clear = use_signal(|| false);

    // The cache stats are read once when the screen opens.
    let stats_services = services.clone();
    use_effect(move || {
        let services = stats_services.clone();
        spawn(async move {
            if let Ok(stats) = services.telegram.media_cache_stats().await {
                cache_stats.set(Some(stats));
            }
        });
    });

    let database_path = services
        .env
        .paths()
        .data_dir()
        .join("plasma-hub.sqlite3")
        .display()
        .to_string();
    let data_dir = services.env.paths().data_dir().display().to_string();
    let cache_dir = services.env.paths().cache_dir().display().to_string();

    rsx! {
        section { class: "settings-view",
            div { class: "settings-inner",
                header { class: "settings-heading",
                    h1 { "Ajustes" }
                    p { class: "muted",
                        "Esta tela já lê e grava no mesmo SQLite usado pelo app React. \
                         As chaves são idênticas às do localStorage."
                    }
                }

                section { class: "settings-card",
                    h2 { "Aparência" }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Paleta" }
                            span { "Seis paletas, aplicadas via tokens CSS." }
                        }
                        div { class: "palette-row",
                            for option in Palette::ALL {
                                button {
                                    key: "{option.as_str()}",
                                    class: if palette == option { "palette-chip active" } else { "palette-chip" },
                                    style: "--swatch: {option.swatch()};",
                                    onclick: {
                                        let services = services.clone();
                                        move |_| apply_palette(&services, ui, option)
                                    },
                                    "{option.label()}"
                                }
                            }
                        }
                    }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Densidade" }
                            span { "Altura das linhas de chat e espaçamento." }
                        }
                        div { class: "segmented",
                            for option in Density::ALL {
                                button {
                                    key: "{option.as_str()}",
                                    class: if density == option { "active" } else { "" },
                                    onclick: {
                                        let services = services.clone();
                                        move |_| apply_density(&services, ui, option)
                                    },
                                    "{option.label()}"
                                }
                            }
                        }
                    }
                }

                section { class: "settings-card",
                    h2 { "Sessão" }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Pular login" }
                            span { "Quando ativo, o shell abre direto no dashboard." }
                        }
                        button {
                            class: if skip_login() { "btn small primary" } else { "btn small ghost" },
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    let next = !skip_login();
                                    skip_login.set(next);
                                    match services.settings.set_bool(SettingKey::SKIP_LOGIN, next) {
                                        Ok(()) => push_toast(
                                            ui.toasts,
                                            if next { "Login será pulado no próximo start." } else { "Login volta a ser solicitado." },
                                            ToastKind::Info,
                                        ),
                                        Err(error) => push_toast(
                                            ui.toasts,
                                            format!("Falha ao salvar: {error}"),
                                            ToastKind::Error,
                                        ),
                                    }
                                }
                            },
                            if skip_login() { "Ativado" } else { "Desativado" }
                        }
                    }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Voltar para a tela de login" }
                            span { "Útil para testar o fluxo sem reiniciar o app." }
                        }
                        button {
                            class: "btn small ghost",
                            onclick: move |_| ui.logged_in.set(false),
                            "Mostrar login"
                        }
                    }
                }

                section { class: "settings-card",
                    h2 { "Cache de mídia" }
                    p { class: "hint",
                        "Mídias baixadas do Telegram ficam em um cache local. O limite é aplicado \
                         apagando os arquivos mais antigos quando o espaço é atingido."
                    }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Cache nativo" }
                            span {
                                match &cache_stats() {
                                    Some(stats) if stats.success => format!(
                                        "{} em {} mídias",
                                        format_bytes(stats.total_size),
                                        stats.media_count
                                    ),
                                    Some(stats) => stats
                                        .error
                                        .clone()
                                        .unwrap_or_else(|| "Indisponível".to_string()),
                                    None => "Calculando...".to_string(),
                                }
                            }
                        }
                        button {
                            class: if confirm_clear() { "btn small danger" } else { "btn small ghost" },
                            disabled: clearing_cache(),
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    if !confirm_clear() {
                                        confirm_clear.set(true);
                                        return;
                                    }
                                    confirm_clear.set(false);
                                    clearing_cache.set(true);
                                    let services = services.clone();
                                    spawn(async move {
                                        match services.telegram.clear_media_cache().await {
                                            Ok(stats) => {
                                                cache_stats.set(Some(stats));
                                                push_toast(
                                                    ui.toasts,
                                                    "Cache de mídia limpo.",
                                                    ToastKind::Success,
                                                );
                                            }
                                            Err(error) => push_toast(
                                                ui.toasts,
                                                format!("Falha ao limpar o cache: {error}"),
                                                ToastKind::Error,
                                            ),
                                        }
                                        clearing_cache.set(false);
                                    });
                                }
                            },
                            if clearing_cache() {
                                "Limpando..."
                            } else if confirm_clear() {
                                "Confirmar limpeza"
                            } else {
                                "Limpar cache"
                            }
                        }
                    }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Limite do cache" }
                            span { "Espaço máximo usado pelas mídias baixadas." }
                        }
                        div { class: "setting-row-control",
                            Select {
                                value: cache_limit().to_string(),
                                options: CACHE_LIMITS
                                    .iter()
                                    .map(|(bytes, label)| {
                                        SelectOption::new(bytes.to_string(), (*label).to_string())
                                    })
                                    .collect(),
                                placeholder: "Sem limite",
                                disabled: false,
                                on_change: {
                                    let services = services.clone();
                                    move |value: String| {
                                        let bytes = value.parse::<i64>().unwrap_or(0);
                                        cache_limit.set(bytes);
                                        let services = services.clone();
                                        spawn(async move {
                                            if let Err(error) = services
                                                .settings
                                                .set(SettingKey::MEDIA_CACHE_LIMIT, &bytes.to_string())
                                            {
                                                push_toast(
                                                    ui.toasts,
                                                    format!("Falha ao salvar o limite: {error}"),
                                                    ToastKind::Error,
                                                );
                                            }
                                            match services.telegram.evict_media_cache(bytes).await {
                                                Ok(stats) => cache_stats.set(Some(stats)),
                                                Err(error) => push_toast(
                                                    ui.toasts,
                                                    format!("Falha ao aplicar o limite: {error}"),
                                                    ToastKind::Error,
                                                ),
                                            }
                                        });
                                    }
                                },
                            }
                        }
                    }
                }

                section { class: "settings-card",
                    h2 { "Migração do app antigo" }
                    p { class: "hint",
                        "Lê as chaves globais do localStorage desta WebView e copia para o SQLite. \
                         A migração real do app Tauri também roda no boot do frontend React."
                    }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Importar preferências" }
                            span { "Valores já gravados no SQLite não são sobrescritos." }
                        }
                        button {
                            class: "btn small",
                            onclick: {
                                let services = services.clone();
                                move |_| import_legacy(services.clone(), ui)
                            },
                            "Importar do localStorage"
                        }
                    }
                }

                section { class: "settings-card",
                    h2 { "Diagnóstico" }
                    dl { class: "kv",
                        dt { "Banco" }
                        dd { "{database_path}" }
                        dt { "Dados" }
                        dd { "{data_dir}" }
                        dt { "Cache" }
                        dd { "{cache_dir}" }
                        dt { "Schema" }
                        dd { "v{plasma_store::db::SCHEMA_VERSION}" }
                        dt { "Janela" }
                        dd { "Dioxus 0.7.10 / WebView" }
                    }
                }
            }
        }
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
        format!("{:.0} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{bytes:.0} B")
    }
}

fn import_legacy(services: AppServices, ui: UiState) {
    spawn(async move {
        let keys = LEGACY_KEYS
            .iter()
            .map(|key| format!("{key:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        let script = format!(
            r#"
            (() => {{
                const keys = [{keys}];
                const entries = [];
                for (const key of keys) {{
                    try {{
                        const value = window.localStorage.getItem(key);
                        if (value !== null) entries.push([key, value]);
                    }} catch (_) {{
                        // localStorage can be unavailable; keep the values we have.
                    }}
                }}
                dioxus.send(entries);
            }})()
            "#
        );

        let mut eval = dioxus::document::eval(&script);
        match eval.recv::<Vec<(String, String)>>().await {
            Ok(entries) if entries.is_empty() => push_toast(
                ui.toasts,
                "Nenhuma preferência encontrada nesta WebView.",
                ToastKind::Info,
            ),
            Ok(entries) => match services.settings.import_legacy(entries) {
                Ok(count) => push_toast(
                    ui.toasts,
                    format!("{count} preferência(s) importada(s) para o SQLite."),
                    ToastKind::Success,
                ),
                Err(error) => push_toast(
                    ui.toasts,
                    format!("Falha ao importar: {error}"),
                    ToastKind::Error,
                ),
            },
            Err(error) => push_toast(
                ui.toasts,
                format!("Falha ao ler o localStorage: {error}"),
                ToastKind::Error,
            ),
        }
    });
}
