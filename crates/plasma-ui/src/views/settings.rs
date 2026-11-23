//! Settings view featuring the interactive 4-Style visual gallery, color mode,
//! density, session controls, native media cache and diagnostics.

use crate::icons::{Icon, IconName};
use crate::state::{
    apply_color_mode, apply_density, apply_style, push_toast, set_drawer_open, AppServices,
    ToastKind, UiState,
};
use crate::theme::{ColorMode, Density, UiStyle};
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
    "plasma_twitter_fake_chats",
    "plasma_twitter_pending_fake_chat",
    "plasma_debug_logs",
];

/// Miniature preview palette for a style card in the gallery.
fn style_preview_colors(
    style: UiStyle,
    mode: ColorMode,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    match (style, mode) {
        (UiStyle::Native, ColorMode::Dark) => {
            ("#0f131a", "#161b24", "#0f131a", "#1c2330", "#206191", "0px")
        }
        (UiStyle::Native, ColorMode::Light) => {
            ("#e9eef5", "#ffffff", "#eef3f8", "#ffffff", "#0088cc", "0px")
        }
        (UiStyle::Tonal, ColorMode::Dark) => {
            ("#0c0e15", "#161925", "#131620", "#1f2334", "#7c8cff", "7px")
        }
        (UiStyle::Tonal, ColorMode::Light) => {
            ("#e4e8f4", "#f8f9fd", "#ffffff", "#e4e8f7", "#4f5ee8", "7px")
        }
        (UiStyle::Community, ColorMode::Dark) => {
            ("#18191c", "#1e1f23", "#26282d", "#32353c", "#5865f2", "0px")
        }
        (UiStyle::Community, ColorMode::Light) => {
            ("#ebedef", "#f2f3f5", "#ffffff", "#e6e8ec", "#4752c4", "0px")
        }
        (UiStyle::Warm, ColorMode::Dark) => {
            ("#121110", "#1b1917", "#161413", "#262320", "#e07a5f", "6px")
        }
        (UiStyle::Warm, ColorMode::Light) => {
            ("#ebe6de", "#f7f4ef", "#fcfbf9", "#ebe5dc", "#c85a3b", "6px")
        }
    }
}

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
    let mut clear_twitter_on_cache = use_signal(|| {
        services
            .settings
            .get_bool(SettingKey::CLEAR_TWITTER_GROUPS_ON_CLEAR_CACHE)
            .unwrap_or(false)
    });

    let active_style = ui.shell.style();
    let active_mode = ui.shell.color_mode();
    let density = ui.shell.density();
    let drawer_open = ui.shell.drawer_open();

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
    let crash_log = services
        .env
        .paths()
        .local_data_dir()
        .join("crash.log")
        .display()
        .to_string();
    let app_version = env!("CARGO_PKG_VERSION");
    let platform = format!("{} / {}", std::env::consts::OS, std::env::consts::ARCH);
    let open_data_dir = data_dir.clone();
    let open_crash_log = crash_log.clone();

    rsx! {
        section { class: "settings-view",
            div { class: "settings-inner",
                header { class: "settings-heading",
                    div { class: "page-title-row",
                        if !ui.shell.sidebar_open() {
                            button {
                                class: "icon-button",
                                title: "Mostrar lista de conversas (Ctrl+B)",
                                aria_label: "Mostrar lista de conversas",
                                onclick: move |_| ui.shell.sidebar_open.set(true),
                                Icon { name: IconName::LayoutSidebar, size: 18, class: "icon" }
                            }
                        }
                        div {
                            h1 { "Ajustes" }
                            p { class: "muted",
                                "Personalize o estilo do mensageiro ao vivo, gerencie o cache nativo \
                                 e controle sua sessão."
                            }
                        }
                    }
                    div { class: "downloads-heading-actions",
                        button {
                            class: if drawer_open { "btn small active" } else { "btn small ghost" },
                            onclick: {
                                let services = services.clone();
                                move |_| set_drawer_open(&services, ui, !drawer_open)
                            },
                            Icon { name: IconName::LayoutSidebarRight, size: 15, class: "icon" }
                            if drawer_open { "Ocultar gaveta" } else { "Mostrar gaveta" }
                        }
                    }
                }

                section { class: "settings-card",
                    div { class: "drawer-section-header",
                        div {
                            h2 { "Estilo do Mensageiro" }
                            p { class: "hint",
                                "Todos os 4 estilos usam um único motor CSS sem custo extra de performance."
                            }
                        }
                        div { class: "segmented",
                            for option in ColorMode::ALL {
                                button {
                                    key: "{option.as_str()}",
                                    class: if active_mode == option { "active" } else { "" },
                                    onclick: {
                                        let services = services.clone();
                                        move |_| apply_color_mode(&services, ui, option)
                                    },
                                    "{option.label()}"
                                }
                            }
                        }
                    }

                    div { class: "style-gallery",
                        for option in UiStyle::ALL {
                            {
                                let (frame_bg, side_bg, stage_bg, bub_in, bub_out, island_r) =
                                    style_preview_colors(option, active_mode);
                                let is_community = option == UiStyle::Community;
                                rsx! {
                                    button {
                                        key: "{option.as_str()}",
                                        class: if active_style == option { "style-card active" } else { "style-card" },
                                        onclick: {
                                            let services = services.clone();
                                            move |_| apply_style(&services, ui, option)
                                        },
                                        div {
                                            class: "style-preview-box",
                                            style: "background: {frame_bg};",
                                            div {
                                                class: "style-preview-side",
                                                style: "background: {side_bg}; border-radius: {island_r};",
                                                div {
                                                    style: "height: 8px; width: 75%; border-radius: 99px; background: {bub_out}; opacity: 0.85;",
                                                }
                                                div {
                                                    style: "height: 7px; width: 90%; border-radius: 99px; background: {bub_in};",
                                                }
                                                div {
                                                    style: "height: 7px; width: 65%; border-radius: 99px; background: {bub_in};",
                                                }
                                            }
                                            div {
                                                class: "style-preview-main",
                                                style: "background: {stage_bg}; border-radius: {island_r};",
                                                div {
                                                    class: "style-preview-bubble-in",
                                                    style: "background: {bub_in};",
                                                }
                                                div {
                                                    class: "style-preview-bubble-out",
                                                    style: if is_community {
                                                        format!("align-self: flex-start; width: 82%; background: {bub_in}; border-left: 3px solid {bub_out};")
                                                    } else {
                                                        format!("background: {bub_out};")
                                                    },
                                                }
                                            }
                                        }
                                        div { class: "style-card-title",
                                            strong { "{option.label()}" }
                                            span {
                                                class: "style-swatch-dot",
                                                style: "background: {option.swatch()};",
                                            }
                                        }
                                        p { class: "hint", "{option.description()}" }
                                    }
                                }
                            }
                        }
                    }

                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Densidade das listas e mensagens" }
                            span { "Ajusta a altura das conversas e o respiro entre mensagens." }
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
                    h2 { "Conta & Sessão" }
                    if let Some(user) = ui.session.auth_user() {
                        div { class: "setting-row",
                            div { class: "setting-row-copy",
                                strong { "Conectado como {user}" }
                                span { "Sessão TDLib ativa neste dispositivo." }
                            }
                            span { class: "pill good", "Online" }
                        }
                    }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Pular tela de login na inicialização" }
                            span { "Abre direto nas conversas ou modo prévia ao iniciar o app." }
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
                                            ui.shell.toasts,
                                            if next {
                                                "Login será pulado na próxima abertura."
                                            } else {
                                                "Login voltará a ser exibido ao iniciar."
                                            },
                                            ToastKind::Info,
                                        ),
                                        Err(error) => push_toast(
                                            ui.shell.toasts,
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
                            strong { "Sessão do Telegram" }
                            span { "Voltar para a tela de login ou encerrar a conta no TDLib." }
                        }
                        div { class: "settings-inline-actions",
                            button {
                                class: "btn small ghost",
                                onclick: move |_| ui.shell.logged_in.set(false),
                                "Tela de Login"
                            }
                            button {
                                class: "btn small danger",
                                onclick: {
                                    let services = services.clone();
                                    move |_| {
                                        let services = services.clone();
                                        spawn(async move {
                                            match services.telegram.log_out().await {
                                                Ok(_) => {
                                                    ui.shell.logged_in.set(false);
                                                    ui.session.preview_mode.set(false);
                                                    ui.session.auth_user.set(None);
                                                    ui.conversation.chats.set(Vec::new());
                                                    ui.conversation.selected_chat.set(None);
                                                    push_toast(
                                                        ui.shell.toasts,
                                                        "Sessão Telegram encerrada.",
                                                        ToastKind::Success,
                                                    );
                                                }
                                                Err(error) => push_toast(
                                                    ui.shell.toasts,
                                                    format!("Falha ao sair: {error}"),
                                                    ToastKind::Error,
                                                ),
                                            }
                                        });
                                    }
                                },
                                "Encerrar sessão"
                            }
                        }
                    }
                }

                section { class: "settings-card",
                    h2 { "Cache de Mídia" }
                    p { class: "hint",
                        "Fotos e vídeos visualizados ficam em cache local para abertura instantânea. \
                         O limite remove automaticamente os itens mais antigos."
                    }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Espaço em uso" }
                            span {
                                match &cache_stats() {
                                    Some(stats) if stats.success => format!(
                                        "{} distribuídos em {} mídias",
                                        format_bytes(stats.total_size),
                                        stats.media_count
                                    ),
                                    Some(stats) => stats
                                        .error
                                        .clone()
                                        .unwrap_or_else(|| "Indisponível".to_string()),
                                    None => "Calculando uso do cache…".to_string(),
                                }
                            }
                        }
                        button {
                            class: if confirm_clear() { "btn small danger" } else { "btn small soft" },
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
                                        let clear_twitter = services
                                            .settings
                                            .get_bool(SettingKey::CLEAR_TWITTER_GROUPS_ON_CLEAR_CACHE)
                                            .unwrap_or(false);
                                        match services.telegram.clear_media_cache().await {
                                            Ok(stats) => {
                                                cache_stats.set(Some(stats));
                                                if clear_twitter {
                                                    if let Err(error) =
                                                        crate::twitter_fake::clear_twitter_fake_chats(
                                                            &services.settings,
                                                        )
                                                    {
                                                        push_toast(
                                                            ui.shell.toasts,
                                                            format!(
                                                                "Cache limpo, mas galerias X falharam: {error}"
                                                            ),
                                                            ToastKind::Error,
                                                        );
                                                    } else {
                                                        let mut chats = ui.conversation.chats();
                                                        chats.retain(|chat| {
                                                            !crate::twitter_fake::is_twitter_fake_chat_id(
                                                                &chat.id,
                                                            )
                                                        });
                                                        ui.conversation.chats.set(chats);
                                                        if ui
                                                            .conversation
                                                            .selected_chat()
                                                            .is_some_and(|chat| {
                                                                crate::twitter_fake::is_twitter_fake_chat_id(
                                                                    &chat.id,
                                                                )
                                                            })
                                                        {
                                                            ui.conversation.selected_chat.set(None);
                                                            ui.conversation.messages.set(Vec::new());
                                                        }
                                                        push_toast(
                                                            ui.shell.toasts,
                                                            "Cache de mídia e galerias X limpos.",
                                                            ToastKind::Success,
                                                        );
                                                    }
                                                } else {
                                                    push_toast(
                                                        ui.shell.toasts,
                                                        "Cache de mídia limpo.",
                                                        ToastKind::Success,
                                                    );
                                                }
                                            }
                                            Err(error) => push_toast(
                                                ui.shell.toasts,
                                                format!("Falha ao limpar o cache: {error}"),
                                                ToastKind::Error,
                                            ),
                                        }
                                        clearing_cache.set(false);
                                    });
                                }
                            },
                            if clearing_cache() {
                                "Limpando…"
                            } else if confirm_clear() {
                                "Confirmar limpeza"
                            } else {
                                "Limpar cache"
                            }
                        }
                    }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Limite máximo do cache" }
                            span { "Arquivos mais antigos são removidos quando o teto é atingido." }
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
                                                    ui.shell.toasts,
                                                    format!("Falha ao salvar o limite: {error}"),
                                                    ToastKind::Error,
                                                );
                                            }
                                            match services.telegram.evict_media_cache(bytes).await {
                                                Ok(stats) => cache_stats.set(Some(stats)),
                                                Err(error) => push_toast(
                                                    ui.shell.toasts,
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
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Remover galerias X ao limpar o cache" }
                            span { "Quando ativo, limpar o cache também remove perfis X seguidos." }
                        }
                        button {
                            class: if clear_twitter_on_cache() { "btn small primary" } else { "btn small ghost" },
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    let next = !clear_twitter_on_cache();
                                    clear_twitter_on_cache.set(next);
                                    let _ = services.settings.set_bool(
                                        SettingKey::CLEAR_TWITTER_GROUPS_ON_CLEAR_CACHE,
                                        next,
                                    );
                                }
                            },
                            if clear_twitter_on_cache() { "Sim" } else { "Não" }
                        }
                    }
                }

                section { class: "settings-card",
                    h2 { "Importação & Dados" }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Importar preferências legadas" }
                            span { "Resgata chaves antigas do localStorage sem sobrescrever o SQLite." }
                        }
                        button {
                            class: "btn small soft",
                            onclick: {
                                let services = services.clone();
                                move |_| import_legacy(services.clone(), ui)
                            },
                            "Importar dados antigos"
                        }
                    }
                }

                section { class: "settings-card",
                    h2 { "Sobre & Diagnóstico" }
                    dl { class: "kv",
                        dt { "Versão" }
                        dd { "v{app_version}" }
                        dt { "Plataforma" }
                        dd { "{platform}" }
                        dt { "Banco SQLite" }
                        dd { "{database_path}" }
                        dt { "Diretório" }
                        dd { "{data_dir}" }
                        dt { "Cache" }
                        dd { "{cache_dir}" }
                        dt { "Crash Log" }
                        dd { "{crash_log}" }
                        dt { "Schema" }
                        dd { "v{plasma_store::db::SCHEMA_VERSION}" }
                    }
                    div { class: "setting-row",
                        div { class: "setting-row-copy",
                            strong { "Acesso rápido aos diretórios" }
                            span { "Abra a pasta local do aplicativo ou inspecione o registro de falhas." }
                        }
                        div { class: "settings-inline-actions",
                            button {
                                class: "btn small ghost",
                                onclick: move |_| {
                                    if let Err(error) = plasma_platform::open_path(&open_data_dir) {
                                        push_toast(ui.shell.toasts, error, ToastKind::Error);
                                    }
                                },
                                Icon { name: IconName::Folder, size: 15, class: "icon" }
                                "Abrir pasta de dados"
                            }
                            button {
                                class: "btn small ghost",
                                onclick: move |_| {
                                    if let Err(error) = plasma_platform::open_path(&open_crash_log) {
                                        push_toast(ui.shell.toasts, error, ToastKind::Error);
                                    }
                                },
                                "Abrir log de falhas"
                            }
                        }
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

fn import_legacy(services: AppServices, mut ui: UiState) {
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
                    }} catch (_) {{}}
                }}
                dioxus.send(entries);
            }})()
            "#
        );

        let mut eval = dioxus::document::eval(&script);
        match eval.recv::<Vec<(String, String)>>().await {
            Ok(entries) if entries.is_empty() => push_toast(
                ui.shell.toasts,
                "Nenhuma preferência antiga encontrada nesta WebView.",
                ToastKind::Info,
            ),
            Ok(entries) => {
                let mut twitter_payload = None::<String>;
                let filtered: Vec<(String, String)> = entries
                    .into_iter()
                    .filter(|(key, value)| {
                        if key == SettingKey::TWITTER_FAKE_CHATS.as_str() {
                            twitter_payload = Some(value.clone());
                            false
                        } else {
                            true
                        }
                    })
                    .collect();
                let mut imported = match services.settings.import_legacy(filtered) {
                    Ok(count) => count,
                    Err(error) => {
                        push_toast(
                            ui.shell.toasts,
                            format!("Falha ao importar: {error}"),
                            ToastKind::Error,
                        );
                        return;
                    }
                };
                if let Some(raw) = twitter_payload {
                    match serde_json::from_str::<Vec<crate::twitter_fake::TwitterFakeChat>>(&raw) {
                        Ok(legacy) => match crate::twitter_fake::merge_legacy_chats(
                            &services.settings,
                            legacy,
                        ) {
                            Ok(added) => {
                                imported += added;
                                let mut chats = ui.conversation.chats();
                                crate::twitter_fake::merge_fake_dialogs_into(
                                    &mut chats,
                                    &services.settings,
                                );
                                ui.conversation.chats.set(chats);
                            }
                            Err(error) => push_toast(
                                ui.shell.toasts,
                                format!("Falha ao importar galerias Twitter: {error}"),
                                ToastKind::Error,
                            ),
                        },
                        Err(error) => push_toast(
                            ui.shell.toasts,
                            format!("JSON inválido em plasma_twitter_fake_chats: {error}"),
                            ToastKind::Error,
                        ),
                    }
                }
                if imported == 0 {
                    push_toast(ui.shell.toasts, "Nada novo para importar.", ToastKind::Info);
                } else {
                    push_toast(
                        ui.shell.toasts,
                        format!("{imported} item(ns) importado(s) para o SQLite."),
                        ToastKind::Success,
                    );
                }
            }
            Err(error) => push_toast(
                ui.shell.toasts,
                format!("Falha ao ler o localStorage: {error}"),
                ToastKind::Error,
            ),
        }
    });
}
