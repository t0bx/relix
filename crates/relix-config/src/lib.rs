/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod color;
pub mod error;
pub mod files;
pub mod keys;
pub mod paths;
pub mod settings;
pub mod theme;

pub use color::ColorSpec;
pub use error::ConfigError;
pub use keys::{KeyChord, KeyCode, KeySequence, Modifiers};
pub use paths::AppPaths;
pub use settings::{
    DriverRegistrySettings, EditorSettings, GlobalSettings, QuerySettings, SettingsLoad, UiSettings,
};
pub use theme::{Theme, ThemeCatalog};
