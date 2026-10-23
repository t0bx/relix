/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ProjectError;
use crate::jsonl;
use crate::model::{Project, validate_name};
use crate::session::SessionState;
use relix_config::files::{ensure_private_dir, quarantine, read_toml, write_atomic, write_toml};
use relix_core::ProjectId;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::path::{Path, PathBuf};

const PROJECT_FILE: &str = "project.toml";
const SESSION_FILE: &str = "session.json";
const HISTORY_FILE: &str = "history.jsonl";
const FILTERS_FILE: &str = "filters.json";
const SCRIPTS_DIR: &str = "scripts";
const DRAFTS_DIR: &str = "drafts";

#[derive(Debug, Clone)]
pub struct ProjectStore {
    root: PathBuf,
}

#[derive(Debug, Default)]
pub struct ProjectListing {
    pub projects: Vec<Project>,
    pub warnings: Vec<String>,
}

impl ProjectStore {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn project_dir(&self, id: ProjectId) -> PathBuf {
        self.root.join(id.to_string())
    }

    pub fn scripts_dir(&self, id: ProjectId) -> PathBuf {
        self.project_dir(id).join(SCRIPTS_DIR)
    }

    pub fn list(&self) -> Result<ProjectListing, ProjectError> {
        let mut listing = ProjectListing::default();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(listing),
            Err(error) => return Err(ProjectError::io(&self.root, error)),
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path().join(PROJECT_FILE);
            match read_toml::<Project>(&path) {
                Ok(Some(project)) => listing.projects.push(project),
                Ok(None) => {}
                Err(error) => listing.warnings.push(error.to_string()),
            }
        }
        listing
            .projects
            .sort_by_key(|project| project.name.to_lowercase());
        Ok(listing)
    }

    pub fn create(&self, name: &str) -> Result<Project, ProjectError> {
        let project = Project::new(name)?;
        self.ensure_unique_name(&project.name, None)?;
        ensure_private_dir(&self.scripts_dir(project.id))?;
        self.save(&project)?;
        Ok(project)
    }

    pub fn load(&self, id: ProjectId) -> Result<Project, ProjectError> {
        read_toml::<Project>(&self.project_file(id))?.ok_or(ProjectError::NotFound(id))
    }

    pub fn save(&self, project: &Project) -> Result<(), ProjectError> {
        write_toml(&self.project_file(project.id), project).map_err(ProjectError::from)
    }

    pub fn rename(&self, id: ProjectId, new_name: &str) -> Result<Project, ProjectError> {
        let name = validate_name(new_name)?;
        self.ensure_unique_name(&name, Some(id))?;
        let mut project = self.load(id)?;
        project.name = name;
        self.save(&project)?;
        Ok(project)
    }

    pub fn delete(&self, id: ProjectId) -> Result<(), ProjectError> {
        let dir = self.project_dir(id);
        if !dir.join(PROJECT_FILE).exists() {
            return Err(ProjectError::NotFound(id));
        }
        fs::remove_dir_all(&dir).map_err(|error| ProjectError::io(dir, error))
    }

    /// Returns the stored session; a corrupt session file is moved aside and `None` is
    /// returned so the project still opens with a fresh layout.
    pub fn load_session(&self, id: ProjectId) -> Result<Option<SessionState>, ProjectError> {
        let path = self.project_dir(id).join(SESSION_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(ProjectError::io(path, error)),
        };
        match serde_json::from_str(&text) {
            Ok(session) => Ok(Some(session)),
            Err(_) => {
                quarantine(&path)?;
                Ok(None)
            }
        }
    }

    pub fn save_session(&self, id: ProjectId, session: &SessionState) -> Result<(), ProjectError> {
        self.write_json(&self.project_dir(id).join(SESSION_FILE), session)
    }

    pub fn write_draft(&self, id: ProjectId, draft: &str, text: &str) -> Result<(), ProjectError> {
        let path = self.draft_path(id, draft)?;
        write_atomic(&path, text.as_bytes()).map_err(ProjectError::from)
    }

    pub fn read_draft(&self, id: ProjectId, draft: &str) -> Result<Option<String>, ProjectError> {
        read_optional(&self.draft_path(id, draft)?)
    }

    /// Deletes every draft that is not referenced by the current session.
    pub fn prune_drafts(&self, id: ProjectId, keep: &[String]) -> Result<(), ProjectError> {
        let dir = self.project_dir(id).join(DRAFTS_DIR);
        let Ok(entries) = fs::read_dir(&dir) else {
            return Ok(());
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let stem = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !keep.contains(&stem) {
                fs::remove_file(&path).map_err(|error| ProjectError::io(&path, error))?;
            }
        }
        Ok(())
    }

    pub fn save_script(
        &self,
        id: ProjectId,
        file_name: &str,
        text: &str,
    ) -> Result<PathBuf, ProjectError> {
        let path = self.scripts_dir(id).join(script_file_name(file_name)?);
        write_atomic(&path, text.as_bytes())?;
        Ok(path)
    }

    pub fn resolve_script(&self, id: ProjectId, script: &Path) -> PathBuf {
        if script.is_absolute() {
            script.to_path_buf()
        } else {
            self.scripts_dir(id).join(script)
        }
    }

    pub fn list_scripts(&self, id: ProjectId) -> Result<Vec<PathBuf>, ProjectError> {
        let dir = self.scripts_dir(id);
        let Ok(entries) = fs::read_dir(&dir) else {
            return Ok(Vec::new());
        };
        let mut scripts: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
            .collect();
        scripts.sort();
        Ok(scripts)
    }

    pub fn append_history<T: Serialize>(
        &self,
        id: ProjectId,
        entry: &T,
    ) -> Result<(), ProjectError> {
        jsonl::append(&self.project_dir(id).join(HISTORY_FILE), entry)
    }

    pub fn load_history<T: DeserializeOwned>(
        &self,
        id: ProjectId,
        limit: usize,
    ) -> Result<Vec<T>, ProjectError> {
        jsonl::read_tail(&self.project_dir(id).join(HISTORY_FILE), limit)
    }

    pub fn compact_history<T: Serialize>(
        &self,
        id: ProjectId,
        entries: &[T],
    ) -> Result<(), ProjectError> {
        jsonl::rewrite(&self.project_dir(id).join(HISTORY_FILE), entries)
    }

    pub fn load_filters<T: DeserializeOwned + Default>(
        &self,
        id: ProjectId,
    ) -> Result<T, ProjectError> {
        let path = self.project_dir(id).join(FILTERS_FILE);
        match read_optional(&path)? {
            Some(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
            None => Ok(T::default()),
        }
    }

    pub fn save_filters<T: Serialize>(
        &self,
        id: ProjectId,
        filters: &T,
    ) -> Result<(), ProjectError> {
        self.write_json(&self.project_dir(id).join(FILTERS_FILE), filters)
    }

    fn project_file(&self, id: ProjectId) -> PathBuf {
        self.project_dir(id).join(PROJECT_FILE)
    }

    fn draft_path(&self, id: ProjectId, draft: &str) -> Result<PathBuf, ProjectError> {
        if draft.is_empty()
            || !draft
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            return Err(ProjectError::InvalidName(format!(
                "invalid draft id '{draft}'"
            )));
        }
        Ok(self
            .project_dir(id)
            .join(DRAFTS_DIR)
            .join(format!("{draft}.sql")))
    }

    fn ensure_unique_name(
        &self,
        name: &str,
        except: Option<ProjectId>,
    ) -> Result<(), ProjectError> {
        let clash =
            self.list()?.projects.into_iter().any(|project| {
                Some(project.id) != except && project.name.eq_ignore_ascii_case(name)
            });
        if clash {
            return Err(ProjectError::DuplicateName(name.to_owned()));
        }
        Ok(())
    }

    fn write_json<T: Serialize>(&self, path: &Path, value: &T) -> Result<(), ProjectError> {
        let text = serde_json::to_string_pretty(value).map_err(|error| ProjectError::Corrupt {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;
        write_atomic(path, text.as_bytes()).map_err(ProjectError::from)
    }
}

fn read_optional(path: &Path) -> Result<Option<String>, ProjectError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ProjectError::io(path, error)),
    }
}

/// Normalizes a user supplied script name into a safe file name inside the scripts
/// directory: path separators and parent references are rejected, `.sql` is appended.
fn script_file_name(name: &str) -> Result<String, ProjectError> {
    let trimmed = name.trim();
    let invalid = trimmed.is_empty()
        || trimmed.starts_with('.')
        || trimmed.chars().any(|ch| {
            matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || ch.is_control()
        });
    if invalid {
        return Err(ProjectError::InvalidName(format!(
            "invalid script name '{name}'"
        )));
    }
    if trimmed.to_ascii_lowercase().ends_with(".sql") {
        Ok(trimmed.to_owned())
    } else {
        Ok(format!("{trimmed}.sql"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{EditorSnapshot, LayoutSnapshot, PaneSnapshot, TabSnapshot};
    use pretty_assertions::assert_eq;

    fn store() -> (tempfile::TempDir, ProjectStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path().join("projects"));
        (dir, store)
    }

    #[test]
    fn create_list_rename_delete() {
        let (_dir, store) = store();
        let dev = store.create("Development").unwrap();
        store.create("Personal").unwrap();
        assert!(matches!(
            store.create("development"),
            Err(ProjectError::DuplicateName(_))
        ));
        let names: Vec<_> = store
            .list()
            .unwrap()
            .projects
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, vec!["Development", "Personal"]);
        assert!(store.rename(dev.id, "Personal").is_err());
        assert_eq!(store.rename(dev.id, "Dev").unwrap().name, "Dev");
        store.delete(dev.id).unwrap();
        assert!(matches!(store.load(dev.id), Err(ProjectError::NotFound(_))));
        assert_eq!(store.list().unwrap().projects.len(), 1);
    }

    #[test]
    fn corrupt_project_files_are_reported_not_fatal() {
        let (_dir, store) = store();
        store.create("Good").unwrap();
        let broken = store.root.join("broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join(PROJECT_FILE), "name = ").unwrap();
        let listing = store.list().unwrap();
        assert_eq!(listing.projects.len(), 1);
        assert_eq!(listing.warnings.len(), 1);
    }

    #[test]
    fn session_roundtrip_and_corruption_recovery() {
        let (_dir, store) = store();
        let project = store.create("Dev").unwrap();
        assert_eq!(store.load_session(project.id).unwrap(), None);
        let session = SessionState {
            layout: LayoutSnapshot::Pane(PaneSnapshot {
                tabs: vec![TabSnapshot::Editor(EditorSnapshot {
                    title: "query_1.sql".into(),
                    script: None,
                    draft: Some("abc".into()),
                    data_source: None,
                    cursor_line: 2,
                    cursor_column: 4,
                })],
                active: 0,
            }),
            ..SessionState::default()
        };
        store.save_session(project.id, &session).unwrap();
        assert_eq!(store.load_session(project.id).unwrap(), Some(session));
        fs::write(store.project_dir(project.id).join(SESSION_FILE), "{").unwrap();
        assert_eq!(store.load_session(project.id).unwrap(), None);
    }

    #[test]
    fn drafts_and_scripts() {
        let (_dir, store) = store();
        let project = store.create("Dev").unwrap();
        store
            .write_draft(project.id, "draft-1", "select 1")
            .unwrap();
        store
            .write_draft(project.id, "draft-2", "select 2")
            .unwrap();
        assert!(store.write_draft(project.id, "../escape", "x").is_err());
        store
            .prune_drafts(project.id, &["draft-2".to_owned()])
            .unwrap();
        assert_eq!(store.read_draft(project.id, "draft-1").unwrap(), None);
        assert_eq!(
            store.read_draft(project.id, "draft-2").unwrap().as_deref(),
            Some("select 2")
        );
        let path = store.save_script(project.id, "report", "select 3").unwrap();
        assert!(path.ends_with("scripts/report.sql"));
        assert!(store.save_script(project.id, "../etc/passwd", "x").is_err());
        assert_eq!(store.list_scripts(project.id).unwrap(), vec![path]);
    }
}
