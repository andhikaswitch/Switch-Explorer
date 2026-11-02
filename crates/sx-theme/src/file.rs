//! Theme files (`themes/*.toml`, THEMING.md §5): parsed into raw tables; values stay strings
//! until the resolver evaluates them.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::ThemeError;

/// Theme files larger than this are refused (THEMING.md §4).
pub const MAX_THEME_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    pub meta: ThemeMeta,
    pub palette: PaletteSection,
    #[serde(default)]
    pub material: Material,
    #[serde(default)]
    pub fonts: Fonts,
    #[serde(default)]
    pub numbers: BTreeMap<String, f32>,
    #[serde(default)]
    pub common: BTreeMap<String, String>,
    #[serde(default)]
    pub light: BTreeMap<String, String>,
    #[serde(default)]
    pub dark: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ThemeMeta {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub version: String,
    pub schema: u32,
    pub variants: Vec<String>,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct PaletteSection {
    pub accent: String,
    pub neutral: String,
    #[serde(default)]
    pub pin: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Material {
    /// `mica | mica_alt | acrylic | solid`
    pub backdrop: String,
    pub tint_strength: f32,
    pub noise: f32,
    /// Opaque base under translucent backgrounds when the material is solid (Windows 10,
    /// transparency off, battery saver). Defaults are Windows' own Mica fallback colours.
    pub solid_light: String,
    pub solid_dark: String,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            backdrop: "mica_alt".into(),
            tint_strength: 1.0,
            noise: 0.0,
            solid_light: "#F3F3F3".into(),
            solid_dark: "#202020".into(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Fonts {
    pub ui: Vec<String>,
    pub display: Vec<String>,
    pub mono: Vec<String>,
}

impl ThemeFile {
    pub fn parse(source: &str) -> Result<Self, ThemeError> {
        if source.len() > MAX_THEME_BYTES {
            return Err(ThemeError::TooLarge(source.len()));
        }
        toml::from_str(source).map_err(|err| ThemeError::Toml(err.to_string()))
    }
}
