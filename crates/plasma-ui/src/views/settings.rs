//! Settings view for the Rust shell (appearance, session, migration).

use crate::state::{apply_density, apply_palette, push_toast, AppServices, ToastKind, UiState};
use crate::theme::{Density, Palette};
use dioxus::prelude::*;
use plasma_store::SettingKey;

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
