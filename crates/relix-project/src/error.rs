/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_config::ConfigError;
use relix_core::ProjectId;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error(transparent)]
    Config(#[from] ConfigError),
    #[error("project {0} does not exist")]
    NotFound(ProjectId),
    #[error("a project named '{0}' already exists")]
    DuplicateName(String),
    #[error("invalid name: {0}")]
    InvalidName(String),
    #[error("failed to access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid data in {path}: {message}")]
    Corrupt { path: PathBuf, message: String },
    #[error("credential storage failed: {0}")]
    Secret(String),
}

impl ProjectError {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
