//! Palettes and density tokens shared by every Rust view.

/// One of the six shipped palettes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Palette {
    Abyssal,
    Ion,
    Ember,
    Bloom,
    Forest,
    Light,
}

impl Palette {
    pub const ALL: [Self; 6] = [
        Self::Abyssal,
        Self::Ion,
        Self::Ember,
        Self::Bloom,
        Self::Forest,
        Self::Light,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Abyssal => "abyssal",
            Self::Ion => "ion",
            Self::Ember => "ember",
            Self::Bloom => "bloom",
            Self::Forest => "forest",
            Self::Light => "light",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Abyssal => "Abyssal",
            Self::Ion => "Ion",
            Self::Ember => "Ember",
            Self::Bloom => "Bloom",
            Self::Forest => "Forest",
            Self::Light => "Light",
        }
    }

    /// Representative accent used by the palette picker.
    pub const fn swatch(self) -> &'static str {
        match self {
            Self::Abyssal => "#00daf3",
            Self::Ion => "#44d0e8",
            Self::Ember => "#eea451",
            Self::Bloom => "#ef76c2",
            Self::Forest => "#73d99d",
            Self::Light => "#148bb5",
        }
    }

    pub fn from_stored(value: Option<String>) -> Self {
        match value.as_deref() {
            Some("ion") => Self::Ion,
            Some("ember") => Self::Ember,
            Some("bloom") => Self::Bloom,
            Some("forest") => Self::Forest,
            Some("light") => Self::Light,
            _ => Self::Abyssal,
        }
    }
}

/// Row height/density presets.
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

    pub fn from_stored(value: Option<String>) -> Self {
        match value.as_deref() {
            Some("compact") => Self::Compact,
            Some("roomy") => Self::Roomy,
            _ => Self::Cozy,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_values_fall_back_to_defaults() {
        assert_eq!(Palette::from_stored(Some("forest".into())), Palette::Forest);
        assert_eq!(Palette::from_stored(Some("bogus".into())), Palette::Abyssal);
        assert_eq!(Palette::from_stored(None), Palette::Abyssal);

        assert_eq!(Density::from_stored(Some("roomy".into())), Density::Roomy);
        assert_eq!(Density::from_stored(Some("bogus".into())), Density::Cozy);
        assert_eq!(Density::from_stored(None), Density::Cozy);
    }

    #[test]
    fn every_palette_has_a_unique_key_and_swatch() {
        let keys: std::collections::HashSet<_> = Palette::ALL
            .iter()
            .map(|palette| palette.as_str())
            .collect();
        assert_eq!(keys.len(), Palette::ALL.len());
        assert!(Palette::ALL
            .iter()
            .all(|palette| !palette.swatch().is_empty()));
    }
}
