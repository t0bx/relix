/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ProjectError;
use chrono::{DateTime, Utc};
use relix_config::files::{read_toml, write_toml};
use relix_core::ProjectId;
use serde::{Deserialize, Serialize};
use std::path::Path;

const MAX_RECENT: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentEntry {
    pub id: ProjectId,
    pub opened_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RecentProjects {
    #[serde(default)]
    pub entries: Vec<RecentEntry>,
}

impl RecentProjects {
    pub fn load(path: &Path) -> Self {
        read_toml(path).ok().flatten().unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<(), ProjectError> {
        write_toml(path, self).map_err(ProjectError::from)
    }

    pub fn touch(&mut self, id: ProjectId) {
        self.entries.retain(|entry| entry.id != id);
        self.entries.insert(
            0,
            RecentEntry {
                id,
                opened_at: Utc::now(),
            },
        );
        self.entries.truncate(MAX_RECENT);
    }

    pub fn remove(&mut self, id: ProjectId) {
        self.entries.retain(|entry| entry.id != id);
    }

    pub fn most_recent(&self) -> Option<ProjectId> {
        self.entries.first().map(|entry| entry.id)
    }

    pub fn position(&self, id: ProjectId) -> Option<usize> {
        self.entries.iter().position(|entry| entry.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_moves_project_to_front_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recent.toml");
        let (first, second) = (ProjectId::new(), ProjectId::new());
        let mut recent = RecentProjects::default();
        recent.touch(first);
        recent.touch(second);
        recent.touch(first);
        assert_eq!(recent.most_recent(), Some(first));
        assert_eq!(recent.entries.len(), 2);
        recent.save(&path).unwrap();
        assert_eq!(RecentProjects::load(&path), recent);
        recent.remove(first);
        assert_eq!(recent.most_recent(), Some(second));
    }
}
