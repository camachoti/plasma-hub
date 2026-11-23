//! Multi-style messenger design system tokens (`UiStyle`, `ColorMode`, `Density`).
//!
//! All four visual styles share a single semantic DOM and switch purely via
//! `data-style` and `data-mode` attributes on `.plasma-root`, keeping runtime
//! overhead at zero during scrolling and live updates.

use plasma_store::{SettingKey, SettingsStore};

pub const STYLE_SETTING_KEY: SettingKey = SettingKey::from_static("plasma_ui_style");
pub const COLOR_MODE_SETTING_KEY: SettingKey = SettingKey::from_static("plasma_color_mode");

/// Four selectable messenger visual identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiStyle {
    /// Classic asymmetric bubbles, crisp continuous panes, sapphire blue accent.
    Native,
    /// Floating rounded island panes, soft tonal surfaces, indigo/lavender accent.
    Tonal,
    /// Wide stream layout for large groups, forums and media galleries.
    Community,
    /// Warm stone/ivory messenger surfaces with serene terracotta accent.
    Warm,
}

impl UiStyle {
    pub const ALL: [Self; 4] = [Self::Native, Self::Tonal, Self::Community, Self::Warm];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Tonal => "tonal",
            Self::Community => "community",
            Self::Warm => "warm",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Native => "Nativo Fluido",
            Self::Tonal => "Ilhas Tonais",
            Self::Community => "Hub de Comunidades",
            Self::Warm => "Editorial Quente",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Native => "Bolhas assimétricas clássicas, painéis contínuos e azul safira.",
            Self::Tonal => "Colunas flutuantes arredondadas, pílulas macias e tons índigo.",
            Self::Community => "Fluxo amplo com avatares sempre visíveis e grades de mídia largas.",
            Self::Warm => "Bolhas acolhedoras em tons de pedra, marfim e terracota suave.",
        }
    }

    /// Representative accent color used in style cards and swatches.
    pub const fn swatch(self) -> &'static str {
        match self {
            Self::Native => "#2aabee",
            Self::Tonal => "#7c8cff",
            Self::Community => "#5865f2",
            Self::Warm => "#e07a5f",
        }
    }

    pub fn from_stored(value: Option<String>) -> Self {
        match value.as_deref() {
            Some("tonal") => Self::Tonal,
            Some("community") => Self::Community,
            Some("warm") => Self::Warm,
            _ => Self::Native,
        }
    }

    pub fn load(settings: &SettingsStore) -> Self {
        Self::from_stored(settings.get(STYLE_SETTING_KEY).ok().flatten())
    }
}

/// Light or dark surface mode, independent of [`UiStyle`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Dark,
    Light,
}

impl ColorMode {
    pub const ALL: [Self; 2] = [Self::Dark, Self::Light];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Dark => "Escuro",
            Self::Light => "Claro",
        }
    }

    pub const fn toggle(self) -> Self {
        match self {
            Self::Dark => Self::Light,
            Self::Light => Self::Dark,
        }
    }

    pub fn from_stored(mode: Option<String>, legacy_palette: Option<String>) -> Self {
        match mode.as_deref() {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            _ => match legacy_palette.as_deref() {
                Some("light") => Self::Light,
                _ => Self::Dark,
            },
        }
    }

    pub fn load(settings: &SettingsStore) -> Self {
        let mode = settings.get(COLOR_MODE_SETTING_KEY).ok().flatten();
        let legacy = settings.get(SettingKey::APPEARANCE_PALETTE).ok().flatten();
        Self::from_stored(mode, legacy)
    }
}

/// Row height and spacing presets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Density {
    Compact,
    Cozy,
    Roomy,
}

impl Density {
    pub const ALL: [Self; 3] = [Self::Compact, Self::Cozy, Self::Roomy];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Cozy => "cozy",
            Self::Roomy => "roomy",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compacta",
            Self::Cozy => "Confortável",
            Self::Roomy => "Espaçosa",
        }
    }

    /// Chat-list row height in px — kept in sync with `--row-h` in `tokens.css`.
    pub const fn row_height(self) -> f64 {
        match self {
            Self::Compact => 54.0,
            Self::Cozy => 62.0,
            Self::Roomy => 70.0,
        }
    }

    pub fn from_stored(value: Option<String>) -> Self {
        match value.as_deref() {
            Some("compact") => Self::Compact,
            Some("roomy") => Self::Roomy,
            _ => Self::Cozy,
        }
    }
}

const AVATAR_GRADIENTS: [&str; 8] = [
    "linear-gradient(135deg, var(--avatar-1-from), var(--avatar-1-to))",
    "linear-gradient(135deg, var(--avatar-2-from), var(--avatar-2-to))",
    "linear-gradient(135deg, var(--avatar-3-from), var(--avatar-3-to))",
    "linear-gradient(135deg, var(--avatar-4-from), var(--avatar-4-to))",
    "linear-gradient(135deg, var(--avatar-5-from), var(--avatar-5-to))",
    "linear-gradient(135deg, var(--avatar-6-from), var(--avatar-6-to))",
    "linear-gradient(135deg, var(--avatar-7-from), var(--avatar-7-to))",
    "linear-gradient(135deg, var(--avatar-8-from), var(--avatar-8-to))",
];

const SENDER_HUES: [&str; 8] = [
    "var(--sender-1)",
    "var(--sender-2)",
    "var(--sender-3)",
    "var(--sender-4)",
    "var(--sender-5)",
    "var(--sender-6)",
    "var(--sender-7)",
    "var(--sender-8)",
];

fn hash_seed(seed: &str) -> usize {
    let mut hash: u64 = 5381;
    for byte in seed.bytes() {
        hash = ((hash << 5).wrapping_add(hash)) ^ u64::from(byte);
    }
    (hash as usize) % AVATAR_GRADIENTS.len()
}

/// Deterministic Telegram-like gradient for placeholder avatars.
pub fn avatar_gradient(seed: &str) -> &'static str {
    AVATAR_GRADIENTS[hash_seed(seed)]
}

/// Deterministic sender name color variable for group conversations.
pub fn sender_color(seed: &str) -> &'static str {
    SENDER_HUES[hash_seed(seed)]
}

/// Up to two uppercase initials for circular avatar placeholders.
pub fn avatar_initials(title: &str) -> String {
    let mut words = title
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.chars().find(|ch| ch.is_alphanumeric()));
    match (words.next(), words.next()) {
        (Some(first), Some(second)) => format!("{}{}", first, second).to_uppercase(),
        (Some(first), None) => first.to_uppercase().to_string(),
        _ => title
            .chars()
            .find(|ch| !ch.is_whitespace())
            .unwrap_or('?')
            .to_uppercase()
            .to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_values_fall_back_to_defaults() {
        assert_eq!(UiStyle::from_stored(Some("tonal".into())), UiStyle::Tonal);
        assert_eq!(
            UiStyle::from_stored(Some("community".into())),
            UiStyle::Community
        );
        assert_eq!(UiStyle::from_stored(Some("warm".into())), UiStyle::Warm);
        assert_eq!(UiStyle::from_stored(Some("bogus".into())), UiStyle::Native);
        assert_eq!(UiStyle::from_stored(None), UiStyle::Native);

        assert_eq!(
            ColorMode::from_stored(None, Some("light".into())),
            ColorMode::Light
        );
        assert_eq!(
            ColorMode::from_stored(None, Some("abyssal".into())),
            ColorMode::Dark
        );
        assert_eq!(
            ColorMode::from_stored(Some("light".into()), Some("abyssal".into())),
            ColorMode::Light
        );

        assert_eq!(Density::from_stored(Some("roomy".into())), Density::Roomy);
        assert_eq!(Density::from_stored(Some("bogus".into())), Density::Cozy);
        assert_eq!(Density::from_stored(None), Density::Cozy);
    }

    #[test]
    fn every_style_has_a_unique_key_and_swatch() {
        let keys: std::collections::HashSet<_> =
            UiStyle::ALL.iter().map(|style| style.as_str()).collect();
        assert_eq!(keys.len(), UiStyle::ALL.len());
        assert!(UiStyle::ALL.iter().all(|style| !style.swatch().is_empty()));
    }

    #[test]
    fn initials_extract_clean_monograms() {
        assert_eq!(avatar_initials("Design Archive"), "DA");
        assert_eq!(avatar_initials("@archillect"), "A");
        assert_eq!(avatar_initials("Plasma"), "P");
    }
}
