//! Shared UI state and services.

use crate::theme::{Density, Palette};
use dioxus::prelude::*;
use plasma_core::AppEnv;
use plasma_store::SettingsStore;
use std::sync::atomic::{AtomicU64, Ordering};

/// Runtime services provided by the composition root.
#[derive(Clone)]
pub struct AppServices {
    pub env: AppEnv,
    pub settings: SettingsStore,
}

impl AppServices {
    pub fn new(env: AppEnv, settings: SettingsStore) -> Self {
        Self { env, settings }
    }
}

/// Top-level app sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Telegram,
    Downloads,
    Twitter,
    Settings,
}

impl Tab {
    pub const ALL: [Self; 4] = [
        Self::Telegram,
        Self::Downloads,
        Self::Twitter,
        Self::Settings,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Telegram => "Telegram",
            Self::Downloads => "Downloads",
            Self::Twitter => "Twitter/X",
            Self::Settings => "Ajustes",
        }
    }

    pub const fn rail_label(self) -> &'static str {
        match self {
            Self::Telegram => "TG",
            Self::Downloads => "DL",
            Self::Twitter => "X",
            Self::Settings => "CF",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

impl ToastKind {
    pub const fn class(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Success => "success",
            Self::Error => "error",
        }
    }
}

static NEXT_TOAST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
}

impl Toast {
    pub fn new(message: impl Into<String>, kind: ToastKind) -> Self {
        Self {
            id: NEXT_TOAST_ID.fetch_add(1, Ordering::Relaxed),
            message: message.into(),
            kind,
        }
    }
}

/// Signal bundle shared through context.
#[derive(Clone, Copy)]
pub struct UiState {
    pub tab: Signal<Tab>,
    pub palette: Signal<Palette>,
    pub density: Signal<Density>,
    pub logged_in: Signal<bool>,
    pub show_spike: Signal<bool>,
    pub show_palette: Signal<bool>,
    pub toasts: Signal<Vec<Toast>>,
}

impl UiState {
    pub fn tab(self) -> Tab {
        (self.tab)()
    }

    pub fn palette(self) -> Palette {
        (self.palette)()
    }

    pub fn density(self) -> Density {
        (self.density)()
    }

    pub fn logged_in(self) -> bool {
        (self.logged_in)()
    }

    pub fn show_spike(self) -> bool {
        (self.show_spike)()
    }

    pub fn show_palette(self) -> bool {
        (self.show_palette)()
    }
}

pub fn push_toast(mut toasts: Signal<Vec<Toast>>, message: impl Into<String>, kind: ToastKind) {
    let mut next = toasts.read().clone();
    next.push(Toast::new(message, kind));
    if next.len() > 4 {
        next.remove(0);
    }
    toasts.set(next);
}

pub fn apply_palette(services: &AppServices, mut ui: UiState, palette: Palette) {
    ui.palette.set(palette);
    if let Err(error) = services.settings.set(
        plasma_store::SettingKey::APPEARANCE_PALETTE,
        palette.as_str(),
    ) {
        push_toast(
            ui.toasts,
            format!("Falha ao salvar a paleta: {error}"),
            ToastKind::Error,
        );
    }
}

pub fn apply_density(services: &AppServices, mut ui: UiState, density: Density) {
    ui.density.set(density);
    if let Err(error) = services.settings.set(
        plasma_store::SettingKey::APPEARANCE_DENSITY,
        density.as_str(),
    ) {
        push_toast(
            ui.toasts,
            format!("Falha ao salvar a densidade: {error}"),
            ToastKind::Error,
        );
    }
}
