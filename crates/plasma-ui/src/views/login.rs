//! Telegram login (TDLib) and preview entry point for the Rust shell.

use crate::state::{
    apply_auth_error, apply_auth_status, apply_palette, push_toast, AppServices, ToastKind, UiState,
};
use crate::theme::Palette;
use dioxus::prelude::*;
use plasma_store::SettingKey;

#[component]
pub fn LoginView() -> Element {
    let services = use_context::<AppServices>();
    let mut ui = use_context::<UiState>();

    let mut phone = use_signal(String::new);
    let mut code = use_signal(String::new);
    let mut password = use_signal(String::new);
    let mut busy = use_signal(|| false);
    let mut form_error = use_signal(|| None::<String>);

    let state = ui.auth_state();
    let booting = ui.booting();
    let error = ui.auth_error();
    let busy_value = busy();

    rsx! {
        div { class: "login-screen",
            div { class: "login-card",
                div { class: "brand-mark", "P" }
                h1 { "Plasma Hub" }
                p { class: "muted",
                    "Entre com sua conta do Telegram ou abra a prévia da interface Rust."
                }

                if booting {
                    div { class: "login-status",
                        div { class: "spinner" }
                        strong { "Conectando ao Telegram" }
                        p { class: "hint", "Inicializando o TDLib e restaurando a sessão..." }
                    }
                } else if state == "ready" {
                    div { class: "login-status",
                        strong { "Conta conectada" }
                        p { class: "hint", "A sessão do Telegram já está pronta." }
                    }
                    div { class: "login-actions",
                        button {
                            class: "btn primary",
                            onclick: move |_| {
                                ui.preview_mode.set(false);
                                ui.logged_in.set(true);
                            },
                            "Abrir dashboard"
                        }
                    }
                } else if state == "wait_code" {
                    div { class: "login-status",
                        strong { "Código de confirmação" }
                        p { class: "hint", "Digite o código enviado pelo Telegram." }
                    }
                    input {
                        class: "login-input",
                        r#type: "text",
                        inputmode: "numeric",
                        autocomplete: "one-time-code",
                        placeholder: "12345",
                        maxlength: "16",
                        value: "{code}",
                        oninput: move |event| code.set(event.value()),
                    }
                    div { class: "login-actions",
                        button {
                            class: "btn primary",
                            disabled: busy_value,
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    if busy() { return; }
                                    let value = code().trim().to_string();
                                    if value.is_empty() {
                                        form_error.set(Some("Informe o código recebido.".to_string()));
                                        return;
                                    }
                                    busy.set(true);
                                    form_error.set(None);
                                    let services = services.clone();
                                    spawn(async move {
                                        match services.telegram.check_code(value).await {
                                            Ok(status) => finish_status(services.clone(), ui, status).await,
                                            Err(error) => {
                                                apply_auth_error(ui, error.clone());
                                                form_error.set(Some(error));
                                            }
                                        }
                                        busy.set(false);
                                    });
                                }
                            },
                            if busy_value { "Verificando..." } else { "Confirmar código" }
                        }
                    }
                } else if state == "wait_password" {
                    div { class: "login-status",
                        strong { "Senha de duas etapas" }
                        p { class: "hint", "Sua conta tem verificação em duas etapas ativa." }
                    }
                    input {
                        class: "login-input",
                        r#type: "password",
                        autocomplete: "current-password",
                        placeholder: "Senha do Telegram",
                        value: "{password}",
                        oninput: move |event| password.set(event.value()),
                    }
                    div { class: "login-actions",
                        button {
                            class: "btn primary",
                            disabled: busy_value,
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    if busy() { return; }
                                    let value = password().to_string();
                                    if value.is_empty() {
                                        form_error.set(Some("Informe a senha.".to_string()));
                                        return;
                                    }
                                    busy.set(true);
                                    form_error.set(None);
                                    let services = services.clone();
                                    spawn(async move {
                                        match services.telegram.check_password(value).await {
                                            Ok(status) => finish_status(services.clone(), ui, status).await,
                                            Err(error) => {
                                                apply_auth_error(ui, error.clone());
                                                form_error.set(Some(error));
                                            }
                                        }
                                        busy.set(false);
                                    });
                                }
                            },
                            if busy_value { "Verificando..." } else { "Entrar" }
                        }
                    }
                } else if matches!(
                    state.as_str(),
                    "wait_other_device_confirmation" | "wait_registration" | "wait_email_address"
                        | "wait_email_code"
                ) {
                    div { class: "login-status",
                        strong { "Confirmação adicional" }
                        p { class: "hint",
                            "Este tipo de confirmação ainda não está disponível no shell Rust. \
                             Conclua o login no app Tauri e tente novamente."
                        }
                    }
                } else if state == "error" || state == "closed" || state == "closing" {
                    div { class: "login-status error",
                        strong { "Não foi possível conectar" }
                        p { class: "hint",
                            {error.clone().unwrap_or_else(|| "TDLib foi encerrado. Tente novamente.".to_string())}
                        }
                    }
                    div { class: "login-actions",
                        button {
                            class: "btn primary",
                            disabled: busy_value,
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    if busy() { return; }
                                    let Some(credentials) = services.credentials.clone() else {
                                        form_error.set(Some(
                                            "Credenciais ausentes. Defina VITE_API_ID e VITE_API_HASH no .env."
                                                .to_string(),
                                        ));
                                        return;
                                    };
                                    busy.set(true);
                                    form_error.set(None);
                                    let services = services.clone();
                                    spawn(async move {
                                        ui.auth_state.set("initializing".to_string());
                                        match services
                                            .telegram
                                            .init(credentials.api_id, credentials.api_hash)
                                            .await
                                        {
                                            Ok(status) => finish_status(services.clone(), ui, status).await,
                                            Err(error) => {
                                                apply_auth_error(ui, error.clone());
                                                form_error.set(Some(error));
                                            }
                                        }
                                        busy.set(false);
                                    });
                                }
                            },
                            if busy_value { "Reconectando..." } else { "Tentar novamente" }
                        }
                    }
                } else if state == "not_initialized" && !booting {
                    div { class: "login-status",
                        strong { "Credenciais não configuradas" }
                        p { class: "hint",
                            {error.clone().unwrap_or_else(|| {
                                "Defina VITE_API_ID e VITE_API_HASH no .env e recompile o shell."
                                    .to_string()
                            })}
                        }
                    }
                } else {
                    div { class: "login-status",
                        strong { "Telefone" }
                        p { class: "hint", "Use o número completo com código do país, por exemplo +5511999999999." }
                    }
                    input {
                        class: "login-input",
                        r#type: "tel",
                        inputmode: "tel",
                        autocomplete: "tel",
                        placeholder: "+55 11 99999-9999",
                        value: "{phone}",
                        oninput: move |event| phone.set(event.value()),
                    }
                    div { class: "login-actions",
                        button {
                            class: "btn primary",
                            disabled: busy_value,
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    if busy() { return; }
                                    let Some(value) = normalize_phone(&phone()) else {
                                        form_error.set(Some(
                                            "Informe o número com DDI, por exemplo +5511999999999."
                                                .to_string(),
                                        ));
                                        return;
                                    };
                                    busy.set(true);
                                    form_error.set(None);
                                    let services = services.clone();
                                    spawn(async move {
                                        match services.telegram.set_phone(value).await {
                                            Ok(status) => finish_status(services.clone(), ui, status).await,
                                            Err(error) => {
                                                apply_auth_error(ui, error.clone());
                                                form_error.set(Some(error));
                                            }
                                        }
                                        busy.set(false);
                                    });
                                }
                            },
                            if busy_value { "Enviando..." } else { "Enviar código" }
                        }
                    }
                }

                if let Some(message) = form_error().or_else(|| {
                    if state == "wait_phone_number" || state == "wait_code" || state == "wait_password" {
                        error.clone()
                    } else {
                        None
                    }
                }) {
                    p { class: "login-error", "{message}" }
                }

                div { class: "login-divider" }
                div { class: "login-actions",
                    button {
                        class: "btn ghost",
                        onclick: {
                            let services = services.clone();
                            move |_| {
                                ui.preview_mode.set(true);
                                ui.logged_in.set(true);
                                match services.settings.set_bool(SettingKey::SKIP_LOGIN, true) {
                                    Ok(()) => push_toast(
                                        ui.toasts,
                                        "Prévia aberta sem conta. Você pode desativar isso nos ajustes.",
                                        ToastKind::Info,
                                    ),
                                    Err(error) => push_toast(
                                        ui.toasts,
                                        format!("Prévia aberta, mas a preferência não foi salva: {error}"),
                                        ToastKind::Error,
                                    ),
                                }
                            }
                        },
                        "Continuar sem conta"
                    }
                }

                div { class: "login-divider" }
                p { class: "hint", "Aparência" }
                div { class: "palette-row",
                    for palette in Palette::ALL {
                        button {
                            key: "{palette.as_str()}",
                            class: if ui.palette() == palette { "palette-chip active" } else { "palette-chip" },
                            style: "--swatch: {palette.swatch()};",
                            onclick: {
                                let services = services.clone();
                                move |_| apply_palette(&services, ui, palette)
                            },
                            "{palette.label()}"
                        }
                    }
                }
            }
        }
    }
}

async fn finish_status(services: AppServices, ui: UiState, status: plasma_telegram::TdlibStatus) {
    let ready = status.ready;
    apply_auth_status(ui, status);
    if ready {
        crate::refresh_identity(services, ui).await;
    }
}

fn normalize_phone(raw: &str) -> Option<String> {
    let mut compact = String::new();
    for character in raw.chars() {
        if character.is_ascii_digit() || (character == '+' && compact.is_empty()) {
            compact.push(character);
        }
    }

    let digits = compact.trim_start_matches('+');
    if digits.len() < 7 || digits.len() > 15 {
        return None;
    }

    if compact.starts_with('+') {
        Some(compact)
    } else {
        Some(format!("+{compact}"))
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_phone;

    #[test]
    fn phone_normalization_keeps_the_country_prefix() {
        assert_eq!(
            normalize_phone("+55 (11) 99999-9999").as_deref(),
            Some("+5511999999999")
        );
        assert_eq!(
            normalize_phone("5511999999999").as_deref(),
            Some("+5511999999999")
        );
    }

    #[test]
    fn phone_normalization_rejects_short_or_oversized_numbers() {
        assert!(normalize_phone("123").is_none());
        assert!(normalize_phone("1234567890123456").is_none());
    }
}
