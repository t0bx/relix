/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::color::ColorSpec;
use crate::error::ConfigError;
use crate::files::{quarantine, read_toml, write_toml};
use crate::keys::KeySequence;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const CURRENT_CONFIG_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    pub show_hints: bool,
    pub sidebar_width: u16,
    pub mouse: bool,
    pub show_line_numbers: bool,
    pub unicode_icons: bool,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            show_hints: true,
            sidebar_width: 34,
            mouse: true,
            show_line_numbers: true,
            unicode_icons: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    pub tab_width: u8,
    pub auto_complete: bool,
}

impl Default for EditorSettings {
    fn default() -> Self {
        Self {
            tab_width: 4,
            auto_complete: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct QuerySettings {
    pub row_limit: usize,
    pub page_size: u32,
    pub timeout_secs: u64,
    pub confirm_destructive: bool,
    pub history_limit: usize,
}

impl Default for QuerySettings {
    fn default() -> Self {
        Self {
            row_limit: 500,
            page_size: 200,
            timeout_secs: 300,
            confirm_destructive: true,
            history_limit: 1000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverRegistrySettings {
    pub name: String,
    pub url: String,
    pub public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DriverSettings {
    pub offline: bool,
    pub registries: Vec<DriverRegistrySettings>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum KeyBindingValue {
    One(String),
    Many(Vec<String>),
}

impl KeyBindingValue {
    pub fn specs(&self) -> Vec<&str> {
        match self {
            Self::One(spec) => vec![spec.as_str()],
            Self::Many(specs) => specs.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalSettings {
    pub config_version: u32,
    pub theme: String,
    pub ui: UiSettings,
    pub editor: EditorSettings,
    pub query: QuerySettings,
    pub drivers: DriverSettings,
    pub keybindings: BTreeMap<String, KeyBindingValue>,
    pub theme_overrides: BTreeMap<String, ColorSpec>,
}

impl Default for GlobalSettings {
    fn default() -> Self {
        Self {
            config_version: CURRENT_CONFIG_VERSION,
            theme: "dark".to_owned(),
            ui: UiSettings::default(),
            editor: EditorSettings::default(),
            query: QuerySettings::default(),
            drivers: DriverSettings::default(),
            keybindings: BTreeMap::new(),
            theme_overrides: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SettingsLoad {
    pub settings: GlobalSettings,
    pub warnings: Vec<String>,
    pub recovered_from: Option<PathBuf>,
}

impl GlobalSettings {
    /// Loads, migrates and validates the settings file. A missing file produces defaults
    /// (written to disk for discoverability); a corrupt file is quarantined and replaced
    /// by defaults so startup never fails because of configuration.
    pub fn load_or_recover(path: &Path) -> Result<SettingsLoad, ConfigError> {
        let mut warnings = Vec::new();
        let mut recovered_from = None;
        let loaded = match read_toml::<GlobalSettings>(path) {
            Ok(loaded) => loaded,
            Err(ConfigError::Parse { message, .. }) => {
                let backup = quarantine(path)?;
                warnings.push(format!(
                    "configuration was invalid ({message}); defaults restored, previous file saved as {}",
                    backup.display()
                ));
                recovered_from = Some(backup);
                None
            }
            Err(other) => return Err(other),
        };
        let needs_write = loaded
            .as_ref()
            .is_none_or(|settings| settings.config_version < CURRENT_CONFIG_VERSION);
        let mut settings = loaded.unwrap_or_default();
        settings.migrate();
        warnings.extend(settings.validate());
        if needs_write {
            write_toml(path, &settings)?;
        }
        Ok(SettingsLoad {
            settings,
            warnings,
            recovered_from,
        })
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        write_toml(path, self)
    }

    fn migrate(&mut self) {
        if self.config_version == 0 {
            self.config_version = 1;
        }
    }

    /// Clamps out-of-range values and drops unparsable key bindings, returning one
    /// human readable warning per correction.
    pub fn validate(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();
        let mut clamp = |name: &str, value: &mut u64, min: u64, max: u64| {
            let clamped = (*value).clamp(min, max);
            if clamped != *value {
                warnings.push(format!("{name} = {value} is out of range, using {clamped}"));
                *value = clamped;
            }
        };
        let mut row_limit = self.query.row_limit as u64;
        clamp("query.row_limit", &mut row_limit, 1, 1_000_000);
        self.query.row_limit = usize::try_from(row_limit).unwrap_or(500);
        let mut page_size = u64::from(self.query.page_size);
        clamp("query.page_size", &mut page_size, 10, 100_000);
        self.query.page_size = u32::try_from(page_size).unwrap_or(200);
        let mut sidebar = u64::from(self.ui.sidebar_width);
        clamp("ui.sidebar_width", &mut sidebar, 16, 120);
        self.ui.sidebar_width = u16::try_from(sidebar).unwrap_or(34);
        let mut tab_width = u64::from(self.editor.tab_width);
        clamp("editor.tab_width", &mut tab_width, 1, 16);
        self.editor.tab_width = u8::try_from(tab_width).unwrap_or(4);
        self.keybindings.retain(|command, value| {
            let invalid: Vec<&str> = value
                .specs()
                .into_iter()
                .filter(|spec| spec.parse::<KeySequence>().is_err())
                .collect();
            for spec in &invalid {
                warnings.push(format!("ignoring invalid key '{spec}' for '{command}'"));
            }
            invalid.is_empty()
        });
        warnings
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn missing_file_creates_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let load = GlobalSettings::load_or_recover(&path).unwrap();
        assert_eq!(load.settings, GlobalSettings::default());
        assert!(path.exists());
        assert!(load.warnings.is_empty());
    }

    #[test]
    fn corrupt_file_is_quarantined() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "theme = [unclosed").unwrap();
        let load = GlobalSettings::load_or_recover(&path).unwrap();
        assert_eq!(load.settings.theme, "dark");
        assert!(load.recovered_from.unwrap().exists());
        assert_eq!(load.warnings.len(), 1);
    }

    #[test]
    fn partial_files_merge_with_defaults_and_validate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "config_version = 1\ntheme = \"light\"\n[query]\npage_size = 1\n\
             [keybindings]\n\"palette.open\" = [\"ctrl+k\", \"f1\"]\n\"editor.run\" = \"bogus+key\"\n",
        )
        .unwrap();
        let load = GlobalSettings::load_or_recover(&path).unwrap();
        assert_eq!(load.settings.theme, "light");
        assert_eq!(load.settings.query.page_size, 10);
        assert_eq!(load.settings.query.row_limit, 500);
        assert!(load.settings.keybindings.contains_key("palette.open"));
        assert!(!load.settings.keybindings.contains_key("editor.run"));
        assert_eq!(load.warnings.len(), 2);
    }

    #[test]
    fn legacy_versions_are_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "theme = \"dark\"\nconfig_version = 0\n").unwrap();
        let load = GlobalSettings::load_or_recover(&path).unwrap();
        assert_eq!(load.settings.config_version, CURRENT_CONFIG_VERSION);
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("config_version = 1"));
    }
}
