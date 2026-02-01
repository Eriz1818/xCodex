use crate::config::Config;
use crate::config::types::ThemeMode;
use crate::config::types::Themes;
use serde::Deserialize;
use serde::Serialize;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ThemeError {
    #[error("failed to read theme file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid theme YAML in {path}: {source}")]
    InvalidYaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
    #[error("theme {name} missing required field {field}")]
    MissingField { name: String, field: &'static str },
    #[error("theme {name} has invalid color {value} for {field}")]
    InvalidColor {
        name: String,
        field: &'static str,
        value: String,
    },
    #[error("theme {name} references missing palette slot {slot}")]
    MissingPaletteSlot { name: String, slot: String },
    #[error("failed to serialize theme YAML: {source}")]
    SerializeYaml {
        #[source]
        source: serde_yaml::Error,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeVariant {
    Light,
    Dark,
}

impl ThemeVariant {
    pub fn from_background(background: ThemeColorResolved) -> Self {
        match background {
            ThemeColorResolved::Rgb(rgb) => {
                if rgb.is_light() {
                    Self::Light
                } else {
                    Self::Dark
                }
            }
            ThemeColorResolved::Inherit => Self::Dark,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeRgb(pub u8, pub u8, pub u8);

impl ThemeRgb {
    fn is_light(self) -> bool {
        let ThemeRgb(r, g, b) = self;
        // Relative luminance (rough) for sRGB.
        let luminance =
            (0.2126 * f64::from(r) + 0.7152 * f64::from(g) + 0.0722 * f64::from(b)) / 255.0;
        luminance > 0.6
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeColorResolved {
    Inherit,
    Rgb(ThemeRgb),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThemeColor(String);

impl ThemeColor {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn set(&mut self, value: impl Into<String>) {
        self.0 = value.into();
    }

    pub fn inherit() -> Self {
        Self("inherit".to_string())
    }

    fn parse_rgb_str(value: &str) -> Option<ThemeRgb> {
        let s = value.strip_prefix('#')?;
        if s.len() != 6 {
            return None;
        }
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(ThemeRgb(r, g, b))
    }

    fn as_palette_ref(&self) -> Option<&str> {
        self.0.strip_prefix("palette.")
    }

    pub fn resolve(&self, palette: &ThemePalette) -> Option<ThemeColorResolved> {
        if self.0 == "inherit" {
            return Some(ThemeColorResolved::Inherit);
        }
        if let Some(rgb) = Self::parse_rgb_str(&self.0) {
            return Some(ThemeColorResolved::Rgb(rgb));
        }
        let slot = self.as_palette_ref()?;
        palette.get(slot)
    }
}

impl fmt::Display for ThemeColor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemePalette {
    pub black: ThemeColor,
    pub red: ThemeColor,
    pub green: ThemeColor,
    pub yellow: ThemeColor,
    pub blue: ThemeColor,
    pub magenta: ThemeColor,
    pub cyan: ThemeColor,
    pub white: ThemeColor,

    pub bright_black: ThemeColor,
    pub bright_red: ThemeColor,
    pub bright_green: ThemeColor,
    pub bright_yellow: ThemeColor,
    pub bright_blue: ThemeColor,
    pub bright_magenta: ThemeColor,
    pub bright_cyan: ThemeColor,
    pub bright_white: ThemeColor,
}

impl ThemePalette {
    pub fn default_inherit() -> Self {
        let inherit = ThemeColor::inherit();
        Self {
            black: inherit.clone(),
            red: inherit.clone(),
            green: inherit.clone(),
            yellow: inherit.clone(),
            blue: inherit.clone(),
            magenta: inherit.clone(),
            cyan: inherit.clone(),
            white: inherit.clone(),
            bright_black: inherit.clone(),
            bright_red: inherit.clone(),
            bright_green: inherit.clone(),
            bright_yellow: inherit.clone(),
            bright_blue: inherit.clone(),
            bright_magenta: inherit.clone(),
            bright_cyan: inherit.clone(),
            bright_white: inherit,
        }
    }

    pub fn get(&self, slot: &str) -> Option<ThemeColorResolved> {
        let color = match slot {
            "black" => &self.black,
            "red" => &self.red,
            "green" => &self.green,
            "yellow" => &self.yellow,
            "blue" => &self.blue,
            "magenta" => &self.magenta,
            "cyan" => &self.cyan,
            "white" => &self.white,
            "bright_black" => &self.bright_black,
            "bright_red" => &self.bright_red,
            "bright_green" => &self.bright_green,
            "bright_yellow" => &self.bright_yellow,
            "bright_blue" => &self.bright_blue,
            "bright_magenta" => &self.bright_magenta,
            "bright_cyan" => &self.bright_cyan,
            "bright_white" => &self.bright_white,
            _ => return None,
        };
        color.resolve(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeRoles {
    pub fg: ThemeColor,
    pub bg: ThemeColor,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transcript_bg: Option<ThemeColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composer_bg: Option<ThemeColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_prompt_highlight_bg: Option<ThemeColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_bg: Option<ThemeColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_ramp_fg: Option<ThemeColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_ramp_highlight: Option<ThemeColor>,

    pub selection_fg: ThemeColor,
    pub selection_bg: ThemeColor,
    pub cursor_fg: ThemeColor,
    pub cursor_bg: ThemeColor,
    pub border: ThemeColor,

    pub accent: ThemeColor,
    pub brand: ThemeColor,
    #[serde(default = "default_command_role")]
    pub command: ThemeColor,
    pub success: ThemeColor,
    pub warning: ThemeColor,
    pub error: ThemeColor,

    pub diff_add_fg: ThemeColor,
    pub diff_add_bg: ThemeColor,
    pub diff_del_fg: ThemeColor,
    pub diff_del_bg: ThemeColor,
    pub diff_hunk_fg: ThemeColor,
    pub diff_hunk_bg: ThemeColor,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<ThemeColor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<ThemeColor>,

    // Syntax highlighting roles (fenced code blocks).
    //
    // These default to palette-based values so themes can override token colors independently of
    // broader UI roles like `roles.brand`/`roles.command`.
    #[serde(default = "default_code_keyword_role")]
    pub code_keyword: ThemeColor,
    #[serde(default = "default_code_operator_role")]
    pub code_operator: ThemeColor,
    #[serde(default = "default_code_comment_role")]
    pub code_comment: ThemeColor,
    #[serde(default = "default_code_string_role")]
    pub code_string: ThemeColor,
    #[serde(default = "default_code_number_role")]
    pub code_number: ThemeColor,
    #[serde(default = "default_code_type_role")]
    pub code_type: ThemeColor,
    #[serde(default = "default_code_function_role")]
    pub code_function: ThemeColor,
    #[serde(default = "default_code_constant_role")]
    pub code_constant: ThemeColor,
    #[serde(default = "default_code_macro_role")]
    pub code_macro: ThemeColor,
    #[serde(default = "default_code_punctuation_role")]
    pub code_punctuation: ThemeColor,
    #[serde(default = "default_code_variable_role")]
    pub code_variable: ThemeColor,
    #[serde(default = "default_code_property_role")]
    pub code_property: ThemeColor,
    #[serde(default = "default_code_attribute_role")]
    pub code_attribute: ThemeColor,
    #[serde(default = "default_code_module_role")]
    pub code_module: ThemeColor,
    #[serde(default = "default_code_label_role")]
    pub code_label: ThemeColor,
    #[serde(default = "default_code_tag_role")]
    pub code_tag: ThemeColor,
    #[serde(default = "default_code_embedded_role")]
    pub code_embedded: ThemeColor,
}

fn default_command_role() -> ThemeColor {
    ThemeColor("palette.magenta".to_string())
}

fn default_code_keyword_role() -> ThemeColor {
    ThemeColor("palette.magenta".to_string())
}

fn default_code_operator_role() -> ThemeColor {
    ThemeColor("palette.magenta".to_string())
}

fn default_code_comment_role() -> ThemeColor {
    ThemeColor("palette.bright_green".to_string())
}

fn default_code_string_role() -> ThemeColor {
    ThemeColor("palette.bright_green".to_string())
}

fn default_code_number_role() -> ThemeColor {
    ThemeColor("palette.blue".to_string())
}

fn default_code_type_role() -> ThemeColor {
    ThemeColor("palette.cyan".to_string())
}

fn default_code_function_role() -> ThemeColor {
    ThemeColor("palette.green".to_string())
}

fn default_code_constant_role() -> ThemeColor {
    ThemeColor("palette.cyan".to_string())
}

fn default_code_macro_role() -> ThemeColor {
    ThemeColor("palette.magenta".to_string())
}

fn default_code_punctuation_role() -> ThemeColor {
    ThemeColor::inherit()
}

fn default_code_variable_role() -> ThemeColor {
    ThemeColor::inherit()
}

fn default_code_property_role() -> ThemeColor {
    ThemeColor::inherit()
}

fn default_code_attribute_role() -> ThemeColor {
    ThemeColor("palette.yellow".to_string())
}

fn default_code_module_role() -> ThemeColor {
    ThemeColor("palette.cyan".to_string())
}

fn default_code_label_role() -> ThemeColor {
    ThemeColor("palette.yellow".to_string())
}

fn default_code_tag_role() -> ThemeColor {
    ThemeColor("palette.magenta".to_string())
}

fn default_code_embedded_role() -> ThemeColor {
    ThemeColor("palette.red".to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeDefinition {
    pub name: String,
    pub variant: ThemeVariant,
    pub palette: ThemePalette,
    pub roles: ThemeRoles,
}

impl ThemeDefinition {
    fn resolve_role_or_inherit(
        &self,
        field: &'static str,
        color: &ThemeColor,
    ) -> ThemeColorResolved {
        self.resolve_role(field, color)
            .unwrap_or(ThemeColorResolved::Inherit)
    }

    pub fn resolve_role(
        &self,
        field: &'static str,
        color: &ThemeColor,
    ) -> Result<ThemeColorResolved, ThemeError> {
        color.resolve(&self.palette).ok_or_else(|| {
            if let Some(slot) = color.as_palette_ref() {
                ThemeError::MissingPaletteSlot {
                    name: self.name.clone(),
                    slot: slot.to_string(),
                }
            } else {
                ThemeError::InvalidColor {
                    name: self.name.clone(),
                    field,
                    value: color.to_string(),
                }
            }
        })
    }

    pub fn resolve_dim(&self) -> ThemeColorResolved {
        let fg = self.roles.fg.resolve(&self.palette);
        let bg = self.roles.bg.resolve(&self.palette);
        match (fg, bg) {
            (
                Some(ThemeColorResolved::Rgb(ThemeRgb(fr, fg, fb))),
                Some(ThemeColorResolved::Rgb(ThemeRgb(br, bg, bb))),
            ) => {
                // Blend 60% fg with 40% bg for a dim-ish value.
                let r = ((fr as u16 * 6 + br as u16 * 4) / 10) as u8;
                let g = ((fg as u16 * 6 + bg as u16 * 4) / 10) as u8;
                let b = ((fb as u16 * 6 + bb as u16 * 4) / 10) as u8;
                ThemeColorResolved::Rgb(ThemeRgb(r, g, b))
            }
            _ => ThemeColorResolved::Inherit,
        }
    }

    fn lighten_bg(bg: ThemeColorResolved, percent: u16) -> ThemeColorResolved {
        let percent = percent.min(100);
        match bg {
            ThemeColorResolved::Rgb(ThemeRgb(br, bg, bb)) => {
                let r = br.saturating_add(((u16::from(255u8 - br) * percent) / 100) as u8);
                let g = bg.saturating_add(((u16::from(255u8 - bg) * percent) / 100) as u8);
                let b = bb.saturating_add(((u16::from(255u8 - bb) * percent) / 100) as u8);
                ThemeColorResolved::Rgb(ThemeRgb(r, g, b))
            }
            ThemeColorResolved::Inherit => ThemeColorResolved::Inherit,
        }
    }

    fn darken_bg(bg: ThemeColorResolved, percent: u16) -> ThemeColorResolved {
        let percent = percent.min(100);
        match bg {
            ThemeColorResolved::Rgb(ThemeRgb(br, bg, bb)) => {
                let r = br.saturating_sub(((u16::from(br) * percent) / 100) as u8);
                let g = bg.saturating_sub(((u16::from(bg) * percent) / 100) as u8);
                let b = bb.saturating_sub(((u16::from(bb) * percent) / 100) as u8);
                ThemeColorResolved::Rgb(ThemeRgb(r, g, b))
            }
            ThemeColorResolved::Inherit => ThemeColorResolved::Inherit,
        }
    }

    pub fn resolve_transcript_bg(&self) -> ThemeColorResolved {
        match self.roles.transcript_bg.as_ref() {
            Some(color) => self.resolve_role_or_inherit("roles.transcript_bg", color),
            None => self.resolve_role_or_inherit("roles.bg", &self.roles.bg),
        }
    }

    pub fn resolve_composer_bg(&self) -> ThemeColorResolved {
        match self.roles.composer_bg.as_ref() {
            Some(color) => self.resolve_role_or_inherit("roles.composer_bg", color),
            None => {
                let bg = self.resolve_transcript_bg();
                // Default composer background: derive from transcript background so the composer
                // reads as a distinct surface. On dark themes, lift; on light themes, sink.
                //
                // We derive the behavior from the *resolved* background when available so theme
                // edits that change bg colors automatically adjust the derived composer surface,
                // even if the theme's stored `variant` wasn't updated.
                let variant = match bg {
                    ThemeColorResolved::Rgb(_) => ThemeVariant::from_background(bg),
                    ThemeColorResolved::Inherit => self.variant,
                };
                match variant {
                    ThemeVariant::Light => Self::darken_bg(bg, 15),
                    ThemeVariant::Dark => Self::lighten_bg(bg, 15),
                }
            }
        }
    }

    pub fn resolve_user_prompt_highlight_bg(&self) -> ThemeColorResolved {
        match self.roles.user_prompt_highlight_bg.as_ref() {
            Some(color) => self.resolve_role_or_inherit("roles.user_prompt_highlight_bg", color),
            None => self.resolve_composer_bg(),
        }
    }

    pub fn resolve_status_bg(&self) -> ThemeColorResolved {
        match self.roles.status_bg.as_ref() {
            Some(color) => self.resolve_role_or_inherit("roles.status_bg", color),
            None => self.resolve_transcript_bg(),
        }
    }

    pub fn validate(&self) -> Result<(), ThemeError> {
        // Roles are “required” but can be set to inherit or palette refs. We validate they resolve.
        let roles = &self.roles;
        let required: [(&'static str, &ThemeColor); 36] = [
            ("roles.fg", &roles.fg),
            ("roles.bg", &roles.bg),
            ("roles.selection_fg", &roles.selection_fg),
            ("roles.selection_bg", &roles.selection_bg),
            ("roles.cursor_fg", &roles.cursor_fg),
            ("roles.cursor_bg", &roles.cursor_bg),
            ("roles.border", &roles.border),
            ("roles.accent", &roles.accent),
            ("roles.brand", &roles.brand),
            ("roles.command", &roles.command),
            ("roles.success", &roles.success),
            ("roles.warning", &roles.warning),
            ("roles.error", &roles.error),
            ("roles.diff_add_fg", &roles.diff_add_fg),
            ("roles.diff_add_bg", &roles.diff_add_bg),
            ("roles.diff_del_fg", &roles.diff_del_fg),
            ("roles.diff_del_bg", &roles.diff_del_bg),
            ("roles.diff_hunk_fg", &roles.diff_hunk_fg),
            ("roles.diff_hunk_bg", &roles.diff_hunk_bg),
            ("roles.code_keyword", &roles.code_keyword),
            ("roles.code_operator", &roles.code_operator),
            ("roles.code_comment", &roles.code_comment),
            ("roles.code_string", &roles.code_string),
            ("roles.code_number", &roles.code_number),
            ("roles.code_type", &roles.code_type),
            ("roles.code_function", &roles.code_function),
            ("roles.code_constant", &roles.code_constant),
            ("roles.code_macro", &roles.code_macro),
            ("roles.code_punctuation", &roles.code_punctuation),
            ("roles.code_variable", &roles.code_variable),
            ("roles.code_property", &roles.code_property),
            ("roles.code_attribute", &roles.code_attribute),
            ("roles.code_module", &roles.code_module),
            ("roles.code_label", &roles.code_label),
            ("roles.code_tag", &roles.code_tag),
            ("roles.code_embedded", &roles.code_embedded),
        ];

        for (field, color) in required {
            let _ = self.resolve_role(field, color)?;
        }

        let optional = [
            ("roles.transcript_bg", roles.transcript_bg.as_ref()),
            ("roles.composer_bg", roles.composer_bg.as_ref()),
            (
                "roles.user_prompt_highlight_bg",
                roles.user_prompt_highlight_bg.as_ref(),
            ),
            ("roles.status_bg", roles.status_bg.as_ref()),
            ("roles.status_ramp_fg", roles.status_ramp_fg.as_ref()),
            (
                "roles.status_ramp_highlight",
                roles.status_ramp_highlight.as_ref(),
            ),
            ("roles.badge", roles.badge.as_ref()),
            ("roles.link", roles.link.as_ref()),
        ];
        for (field, color) in optional {
            if let Some(color) = color {
                let _ = self.resolve_role(field, color)?;
            }
        }
        Ok(())
    }

    pub fn to_yaml(&self) -> Result<String, ThemeError> {
        serde_yaml::to_string(self).map_err(|source| ThemeError::SerializeYaml { source })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeLoadWarning {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeCatalog {
    default: ThemeDefinition,
    by_name: BTreeMap<String, ThemeDefinition>,
    built_in_names: BTreeSet<String>,
    user_theme_paths: BTreeMap<String, PathBuf>,
    load_warnings: Vec<ThemeLoadWarning>,
}

impl ThemeCatalog {
    fn built_in_bundle() -> Vec<ThemeDefinition> {
        let bytes = include_bytes!("../themes/mbadolato_builtins.json");
        let themes: Vec<ThemeDefinition> = match serde_json::from_slice(bytes) {
            Ok(themes) => themes,
            Err(_) => return Vec::new(),
        };

        themes
            .into_iter()
            .filter(|theme| theme.validate().is_ok())
            .collect()
    }

    pub fn built_in_default() -> ThemeDefinition {
        // Preserve the current “use terminal defaults” behavior until a user selects a theme.
        let inherit = ThemeColor::inherit();
        ThemeDefinition {
            name: "default".to_string(),
            variant: ThemeVariant::Dark,
            palette: ThemePalette::default_inherit(),
            roles: ThemeRoles {
                fg: inherit.clone(),
                bg: inherit.clone(),
                transcript_bg: None,
                composer_bg: None,
                user_prompt_highlight_bg: None,
                status_bg: None,
                status_ramp_fg: None,
                status_ramp_highlight: None,
                selection_fg: inherit.clone(),
                selection_bg: inherit.clone(),
                cursor_fg: inherit.clone(),
                cursor_bg: inherit.clone(),
                border: inherit.clone(),
                accent: ThemeColor("palette.blue".to_string()),
                brand: ThemeColor("palette.magenta".to_string()),
                command: ThemeColor("palette.magenta".to_string()),
                success: ThemeColor("palette.green".to_string()),
                warning: ThemeColor("palette.yellow".to_string()),
                error: ThemeColor("palette.red".to_string()),
                diff_add_fg: ThemeColor("palette.green".to_string()),
                diff_add_bg: inherit.clone(),
                diff_del_fg: ThemeColor("palette.red".to_string()),
                diff_del_bg: inherit.clone(),
                diff_hunk_fg: ThemeColor("palette.cyan".to_string()),
                diff_hunk_bg: inherit,
                badge: None,
                link: None,
                code_keyword: default_code_keyword_role(),
                code_operator: default_code_operator_role(),
                code_comment: default_code_comment_role(),
                code_string: default_code_string_role(),
                code_number: default_code_number_role(),
                code_type: default_code_type_role(),
                code_function: default_code_function_role(),
                code_constant: default_code_constant_role(),
                code_macro: default_code_macro_role(),
                code_punctuation: default_code_punctuation_role(),
                code_variable: default_code_variable_role(),
                code_property: default_code_property_role(),
                code_attribute: default_code_attribute_role(),
                code_module: default_code_module_role(),
                code_label: default_code_label_role(),
                code_tag: default_code_tag_role(),
                code_embedded: default_code_embedded_role(),
            },
        }
    }

    pub fn built_in_themes() -> Vec<ThemeDefinition> {
        let bundle = Self::built_in_bundle();
        if !bundle.is_empty() {
            return bundle;
        }

        let rgb = |value: &'static str| ThemeColor(value.to_string());
        let slot = |name: &'static str| ThemeColor(format!("palette.{name}"));

        vec![
            ThemeDefinition {
                name: "dracula".to_string(),
                variant: ThemeVariant::Dark,
                palette: ThemePalette {
                    black: rgb("#21222c"),
                    red: rgb("#ff5555"),
                    green: rgb("#50fa7b"),
                    yellow: rgb("#f1fa8c"),
                    blue: rgb("#bd93f9"),
                    magenta: rgb("#ff79c6"),
                    cyan: rgb("#8be9fd"),
                    white: rgb("#f8f8f2"),
                    bright_black: rgb("#6272a4"),
                    bright_red: rgb("#ff6e6e"),
                    bright_green: rgb("#69ff94"),
                    bright_yellow: rgb("#ffffa5"),
                    bright_blue: rgb("#d6acff"),
                    bright_magenta: rgb("#ff92df"),
                    bright_cyan: rgb("#a4ffff"),
                    bright_white: rgb("#ffffff"),
                },
                roles: ThemeRoles {
                    fg: rgb("#f8f8f2"),
                    bg: rgb("#282a36"),
                    transcript_bg: None,
                    composer_bg: None,
                    user_prompt_highlight_bg: None,
                    status_bg: None,
                    status_ramp_fg: None,
                    status_ramp_highlight: None,
                    selection_fg: rgb("#f8f8f2"),
                    selection_bg: rgb("#44475a"),
                    cursor_fg: rgb("#282a36"),
                    cursor_bg: rgb("#f8f8f2"),
                    border: slot("bright_black"),
                    accent: slot("blue"),
                    brand: slot("magenta"),
                    command: slot("magenta"),
                    success: slot("green"),
                    warning: slot("yellow"),
                    error: slot("red"),
                    diff_add_fg: slot("bright_green"),
                    diff_add_bg: rgb("#1f3a24"),
                    diff_del_fg: slot("bright_red"),
                    diff_del_bg: rgb("#3a1f1f"),
                    diff_hunk_fg: slot("cyan"),
                    diff_hunk_bg: slot("bright_black"),
                    badge: Some(slot("bright_blue")),
                    link: Some(slot("blue")),
                    code_keyword: default_code_keyword_role(),
                    code_operator: default_code_operator_role(),
                    code_comment: default_code_comment_role(),
                    code_string: default_code_string_role(),
                    code_number: default_code_number_role(),
                    code_type: default_code_type_role(),
                    code_function: default_code_function_role(),
                    code_constant: default_code_constant_role(),
                    code_macro: default_code_macro_role(),
                    code_punctuation: default_code_punctuation_role(),
                    code_variable: default_code_variable_role(),
                    code_property: default_code_property_role(),
                    code_attribute: default_code_attribute_role(),
                    code_module: default_code_module_role(),
                    code_label: default_code_label_role(),
                    code_tag: default_code_tag_role(),
                    code_embedded: default_code_embedded_role(),
                },
            },
            ThemeDefinition {
                name: "gruvbox-dark".to_string(),
                variant: ThemeVariant::Dark,
                palette: ThemePalette {
                    black: rgb("#282828"),
                    red: rgb("#cc241d"),
                    green: rgb("#98971a"),
                    yellow: rgb("#d79921"),
                    blue: rgb("#458588"),
                    magenta: rgb("#b16286"),
                    cyan: rgb("#689d6a"),
                    white: rgb("#a89984"),
                    bright_black: rgb("#928374"),
                    bright_red: rgb("#fb4934"),
                    bright_green: rgb("#b8bb26"),
                    bright_yellow: rgb("#fabd2f"),
                    bright_blue: rgb("#83a598"),
                    bright_magenta: rgb("#d3869b"),
                    bright_cyan: rgb("#8ec07c"),
                    bright_white: rgb("#ebdbb2"),
                },
                roles: ThemeRoles {
                    fg: rgb("#ebdbb2"),
                    bg: rgb("#282828"),
                    transcript_bg: None,
                    composer_bg: None,
                    user_prompt_highlight_bg: None,
                    status_bg: None,
                    status_ramp_fg: None,
                    status_ramp_highlight: None,
                    selection_fg: rgb("#ebdbb2"),
                    selection_bg: rgb("#3c3836"),
                    cursor_fg: rgb("#282828"),
                    cursor_bg: rgb("#ebdbb2"),
                    border: slot("bright_black"),
                    accent: slot("bright_blue"),
                    brand: slot("bright_magenta"),
                    command: slot("magenta"),
                    success: slot("bright_green"),
                    warning: slot("bright_yellow"),
                    error: slot("bright_red"),
                    diff_add_fg: slot("bright_green"),
                    diff_add_bg: rgb("#1f2d1f"),
                    diff_del_fg: slot("bright_red"),
                    diff_del_bg: rgb("#2d1f1f"),
                    diff_hunk_fg: slot("bright_cyan"),
                    diff_hunk_bg: rgb("#32302f"),
                    badge: Some(slot("bright_blue")),
                    link: Some(slot("blue")),
                    code_keyword: default_code_keyword_role(),
                    code_operator: default_code_operator_role(),
                    code_comment: default_code_comment_role(),
                    code_string: default_code_string_role(),
                    code_number: default_code_number_role(),
                    code_type: default_code_type_role(),
                    code_function: default_code_function_role(),
                    code_constant: default_code_constant_role(),
                    code_macro: default_code_macro_role(),
                    code_punctuation: default_code_punctuation_role(),
                    code_variable: default_code_variable_role(),
                    code_property: default_code_property_role(),
                    code_attribute: default_code_attribute_role(),
                    code_module: default_code_module_role(),
                    code_label: default_code_label_role(),
                    code_tag: default_code_tag_role(),
                    code_embedded: default_code_embedded_role(),
                },
            },
            ThemeDefinition {
                name: "nord".to_string(),
                variant: ThemeVariant::Dark,
                palette: ThemePalette {
                    black: rgb("#3b4252"),
                    red: rgb("#bf616a"),
                    green: rgb("#a3be8c"),
                    yellow: rgb("#ebcb8b"),
                    blue: rgb("#81a1c1"),
                    magenta: rgb("#b48ead"),
                    cyan: rgb("#88c0d0"),
                    white: rgb("#e5e9f0"),
                    bright_black: rgb("#4c566a"),
                    bright_red: rgb("#bf616a"),
                    bright_green: rgb("#a3be8c"),
                    bright_yellow: rgb("#ebcb8b"),
                    bright_blue: rgb("#81a1c1"),
                    bright_magenta: rgb("#b48ead"),
                    bright_cyan: rgb("#8fbcbb"),
                    bright_white: rgb("#eceff4"),
                },
                roles: ThemeRoles {
                    fg: rgb("#d8dee9"),
                    bg: rgb("#2e3440"),
                    transcript_bg: None,
                    composer_bg: None,
                    user_prompt_highlight_bg: None,
                    status_bg: None,
                    status_ramp_fg: None,
                    status_ramp_highlight: None,
                    selection_fg: rgb("#d8dee9"),
                    selection_bg: rgb("#434c5e"),
                    cursor_fg: rgb("#2e3440"),
                    cursor_bg: rgb("#d8dee9"),
                    border: slot("bright_black"),
                    accent: slot("bright_cyan"),
                    brand: slot("bright_blue"),
                    command: slot("magenta"),
                    success: slot("green"),
                    warning: slot("yellow"),
                    error: slot("red"),
                    diff_add_fg: slot("green"),
                    diff_add_bg: rgb("#273126"),
                    diff_del_fg: slot("red"),
                    diff_del_bg: rgb("#312526"),
                    diff_hunk_fg: slot("cyan"),
                    diff_hunk_bg: rgb("#3b4252"),
                    badge: Some(slot("bright_blue")),
                    link: Some(slot("cyan")),
                    code_keyword: default_code_keyword_role(),
                    code_operator: default_code_operator_role(),
                    code_comment: default_code_comment_role(),
                    code_string: default_code_string_role(),
                    code_number: default_code_number_role(),
                    code_type: default_code_type_role(),
                    code_function: default_code_function_role(),
                    code_constant: default_code_constant_role(),
                    code_macro: default_code_macro_role(),
                    code_punctuation: default_code_punctuation_role(),
                    code_variable: default_code_variable_role(),
                    code_property: default_code_property_role(),
                    code_attribute: default_code_attribute_role(),
                    code_module: default_code_module_role(),
                    code_label: default_code_label_role(),
                    code_tag: default_code_tag_role(),
                    code_embedded: default_code_embedded_role(),
                },
            },
            ThemeDefinition {
                name: "solarized-dark".to_string(),
                variant: ThemeVariant::Dark,
                palette: ThemePalette {
                    black: rgb("#073642"),
                    red: rgb("#dc322f"),
                    green: rgb("#859900"),
                    yellow: rgb("#b58900"),
                    blue: rgb("#268bd2"),
                    magenta: rgb("#d33682"),
                    cyan: rgb("#2aa198"),
                    white: rgb("#eee8d5"),
                    bright_black: rgb("#002b36"),
                    bright_red: rgb("#cb4b16"),
                    bright_green: rgb("#586e75"),
                    bright_yellow: rgb("#657b83"),
                    bright_blue: rgb("#839496"),
                    bright_magenta: rgb("#6c71c4"),
                    bright_cyan: rgb("#93a1a1"),
                    bright_white: rgb("#fdf6e3"),
                },
                roles: ThemeRoles {
                    fg: rgb("#839496"),
                    bg: rgb("#002b36"),
                    transcript_bg: None,
                    composer_bg: None,
                    user_prompt_highlight_bg: None,
                    status_bg: None,
                    status_ramp_fg: None,
                    status_ramp_highlight: None,
                    selection_fg: rgb("#93a1a1"),
                    selection_bg: rgb("#073642"),
                    cursor_fg: rgb("#002b36"),
                    cursor_bg: rgb("#839496"),
                    border: slot("bright_green"),
                    accent: slot("blue"),
                    brand: slot("magenta"),
                    command: slot("magenta"),
                    success: slot("green"),
                    warning: slot("yellow"),
                    error: slot("red"),
                    diff_add_fg: slot("green"),
                    diff_add_bg: rgb("#0b3b3b"),
                    diff_del_fg: slot("red"),
                    diff_del_bg: rgb("#3b0b18"),
                    diff_hunk_fg: slot("cyan"),
                    diff_hunk_bg: rgb("#073642"),
                    badge: Some(slot("bright_blue")),
                    link: Some(slot("blue")),
                    code_keyword: default_code_keyword_role(),
                    code_operator: default_code_operator_role(),
                    code_comment: default_code_comment_role(),
                    code_string: default_code_string_role(),
                    code_number: default_code_number_role(),
                    code_type: default_code_type_role(),
                    code_function: default_code_function_role(),
                    code_constant: default_code_constant_role(),
                    code_macro: default_code_macro_role(),
                    code_punctuation: default_code_punctuation_role(),
                    code_variable: default_code_variable_role(),
                    code_property: default_code_property_role(),
                    code_attribute: default_code_attribute_role(),
                    code_module: default_code_module_role(),
                    code_label: default_code_label_role(),
                    code_tag: default_code_tag_role(),
                    code_embedded: default_code_embedded_role(),
                },
            },
            ThemeDefinition {
                name: "solarized-light".to_string(),
                variant: ThemeVariant::Light,
                palette: ThemePalette {
                    black: rgb("#eee8d5"),
                    red: rgb("#dc322f"),
                    green: rgb("#859900"),
                    yellow: rgb("#b58900"),
                    blue: rgb("#268bd2"),
                    magenta: rgb("#d33682"),
                    cyan: rgb("#2aa198"),
                    white: rgb("#073642"),
                    bright_black: rgb("#fdf6e3"),
                    bright_red: rgb("#cb4b16"),
                    bright_green: rgb("#93a1a1"),
                    bright_yellow: rgb("#839496"),
                    bright_blue: rgb("#657b83"),
                    bright_magenta: rgb("#6c71c4"),
                    bright_cyan: rgb("#586e75"),
                    bright_white: rgb("#002b36"),
                },
                roles: ThemeRoles {
                    fg: rgb("#586e75"),
                    bg: rgb("#fdf6e3"),
                    transcript_bg: None,
                    composer_bg: None,
                    user_prompt_highlight_bg: None,
                    status_bg: None,
                    status_ramp_fg: None,
                    status_ramp_highlight: None,
                    selection_fg: rgb("#586e75"),
                    selection_bg: rgb("#eee8d5"),
                    cursor_fg: rgb("#fdf6e3"),
                    cursor_bg: rgb("#586e75"),
                    border: slot("bright_green"),
                    accent: slot("blue"),
                    brand: slot("magenta"),
                    command: slot("magenta"),
                    success: slot("green"),
                    warning: slot("yellow"),
                    error: slot("red"),
                    diff_add_fg: slot("green"),
                    diff_add_bg: rgb("#dff5d8"),
                    diff_del_fg: slot("red"),
                    diff_del_bg: rgb("#f6d6d6"),
                    diff_hunk_fg: slot("cyan"),
                    diff_hunk_bg: rgb("#eee8d5"),
                    badge: Some(slot("bright_blue")),
                    link: Some(slot("blue")),
                    code_keyword: default_code_keyword_role(),
                    code_operator: default_code_operator_role(),
                    code_comment: default_code_comment_role(),
                    code_string: default_code_string_role(),
                    code_number: default_code_number_role(),
                    code_type: default_code_type_role(),
                    code_function: default_code_function_role(),
                    code_constant: default_code_constant_role(),
                    code_macro: default_code_macro_role(),
                    code_punctuation: default_code_punctuation_role(),
                    code_variable: default_code_variable_role(),
                    code_property: default_code_property_role(),
                    code_attribute: default_code_attribute_role(),
                    code_module: default_code_module_role(),
                    code_label: default_code_label_role(),
                    code_tag: default_code_tag_role(),
                    code_embedded: default_code_embedded_role(),
                },
            },
        ]
        .into_iter()
        .inspect(|theme| {
            theme.validate().unwrap_or_else(|err| {
                panic!(
                    "invalid built-in theme {name}: {err}",
                    name = theme.name.as_str()
                )
            });
        })
        .collect()
    }

    pub fn example_theme_yaml(variant: ThemeVariant) -> String {
        let (name, fg, bg, selection_fg, selection_bg) = match variant {
            ThemeVariant::Light => ("example-light", "#1b1b1b", "#f7f7f7", "#1b1b1b", "#c7d7ff"),
            ThemeVariant::Dark => ("example-dark", "#d6d6d6", "#1e1e1e", "#1e1e1e", "#c7d7ff"),
        };

        let theme = ThemeDefinition {
            name: name.to_string(),
            variant,
            palette: ThemePalette {
                black: ThemeColor("#1b1b1b".to_string()),
                red: ThemeColor("#d14d41".to_string()),
                green: ThemeColor("#3faa54".to_string()),
                yellow: ThemeColor("#c58b2a".to_string()),
                blue: ThemeColor("#2f74d0".to_string()),
                magenta: ThemeColor("#a24bc3".to_string()),
                cyan: ThemeColor("#2aa7a7".to_string()),
                white: ThemeColor("#cfcfcf".to_string()),
                bright_black: ThemeColor("#4a4a4a".to_string()),
                bright_red: ThemeColor("#e06c75".to_string()),
                bright_green: ThemeColor("#7ed07e".to_string()),
                bright_yellow: ThemeColor("#e5c07b".to_string()),
                bright_blue: ThemeColor("#61afef".to_string()),
                bright_magenta: ThemeColor("#c678dd".to_string()),
                bright_cyan: ThemeColor("#56b6c2".to_string()),
                bright_white: ThemeColor("#ffffff".to_string()),
            },
            roles: ThemeRoles {
                fg: ThemeColor(fg.to_string()),
                bg: ThemeColor(bg.to_string()),
                transcript_bg: None,
                composer_bg: None,
                user_prompt_highlight_bg: None,
                status_bg: None,
                status_ramp_fg: None,
                status_ramp_highlight: None,
                selection_fg: ThemeColor(selection_fg.to_string()),
                selection_bg: ThemeColor(selection_bg.to_string()),
                cursor_fg: ThemeColor(bg.to_string()),
                cursor_bg: ThemeColor(fg.to_string()),
                border: ThemeColor("palette.bright_black".to_string()),
                accent: ThemeColor("palette.blue".to_string()),
                brand: ThemeColor("palette.magenta".to_string()),
                command: ThemeColor("palette.magenta".to_string()),
                success: ThemeColor("palette.green".to_string()),
                warning: ThemeColor("palette.yellow".to_string()),
                error: ThemeColor("palette.red".to_string()),
                diff_add_fg: ThemeColor("palette.bright_green".to_string()),
                diff_add_bg: ThemeColor("#1f3a24".to_string()),
                diff_del_fg: ThemeColor("palette.bright_red".to_string()),
                diff_del_bg: ThemeColor("#3a1f1f".to_string()),
                diff_hunk_fg: ThemeColor("palette.cyan".to_string()),
                diff_hunk_bg: ThemeColor("palette.bright_black".to_string()),
                badge: Some(ThemeColor("palette.bright_blue".to_string())),
                link: Some(ThemeColor("palette.blue".to_string())),
                code_keyword: default_code_keyword_role(),
                code_operator: default_code_operator_role(),
                code_comment: default_code_comment_role(),
                code_string: default_code_string_role(),
                code_number: default_code_number_role(),
                code_type: default_code_type_role(),
                code_function: default_code_function_role(),
                code_constant: default_code_constant_role(),
                code_macro: default_code_macro_role(),
                code_punctuation: default_code_punctuation_role(),
                code_variable: default_code_variable_role(),
                code_property: default_code_property_role(),
                code_attribute: default_code_attribute_role(),
                code_module: default_code_module_role(),
                code_label: default_code_label_role(),
                code_tag: default_code_tag_role(),
                code_embedded: default_code_embedded_role(),
            },
        };

        // The example should always be valid so users can copy/paste without
        // hitting validation errors immediately.
        let _ = theme.validate();

        serde_yaml::to_string(&theme).unwrap_or_default()
    }

    pub fn load(config: &Config) -> Result<Self, ThemeError> {
        let mut by_name = BTreeMap::new();
        let mut built_in_names = BTreeSet::new();
        let mut user_theme_paths = BTreeMap::new();
        let mut load_warnings = Vec::new();
        let default = Self::built_in_default();
        built_in_names.insert(default.name.clone());
        by_name.insert(default.name.clone(), default.clone());
        for theme in Self::built_in_themes() {
            built_in_names.insert(theme.name.clone());
            by_name.insert(theme.name.clone(), theme);
        }

        let themes_cfg = config.xcodex.themes.clone();
        let dir = themes_dir(&config.codex_home, &themes_cfg);
        if dir.exists() {
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(source) => {
                    load_warnings.push(ThemeLoadWarning {
                        path: dir,
                        message: format!("Failed to read themes directory: {source}"),
                    });
                    return Ok(Self {
                        default,
                        by_name,
                        built_in_names,
                        user_theme_paths,
                        load_warnings,
                    });
                }
            };

            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(source) => {
                        load_warnings.push(ThemeLoadWarning {
                            path: dir.clone(),
                            message: format!("Failed to read themes directory entry: {source}"),
                        });
                        continue;
                    }
                };
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("yml")
                    && path.extension().and_then(|s| s.to_str()) != Some("yaml")
                {
                    continue;
                }

                let src = match fs::read_to_string(&path) {
                    Ok(src) => src,
                    Err(source) => {
                        load_warnings.push(ThemeLoadWarning {
                            path: path.clone(),
                            message: format!("Failed to read: {source}"),
                        });
                        continue;
                    }
                };
                let theme: ThemeDefinition = match serde_yaml::from_str(&src) {
                    Ok(theme) => theme,
                    Err(source) => {
                        load_warnings.push(ThemeLoadWarning {
                            path: path.clone(),
                            message: format!("Invalid YAML: {source}"),
                        });
                        continue;
                    }
                };
                if let Err(err) = theme.validate() {
                    load_warnings.push(ThemeLoadWarning {
                        path: path.clone(),
                        message: format!("Invalid theme: {err}"),
                    });
                    continue;
                }
                let name = theme.name.clone();
                by_name.insert(name.clone(), theme);
                user_theme_paths.insert(name, path);
            }
        }

        Ok(Self {
            default,
            by_name,
            built_in_names,
            user_theme_paths,
            load_warnings,
        })
    }

    pub fn list_names(&self) -> impl Iterator<Item = (&str, ThemeVariant)> {
        self.by_name
            .iter()
            .map(|(name, theme)| (name.as_str(), theme.variant))
    }

    pub fn load_warnings(&self) -> &[ThemeLoadWarning] {
        &self.load_warnings
    }

    pub fn is_built_in_name(&self, name: &str) -> bool {
        self.built_in_names.contains(name)
    }

    pub fn user_theme_path(&self, name: &str) -> Option<&Path> {
        self.user_theme_paths.get(name).map(PathBuf::as_path)
    }

    pub fn get(&self, name: &str) -> Option<&ThemeDefinition> {
        self.by_name.get(name)
    }

    pub fn resolve_active<'a>(
        &'a self,
        cfg: &Themes,
        auto_variant: Option<ThemeVariant>,
        terminal_background_is_light: bool,
    ) -> &'a ThemeDefinition {
        let variant = match cfg.theme_mode {
            ThemeMode::Light => ThemeVariant::Light,
            ThemeMode::Dark => ThemeVariant::Dark,
            ThemeMode::Auto => auto_variant.unwrap_or({
                if terminal_background_is_light {
                    ThemeVariant::Light
                } else {
                    ThemeVariant::Dark
                }
            }),
        };
        let selected = match variant {
            ThemeVariant::Light => cfg.light.as_deref(),
            ThemeVariant::Dark => cfg.dark.as_deref(),
        }
        .unwrap_or("default");

        self.get(selected).unwrap_or(&self.default)
    }
}

pub fn themes_dir(codex_home: &Path, cfg: &Themes) -> PathBuf {
    match cfg.dir.as_ref() {
        Some(dir) => dir.as_path().to_path_buf(),
        None => codex_home.join("themes"),
    }
}

#[derive(Debug, Deserialize)]
struct UpstreamThemeYaml {
    name: String,
    color_01: String,
    color_02: String,
    color_03: String,
    color_04: String,
    color_05: String,
    color_06: String,
    color_07: String,
    color_08: String,
    color_09: String,
    color_10: String,
    color_11: String,
    color_12: String,
    color_13: String,
    color_14: String,
    color_15: String,
    color_16: String,
    foreground: String,
    background: String,
    cursor: String,
    cursor_text: String,
    selection: String,
    selection_text: String,
    #[serde(default)]
    badge: Option<String>,
    #[serde(default)]
    link: Option<String>,
}

pub fn convert_upstream_yaml(src: &str) -> Result<ThemeDefinition, ThemeError> {
    let upstream: UpstreamThemeYaml =
        serde_yaml::from_str(src).map_err(|e| ThemeError::InvalidYaml {
            path: PathBuf::from("<upstream>"),
            source: e,
        })?;

    let palette = ThemePalette {
        black: ThemeColor(upstream.color_01),
        red: ThemeColor(upstream.color_02),
        green: ThemeColor(upstream.color_03),
        yellow: ThemeColor(upstream.color_04),
        blue: ThemeColor(upstream.color_05),
        magenta: ThemeColor(upstream.color_06),
        cyan: ThemeColor(upstream.color_07),
        white: ThemeColor(upstream.color_08),
        bright_black: ThemeColor(upstream.color_09),
        bright_red: ThemeColor(upstream.color_10),
        bright_green: ThemeColor(upstream.color_11),
        bright_yellow: ThemeColor(upstream.color_12),
        bright_blue: ThemeColor(upstream.color_13),
        bright_magenta: ThemeColor(upstream.color_14),
        bright_cyan: ThemeColor(upstream.color_15),
        bright_white: ThemeColor(upstream.color_16),
    };

    let roles = ThemeRoles {
        fg: ThemeColor(upstream.foreground),
        bg: ThemeColor(upstream.background),
        transcript_bg: None,
        composer_bg: None,
        user_prompt_highlight_bg: None,
        status_bg: None,
        status_ramp_fg: None,
        status_ramp_highlight: None,
        selection_fg: ThemeColor(upstream.selection_text),
        selection_bg: ThemeColor(upstream.selection),
        cursor_fg: ThemeColor(upstream.cursor_text),
        cursor_bg: ThemeColor(upstream.cursor),
        border: ThemeColor("palette.bright_black".to_string()),
        accent: upstream
            .link
            .as_ref()
            .map(|s| ThemeColor(s.clone()))
            .unwrap_or_else(|| ThemeColor("palette.blue".to_string())),
        brand: ThemeColor("palette.magenta".to_string()),
        command: ThemeColor("palette.magenta".to_string()),
        success: ThemeColor("palette.green".to_string()),
        warning: ThemeColor("palette.yellow".to_string()),
        error: ThemeColor("palette.red".to_string()),
        diff_add_fg: ThemeColor("palette.green".to_string()),
        diff_add_bg: ThemeColor::inherit(),
        diff_del_fg: ThemeColor("palette.red".to_string()),
        diff_del_bg: ThemeColor::inherit(),
        diff_hunk_fg: ThemeColor("palette.cyan".to_string()),
        diff_hunk_bg: ThemeColor::inherit(),
        badge: upstream.badge.map(ThemeColor),
        link: upstream.link.map(ThemeColor),
        code_keyword: default_code_keyword_role(),
        code_operator: default_code_operator_role(),
        code_comment: default_code_comment_role(),
        code_string: default_code_string_role(),
        code_number: default_code_number_role(),
        code_type: default_code_type_role(),
        code_function: default_code_function_role(),
        code_constant: default_code_constant_role(),
        code_macro: default_code_macro_role(),
        code_punctuation: default_code_punctuation_role(),
        code_variable: default_code_variable_role(),
        code_property: default_code_property_role(),
        code_attribute: default_code_attribute_role(),
        code_module: default_code_module_role(),
        code_label: default_code_label_role(),
        code_tag: default_code_tag_role(),
        code_embedded: default_code_embedded_role(),
    };

    let background_resolved =
        roles
            .bg
            .resolve(&palette)
            .ok_or_else(|| ThemeError::InvalidColor {
                name: upstream.name.clone(),
                field: "roles.bg",
                value: roles.bg.to_string(),
            })?;

    let variant = ThemeVariant::from_background(background_resolved);

    let theme = ThemeDefinition {
        name: upstream.name,
        variant,
        palette,
        roles,
    };
    theme.validate()?;
    Ok(theme)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn resolve_composer_bg_derives_from_transcript_by_variant() {
        let mut theme = convert_upstream_yaml(
            r##"
name: Test Theme
color_01: "#000001"
color_02: "#000002"
color_03: "#000003"
color_04: "#000004"
color_05: "#000005"
color_06: "#000006"
color_07: "#000007"
color_08: "#000008"
color_09: "#000009"
color_10: "#00000a"
color_11: "#00000b"
color_12: "#00000c"
color_13: "#00000d"
color_14: "#00000e"
color_15: "#00000f"
color_16: "#000010"
foreground: "#111111"
background: "#f0f0f0"
cursor: "#222222"
cursor_text: "#333333"
selection: "#444444"
selection_text: "#555555"
"##,
        )
        .expect("conversion should succeed");

        assert_eq!(theme.variant, ThemeVariant::Light);
        assert_eq!(
            theme.resolve_transcript_bg(),
            ThemeColorResolved::Rgb(ThemeRgb(0xf0, 0xf0, 0xf0))
        );
        assert_eq!(
            theme.resolve_composer_bg(),
            ThemeColorResolved::Rgb(ThemeRgb(0xcc, 0xcc, 0xcc))
        );

        theme.variant = ThemeVariant::Dark;
        assert_eq!(
            theme.resolve_composer_bg(),
            ThemeColorResolved::Rgb(ThemeRgb(0xcc, 0xcc, 0xcc))
        );

        theme.roles.bg.set("#1e1e1e");
        assert_eq!(
            theme.resolve_composer_bg(),
            ThemeColorResolved::Rgb(ThemeRgb(0x3f, 0x3f, 0x3f))
        );
    }

    #[test]
    fn built_in_theme_bundle_loads_and_validates() {
        let themes = ThemeCatalog::built_in_bundle();
        assert_eq!(themes.len(), 453);
        assert!(themes.iter().any(|theme| theme.name == "dracula"));
        assert!(themes.iter().all(|theme| theme.validate().is_ok()));
    }

    #[test]
    fn convert_upstream_yaml_maps_core_fields_and_palette() {
        let src = r##"
name: Test Theme
color_01: "#000001"
color_02: "#000002"
color_03: "#000003"
color_04: "#000004"
color_05: "#000005"
color_06: "#000006"
color_07: "#000007"
color_08: "#000008"
color_09: "#000009"
color_10: "#00000a"
color_11: "#00000b"
color_12: "#00000c"
color_13: "#00000d"
color_14: "#00000e"
color_15: "#00000f"
color_16: "#000010"
foreground: "#111111"
background: "#ffffff"
cursor: "#222222"
cursor_text: "#333333"
selection: "#444444"
selection_text: "#555555"
"##;

        let theme = convert_upstream_yaml(src).expect("conversion should succeed");
        assert_eq!(theme.name, "Test Theme");
        assert_eq!(theme.variant, ThemeVariant::Light);
        assert_eq!(theme.palette.black.to_string(), "#000001");
        assert_eq!(theme.palette.bright_white.to_string(), "#000010");
        assert_eq!(theme.roles.fg.to_string(), "#111111");
        assert_eq!(theme.roles.bg.to_string(), "#ffffff");
        assert_eq!(theme.roles.selection_bg.to_string(), "#444444");
        assert_eq!(theme.roles.selection_fg.to_string(), "#555555");
        assert_eq!(theme.roles.cursor_bg.to_string(), "#222222");
        assert_eq!(theme.roles.cursor_fg.to_string(), "#333333");
        assert_eq!(theme.roles.border.to_string(), "palette.bright_black");
    }
}
