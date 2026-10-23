/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ProjectError;
use crate::model::Project;
use crate::store::ProjectStore;
use chrono::Utc;
use relix_core::{DataSourceId, ProjectId};
use serde::{Deserialize, Serialize};

pub const EXPORT_FORMAT: &str = "relix-project";

#[derive(Debug, Serialize, Deserialize)]
struct ProjectExport {
    format: String,
    project: Project,
}

/// Serializes a project for sharing. Credentials are never part of a project file, and
/// the export additionally disables password saving so importers are prompted instead.
pub fn export_project(project: &Project) -> Result<String, ProjectError> {
    let mut project = project.clone();
    for source in &mut project.data_sources {
        source.settings.save_password = false;
    }
    let export = ProjectExport {
        format: EXPORT_FORMAT.to_owned(),
        project,
    };
    toml::to_string_pretty(&export).map_err(|error| ProjectError::Corrupt {
        path: "export".into(),
        message: error.to_string(),
    })
}

pub fn import_project(
    store: &ProjectStore,
    text: &str,
    name: Option<&str>,
) -> Result<Project, ProjectError> {
    let export: ProjectExport = toml::from_str(text).map_err(|error| ProjectError::Corrupt {
        path: "import".into(),
        message: error.message().to_owned(),
    })?;
    if export.format != EXPORT_FORMAT {
        return Err(ProjectError::Corrupt {
            path: "import".into(),
            message: format!("unsupported export format '{}'", export.format),
        });
    }
    let imported_name = name.unwrap_or(&export.project.name);
    let mut project = store.create(imported_name)?;
    project.settings = export.project.settings;
    project.settings.default_data_source = None;
    project.created_at = Utc::now();
    for mut source in export.project.data_sources {
        source.id = DataSourceId::new();
        project.upsert_data_source(source)?;
    }
    store.save(&project)?;
    Ok(project)
}

pub fn exported_project_id(text: &str) -> Option<ProjectId> {
    toml::from_str::<ProjectExport>(text)
        .ok()
        .map(|export| export.project.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{DataSourceDefinition, DriverId};

    #[test]
    fn export_import_creates_independent_copy_without_saved_passwords() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path());
        let mut original = store.create("Production").unwrap();
        let mut source = DataSourceDefinition::new("Main", DriverId::from("postgres"));
        source.settings.save_password = true;
        original.upsert_data_source(source).unwrap();
        store.save(&original).unwrap();

        let text = export_project(&original).unwrap();
        assert!(text.contains("save_password = false"));
        assert_eq!(exported_project_id(&text), Some(original.id));
        let imported = import_project(&store, &text, Some("Production (shared)")).unwrap();
        assert_ne!(imported.id, original.id);
        assert_eq!(imported.data_sources.len(), 1);
        assert_ne!(imported.data_sources[0].id, original.data_sources[0].id);
        assert!(import_project(&store, "format = \"other\"", None).is_err());
    }
}
