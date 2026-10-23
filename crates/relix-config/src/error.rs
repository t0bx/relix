/*
 * @author t0bx
 * @since 23.10.2026
 */

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("no platform configuration directory could be determined")]
    NoHomeDirectory,
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid TOML in {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("failed to serialize configuration: {0}")]
    Serialize(String),
    #[error("invalid color '{0}'")]
    InvalidColor(String),
    #[error("invalid key binding '{0}'")]
    InvalidKey(String),
    #[error("unknown theme '{0}'")]
    UnknownTheme(String),
}

impl ConfigError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
