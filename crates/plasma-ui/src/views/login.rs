//! Telegram login (TDLib) and welcome screen with live style/theme switching.

use crate::icons::{Icon, IconName};
use crate::state::{
    apply_auth_error, apply_auth_status, apply_color_mode, apply_style, push_toast, AppServices,
    ToastKind, UiState,
};
use crate::theme::{ColorMode, UiStyle};
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

    let state = ui.session.auth_state();
    let booting = ui.session.booting();
    let error = ui.session.auth_error();
    let busy_value = busy();
    let active_style = ui.shell.style();
    let active_mode = ui.shell.color_mode();

    let submit_phone = {
        let services = services.clone();
        move || {
            if busy() {
                return;
            }
            let Some(value) = normalize_phone(&phone()) else {
                form_error.set(Some(
                    "Informe o número com DDI, por exemplo +5511999999999.".to_string(),
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
                        apply_auth_error(ui, error.to_string());
                        form_error.set(Some(error.to_string()));
                    }
                }
                busy.set(false);
            });
        }
    };

    let submit_code = {
        let services = services.clone();
        move || {
            if busy() {
                return;
            }
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
                        apply_auth_error(ui, error.to_string());
                        form_error.set(Some(error.to_string()));
                    }
                }
                busy.set(false);
            });
        }
    };

    let submit_password = {
        let services = services.clone();
        move || {
            if busy() {
                return;
            }
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
                        apply_auth_error(ui, error.to_string());
                        form_error.set(Some(error.to_string()));
                    }
                }
                busy.set(false);
            });
        }
    };

    rsx! {
        div { class: "login-screen",
            div { class: "login-shell",
                aside { class: "login-hero",
                    div {
                        div { class: "brand-mark",
                            Icon { name: IconName::BrandTelegram, size: 26, class: "icon" }
                        }
                        h1 { "Plasma Hub" }
                        p { class: "muted",
                            "Suas conversas, fóruns, galerias sincronizadas do X e downloads \
                             de alta qualidade reunidos em uma única janela."
                        }
                    }

                    div { class: "login-appearance-box",
                        div { class: "login-appearance-head",
                            span { class: "hint", "Personalize o visual agora" }
                            button {
                                class: "btn small ghost",
                                onclick: {
                                    let services = services.clone();
                                    move |_| apply_color_mode(&services, ui, active_mode.toggle())
                                },
                                Icon {
                                    name: if active_mode == ColorMode::Dark { IconName::Sun } else { IconName::Moon },
                                    size: 14,
                                    class: "icon",
                                }
                                "{active_mode.toggle().label()}"
                            }
                        }
                        div { class: "login-style-strip",
                            for option in UiStyle::ALL {
                                button {
                                    key: "{option.as_str()}",
                                    class: if active_style == option { "scope-pill active" } else { "scope-pill" },
                                    onclick: {
                                        let services = services.clone();
                                        move |_| apply_style(&services, ui, option)
                                    },
                                    "{option.label()}"
                                }
                            }
                        }
                    }
                }

                section { class: "login-card",
                    if booting {
                        div { class: "login-status",
                            div { class: "spinner" }
                            strong { "Conectando ao Telegram" }
                            p { class: "hint", "Inicializando o TDLib e restaurando sua sessão local…" }
                        }
                    } else if state == "ready" {
                        div { class: "login-status",
                            strong { "Sessão pronta" }
                            p { class: "hint", "Sua conta do Telegram já está autenticada neste dispositivo." }
                        }
                        div { class: "login-actions",
                            button {
                                class: "btn primary",
                                onclick: move |_| {
                                    ui.session.preview_mode.set(false);
                                    ui.shell.logged_in.set(true);
                                },
                                "Entrar nas conversas"
                            }
                        }
                    } else if state == "wait_code" {
                        div { class: "login-status",
                            strong { "Código de verificação" }
                            p { class: "hint", "Digite o código enviado pelo aplicativo do Telegram ou SMS." }
                        }
                        input {
                            class: "login-input",
                            r#type: "text",
                            inputmode: "numeric",
                            autocomplete: "one-time-code",
                            placeholder: "12345",
                            maxlength: "16",
                            autofocus: true,
                            value: "{code}",
                            oninput: move |event| code.set(event.value()),
                            onkeydown: {
                                let mut submit = submit_code.clone();
                                move |event: KeyboardEvent| {
                                    if event.key() == Key::Enter {
                                        submit();
                                    }
                                }
                            },
                        }
                        div { class: "login-actions",
                            button {
                                class: "btn primary",
                                disabled: busy_value,
                                onclick: {
                                    let mut submit = submit_code;
                                    move |_| submit()
                                },
                                if busy_value { "Verificando…" } else { "Confirmar código" }
                            }
                        }
                    } else if state == "wait_password" {
                        div { class: "login-status",
                            strong { "Senha de duas etapas" }
                            p { class: "hint", "Sua conta possui verificação em duas etapas ativada." }
                        }
                        input {
                            class: "login-input",
                            r#type: "password",
                            autocomplete: "current-password",
                            placeholder: "Sua senha do Telegram",
                            autofocus: true,
                            value: "{password}",
                            oninput: move |event| password.set(event.value()),
                            onkeydown: {
                                let mut submit = submit_password.clone();
                                move |event: KeyboardEvent| {
                                    if event.key() == Key::Enter {
                                        submit();
                                    }
                                }
                            },
                        }
                        div { class: "login-actions",
                            button {
                                class: "btn primary",
                                disabled: busy_value,
                                onclick: {
                                    let mut submit = submit_password;
                                    move |_| submit()
                                },
                                if busy_value { "Entrando…" } else { "Desbloquear conta" }
                            }
                        }
                    } else if matches!(
                        state.as_str(),
                        "wait_other_device_confirmation"
                            | "wait_registration"
                            | "wait_email_address"
                            | "wait_email_code"
                    ) {
                        div { class: "login-status",
                            strong { "Confirmação adicional exigida" }
                            p { class: "hint",
                                "Este método de confirmação complementar ainda não está disponível \
                                 diretamente nesta tela."
                            }
                        }
                    } else if state == "error" || state == "closed" || state == "closing" {
                        div { class: "login-status",
                            strong { "Não foi possível conectar" }
                            p { class: "hint",
                                {error.clone().unwrap_or_else(|| "O cliente TDLib foi encerrado. Tente reconectar.".to_string())}
                            }
                        }
                        div { class: "login-actions",
                            button {
                                class: "btn primary",
                                disabled: busy_value,
                                onclick: {
                                    let services = services.clone();
                                    move |_| {
                                        if busy() {
                                            return;
                                        }
                                        let Some(credentials) = services.credentials.clone() else {
                                            form_error.set(Some(
                                                "Credenciais ausentes. Defina PLASMA_TELEGRAM_API_ID e PLASMA_TELEGRAM_API_HASH no .env."
                                                    .to_string(),
                                            ));
                                            return;
                                        };
                                        busy.set(true);
                                        form_error.set(None);
                                        let services = services.clone();
                                        spawn(async move {
                                            ui.session.auth_state.set("initializing".to_string());
                                            match services
                                                .telegram
                                                .init(credentials.api_id, credentials.api_hash)
                                                .await
                                            {
                                                Ok(status) => {
                                                    finish_status(services.clone(), ui, status).await
                                                }
                                                Err(error) => {
                                                    apply_auth_error(ui, error.to_string());
                                                    form_error.set(Some(error.to_string()));
                                                }
                                            }
                                            busy.set(false);
                                        });
                                    }
                                },
                                if busy_value { "Reconectando…" } else { "Tentar novamente" }
                            }
                        }
                    } else if state == "not_initialized" && !booting {
                        div { class: "login-status",
                            strong { "Credenciais do Telegram ausentes" }
                            p { class: "hint",
                                {error.clone().unwrap_or_else(|| {
                                    "Defina PLASMA_TELEGRAM_API_ID e PLASMA_TELEGRAM_API_HASH no .env e recompile."
                                        .to_string()
                                })}
                            }
                        }
                    } else {
                        div { class: "login-status",
                            strong { "Entrar com o Telegram" }
                            p { class: "hint",
                                "Informe seu telefone completo com código do país (ex.: +55 11 99999-9999)."
                            }
                        }
                        input {
                            class: "login-input",
                            r#type: "tel",
                            inputmode: "tel",
                            autocomplete: "tel",
                            placeholder: "+55 11 99999-9999",
                            autofocus: true,
                            value: "{phone}",
                            oninput: move |event| phone.set(event.value()),
                            onkeydown: {
                                let mut submit = submit_phone.clone();
                                move |event: KeyboardEvent| {
                                    if event.key() == Key::Enter {
                                        submit();
                                    }
                                }
                            },
                        }
                        div { class: "login-actions",
                            button {
                                class: "btn primary",
                                disabled: busy_value,
                                onclick: {
                                    let mut submit = submit_phone;
                                    move |_| submit()
                                },
                                if busy_value { "Enviando…" } else { "Receber código" }
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
                        p { class: "error-banner", "{message}" }
                    }

                    div { class: "login-divider" }
                    div { class: "login-actions",
                        button {
                            class: "btn soft",
                            onclick: {
                                let services = services.clone();
                                move |_| {
                                    ui.session.preview_mode.set(true);
                                    ui.shell.logged_in.set(true);
                                    match services.settings.set_bool(SettingKey::SKIP_LOGIN, true) {
                                        Ok(()) => push_toast(
                                            ui.shell.toasts,
                                            "Modo sem conta aberto. Downloads e X/Twitter já estão disponíveis.",
                                            ToastKind::Info,
                                        ),
                                        Err(error) => push_toast(
                                            ui.shell.toasts,
                                            format!("Aberto, mas não foi possível salvar a preferência: {error}"),
                                            ToastKind::Error,
                                        ),
                                    }
                                }
                            },
                            "Usar sem conta (Downloads, X & Prévia)"
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
