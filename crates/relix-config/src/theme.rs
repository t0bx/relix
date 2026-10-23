/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::color::ColorSpec;
use crate::error::ConfigError;
use crate::files::read_toml;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

macro_rules! theme_colors {
    ($($field:ident),* $(,)?) => {
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        pub struct ThemePalette<T> {
            $(pub $field: T,)*
        }

        pub type ThemeColors = ThemePalette<ColorSpec>;

        impl<T> ThemePalette<T> {
            pub const FIELDS: &'static [&'static str] = &[$(stringify!($field)),*];

            pub fn map<U>(&self, convert: impl Fn(&T) -> U) -> ThemePalette<U> {
                ThemePalette {
                    $($field: convert(&self.$field),)*
                }
            }

            pub fn set(&mut self, name: &str, value: T) -> bool {
                match name {
                    $(stringify!($field) => {
                        self.$field = value;
                        true
                    })*
                    _ => false,
                }
            }
        }
    };
}

theme_colors!(
    background,
    surface,
    foreground,
    muted,
    border,
    border_focused,
    accent,
    accent_secondary,
    selection_bg,
    selection_fg,
    cursor_line,
    status_bg,
    status_fg,
    error,
    warning,
    success,
    info,
    syntax_keyword,
    syntax_string,
    syntax_number,
    syntax_comment,
    syntax_identifier,
    syntax_operator,
    syntax_function,
    syntax_type,
    grid_header_bg,
    grid_header_fg,
    grid_row_alt_bg,
    grid_null,
    grid_modified_bg,
    grid_inserted_bg,
    grid_deleted_fg,
    grid_cursor_bg,
    env_production,
    env_staging,
    env_testing,
    env_development,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BorderKind {
    Plain,
    #[default]
    Rounded,
    Double,
    Thick,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub borders: BorderKind,
    pub colors: ThemeColors,
}

impl Theme {
    pub fn apply_overrides(&mut self, overrides: &BTreeMap<String, ColorSpec>) -> Vec<String> {
        overrides
            .iter()
            .filter(|(name, color)| !self.colors.set(name, **color))
            .map(|(name, _)| name.clone())
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct ThemeFile {
    name: String,
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    borders: Option<BorderKind>,
    #[serde(default)]
    colors: BTreeMap<String, ColorSpec>,
}

#[derive(Debug, Clone)]
pub struct ThemeCatalog {
    themes: Vec<Theme>,
    pub warnings: Vec<String>,
}

impl ThemeCatalog {
    pub fn built_in() -> Self {
        Self {
            themes: vec![dark(), light(), high_contrast()],
            warnings: Vec::new(),
        }
    }

    /// Loads `*.toml` theme files on top of the built-in themes. Invalid files are
    /// skipped and reported through `warnings` instead of failing startup.
    pub fn load(themes_dir: &Path) -> Self {
        let mut catalog = Self::built_in();
        let Ok(entries) = std::fs::read_dir(themes_dir) else {
            return catalog;
        };
        let mut files: Vec<_> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
            .collect();
        files.sort();
        for path in files {
            if let Err(error) = catalog.load_file(&path) {
                catalog.warnings.push(error.to_string());
            }
        }
        catalog
    }

    fn load_file(&mut self, path: &Path) -> Result<(), ConfigError> {
        let Some(file) = read_toml::<ThemeFile>(path)? else {
            return Ok(());
        };
        let base_name = file.extends.as_deref().unwrap_or("dark");
        let mut theme = self.get(base_name)?;
        theme.name = file.name;
        if let Some(borders) = file.borders {
            theme.borders = borders;
        }
        for unknown in theme.apply_overrides(&file.colors) {
            self.warnings.push(format!(
                "{}: unknown theme color '{unknown}'",
                path.display()
            ));
        }
        self.themes.retain(|existing| existing.name != theme.name);
        self.themes.push(theme);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Result<Theme, ConfigError> {
        self.themes
            .iter()
            .find(|theme| theme.name.eq_ignore_ascii_case(name))
            .cloned()
            .ok_or_else(|| ConfigError::UnknownTheme(name.to_owned()))
    }

    /// Returns the named theme, falling back to the built-in dark theme so a missing or
    /// misspelled theme never prevents startup.
    pub fn resolve(&self, name: &str) -> Theme {
        self.get(name).unwrap_or_else(|_| dark())
    }

    pub fn names(&self) -> Vec<&str> {
        self.themes
            .iter()
            .map(|theme| theme.name.as_str())
            .collect()
    }
}

fn rgb(hex: u32) -> ColorSpec {
    let [_, red, green, blue] = hex.to_be_bytes();
    ColorSpec::Rgb(red, green, blue)
}

fn dark() -> Theme {
    Theme {
        name: "dark".into(),
        borders: BorderKind::Rounded,
        colors: ThemeColors {
            background: rgb(0x16_1821),
            surface: rgb(0x1e_2030),
            foreground: rgb(0xc8_d3f5),
            muted: rgb(0x63_6da6),
            border: rgb(0x3b_4261),
            border_focused: rgb(0x82_aaff),
            accent: rgb(0x82_aaff),
            accent_secondary: rgb(0xc0_99ff),
            selection_bg: rgb(0x2d_3f76),
            selection_fg: rgb(0xe4_eaff),
            cursor_line: rgb(0x22_2436),
            status_bg: rgb(0x1e_2030),
            status_fg: rgb(0x82_8bb8),
            error: rgb(0xff_757f),
            warning: rgb(0xff_c777),
            success: rgb(0xc3_e88d),
            info: rgb(0x86_e1fc),
            syntax_keyword: rgb(0xc0_99ff),
            syntax_string: rgb(0xc3_e88d),
            syntax_number: rgb(0xff_966c),
            syntax_comment: rgb(0x63_6da6),
            syntax_identifier: rgb(0xc8_d3f5),
            syntax_operator: rgb(0x89_ddff),
            syntax_function: rgb(0x82_aaff),
            syntax_type: rgb(0xff_c777),
            grid_header_bg: rgb(0x22_2436),
            grid_header_fg: rgb(0x82_aaff),
            grid_row_alt_bg: rgb(0x1b_1d2b),
            grid_null: rgb(0x54_5c7e),
            grid_modified_bg: rgb(0x3d_3420),
            grid_inserted_bg: rgb(0x1f_3a2a),
            grid_deleted_fg: rgb(0xff_757f),
            grid_cursor_bg: rgb(0x36_4a82),
            env_production: rgb(0xff_5370),
            env_staging: rgb(0xff_c777),
            env_testing: rgb(0x86_e1fc),
            env_development: rgb(0xc3_e88d),
        },
    }
}

fn light() -> Theme {
    Theme {
        name: "light".into(),
        borders: BorderKind::Rounded,
        colors: ThemeColors {
            background: rgb(0xfa_fafa),
            surface: rgb(0xf0_f0f3),
            foreground: rgb(0x38_3a42),
            muted: rgb(0x8a_8f98),
            border: rgb(0xc8_cad0),
            border_focused: rgb(0x40_78f2),
            accent: rgb(0x40_78f2),
            accent_secondary: rgb(0xa6_26a4),
            selection_bg: rgb(0xc9_d9fb),
            selection_fg: rgb(0x20_2228),
            cursor_line: rgb(0xec_eef4),
            status_bg: rgb(0xe8_e9ee),
            status_fg: rgb(0x52_5761),
            error: rgb(0xe4_5649),
            warning: rgb(0xb0_7d00),
            success: rgb(0x2e_8b3e),
            info: rgb(0x01_84bc),
            syntax_keyword: rgb(0xa6_26a4),
            syntax_string: rgb(0x50_a14f),
            syntax_number: rgb(0x98_6801),
            syntax_comment: rgb(0xa0_a1a7),
            syntax_identifier: rgb(0x38_3a42),
            syntax_operator: rgb(0x01_84bc),
            syntax_function: rgb(0x40_78f2),
            syntax_type: rgb(0xc1_8401),
            grid_header_bg: rgb(0xe8_e9ee),
            grid_header_fg: rgb(0x40_78f2),
            grid_row_alt_bg: rgb(0xf4_f4f6),
            grid_null: rgb(0xa0_a1a7),
            grid_modified_bg: rgb(0xfb_efc7),
            grid_inserted_bg: rgb(0xd9_f2dc),
            grid_deleted_fg: rgb(0xe4_5649),
            grid_cursor_bg: rgb(0xb4_cbf8),
            env_production: rgb(0xd7_263d),
            env_staging: rgb(0xb0_7d00),
            env_testing: rgb(0x01_84bc),
            env_development: rgb(0x2e_8b3e),
        },
    }
}

fn high_contrast() -> Theme {
    let named = |index| ColorSpec::Named(index);
    Theme {
        name: "high-contrast".into(),
        borders: BorderKind::Thick,
        colors: ThemeColors {
            background: named(0),
            surface: named(0),
            foreground: named(15),
            muted: named(7),
            border: named(7),
            border_focused: named(11),
            accent: named(11),
            accent_secondary: named(14),
            selection_bg: named(11),
            selection_fg: named(0),
            cursor_line: named(8),
            status_bg: named(15),
            status_fg: named(0),
            error: named(9),
            warning: named(11),
            success: named(10),
            info: named(14),
            syntax_keyword: named(14),
            syntax_string: named(10),
            syntax_number: named(13),
            syntax_comment: named(7),
            syntax_identifier: named(15),
            syntax_operator: named(11),
            syntax_function: named(12),
            syntax_type: named(11),
            grid_header_bg: named(15),
            grid_header_fg: named(0),
            grid_row_alt_bg: named(0),
            grid_null: named(7),
            grid_modified_bg: named(3),
            grid_inserted_bg: named(2),
            grid_deleted_fg: named(9),
            grid_cursor_bg: named(4),
            env_production: named(9),
            env_staging: named(11),
            env_testing: named(14),
            env_development: named(10),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_themes_exist() {
        let catalog = ThemeCatalog::built_in();
        assert_eq!(catalog.names(), vec!["dark", "light", "high-contrast"]);
        assert!(catalog.get("DARK").is_ok());
        assert!(catalog.get("solarized").is_err());
        assert_eq!(catalog.resolve("solarized").name, "dark");
    }

    #[test]
    fn user_themes_extend_built_ins_and_report_problems() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("mine.toml"),
            "name = \"mine\"\nextends = \"light\"\nborders = \"double\"\n\
             [colors]\naccent = \"#ff8800\"\nsparkle = \"red\"\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("broken.toml"), "name = ").unwrap();
        let catalog = ThemeCatalog::load(dir.path());
        let mine = catalog.get("mine").unwrap();
        assert_eq!(mine.colors.accent, ColorSpec::Rgb(0xff, 0x88, 0x00));
        assert_eq!(mine.colors.background, light().colors.background);
        assert_eq!(mine.borders, BorderKind::Double);
        assert_eq!(catalog.warnings.len(), 2);
    }

    #[test]
    fn palettes_map_every_field() {
        let names = dark().colors.map(ToString::to_string);
        assert_eq!(names.accent, "#82aaff");
    }

    #[test]
    fn every_field_can_be_overridden() {
        let mut colors = dark().colors;
        for field in ThemeColors::FIELDS {
            assert!(colors.set(field, ColorSpec::Reset));
        }
        assert!(!colors.set("nope", ColorSpec::Reset));
    }
}
