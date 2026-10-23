/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ConfigError;
use crate::files::ensure_private_dir;
use directories::ProjectDirs;
use std::path::{Path, PathBuf};

pub const HOME_OVERRIDE_ENV: &str = "RELIX_HOME";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self, ConfigError> {
        if let Some(home) = std::env::var_os(HOME_OVERRIDE_ENV) {
            return Ok(Self::rooted(Path::new(&home)));
        }
        let dirs = ProjectDirs::from("dev", "t0bx", "relix").ok_or(ConfigError::NoHomeDirectory)?;
        Ok(Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_dir().to_path_buf(),
            cache_dir: dirs.cache_dir().to_path_buf(),
        })
    }

    pub fn rooted(root: &Path) -> Self {
        Self {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
            cache_dir: root.join("cache"),
        }
    }

    pub fn ensure(&self) -> Result<(), ConfigError> {
        for dir in [
            &self.config_dir,
            &self.data_dir,
            &self.cache_dir,
            &self.projects_dir(),
            &self.drivers_dir(),
            &self.logs_dir(),
        ] {
            ensure_private_dir(dir)?;
        }
        Ok(())
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn themes_dir(&self) -> PathBuf {
        self.config_dir.join("themes")
    }

    pub fn projects_dir(&self) -> PathBuf {
        self.data_dir.join("projects")
    }

    pub fn drivers_dir(&self) -> PathBuf {
        self.data_dir.join("drivers")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn recent_projects_file(&self) -> PathBuf {
        self.data_dir.join("recent.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rooted_paths_live_under_root() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::rooted(dir.path());
        paths.ensure().unwrap();
        assert!(paths.projects_dir().starts_with(dir.path()));
        assert!(paths.drivers_dir().is_dir());
        assert!(paths.settings_file().ends_with("config/config.toml"));
    }
}
