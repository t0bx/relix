/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ProjectError;
use chrono::{DateTime, Utc};
use relix_core::{DataSourceDefinition, DataSourceId, ProjectId};
use serde::{Deserialize, Serialize};

pub const PROJECT_FORMAT_VERSION: u32 = 1;
const MAX_NAME_LENGTH: usize = 100;

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectSettings {
    pub row_limit: Option<usize>,
    pub confirm_destructive: Option<bool>,
    pub default_data_source: Option<DataSourceId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    pub id: ProjectId,
    pub name: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub settings: ProjectSettings,
    #[serde(default)]
    pub data_sources: Vec<DataSourceDefinition>,
}

impl Project {
    pub fn new(name: &str) -> Result<Self, ProjectError> {
        Ok(Self {
            format_version: PROJECT_FORMAT_VERSION,
            id: ProjectId::new(),
            name: validate_name(name)?,
            created_at: Utc::now(),
            settings: ProjectSettings::default(),
            data_sources: Vec::new(),
        })
    }

    pub fn data_source(&self, id: DataSourceId) -> Option<&DataSourceDefinition> {
        self.data_sources.iter().find(|source| source.id == id)
    }

    pub fn upsert_data_source(
        &mut self,
        definition: DataSourceDefinition,
    ) -> Result<(), ProjectError> {
        let name = validate_name(&definition.name)?;
        if self
            .data_sources
            .iter()
            .any(|source| source.id != definition.id && source.name.eq_ignore_ascii_case(&name))
        {
            return Err(ProjectError::DuplicateName(name));
        }
        let definition = DataSourceDefinition { name, ..definition };
        match self
            .data_sources
            .iter_mut()
            .find(|source| source.id == definition.id)
        {
            Some(existing) => *existing = definition,
            None => self.data_sources.push(definition),
        }
        Ok(())
    }

    pub fn remove_data_source(&mut self, id: DataSourceId) -> Option<DataSourceDefinition> {
        let index = self
            .data_sources
            .iter()
            .position(|source| source.id == id)?;
        if self.settings.default_data_source == Some(id) {
            self.settings.default_data_source = None;
        }
        Some(self.data_sources.remove(index))
    }

    pub fn duplicate_data_source(&mut self, id: DataSourceId) -> Option<DataSourceId> {
        let original = self.data_source(id)?.clone();
        let mut copy = DataSourceDefinition {
            id: DataSourceId::new(),
            ..original
        };
        copy.name = self.unique_data_source_name(&copy.name);
        let new_id = copy.id;
        self.data_sources.push(copy);
        Some(new_id)
    }

    fn unique_data_source_name(&self, base: &str) -> String {
        let taken = |name: &str| {
            self.data_sources
                .iter()
                .any(|source| source.name.eq_ignore_ascii_case(name))
        };
        let mut candidate = format!("{base} copy");
        let mut counter = 2;
        while taken(&candidate) {
            candidate = format!("{base} copy {counter}");
            counter += 1;
        }
        candidate
    }
}

pub fn validate_name(name: &str) -> Result<String, ProjectError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(ProjectError::InvalidName("name must not be empty".into()));
    }
    if trimmed.chars().count() > MAX_NAME_LENGTH {
        return Err(ProjectError::InvalidName(format!(
            "name must be at most {MAX_NAME_LENGTH} characters"
        )));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(ProjectError::InvalidName(
            "name must not contain control characters".into(),
        ));
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::DriverId;

    #[test]
    fn names_are_validated_and_trimmed() {
        assert_eq!(Project::new("  Development ").unwrap().name, "Development");
        assert!(Project::new("   ").is_err());
        assert!(Project::new("bad\nname").is_err());
    }

    #[test]
    fn data_source_names_are_unique_per_project() {
        let mut project = Project::new("Dev").unwrap();
        let local = DataSourceDefinition::new("Local", DriverId::from("postgres"));
        project.upsert_data_source(local.clone()).unwrap();
        let clash = DataSourceDefinition::new("local", DriverId::from("mysql"));
        assert!(matches!(
            project.upsert_data_source(clash),
            Err(ProjectError::DuplicateName(_))
        ));
        let renamed = DataSourceDefinition {
            name: "Local PG".into(),
            ..local
        };
        project.upsert_data_source(renamed).unwrap();
        assert_eq!(project.data_sources.len(), 1);
        assert_eq!(project.data_sources[0].name, "Local PG");
    }

    #[test]
    fn duplicates_get_fresh_ids_and_names() {
        let mut project = Project::new("Dev").unwrap();
        let source = DataSourceDefinition::new("Cache", DriverId::from("redis"));
        let id = source.id;
        project.upsert_data_source(source).unwrap();
        let first = project.duplicate_data_source(id).unwrap();
        let second = project.duplicate_data_source(id).unwrap();
        assert_ne!(first, id);
        assert_eq!(project.data_source(first).unwrap().name, "Cache copy");
        assert_eq!(project.data_source(second).unwrap().name, "Cache copy 2");
    }

    #[test]
    fn project_files_from_the_first_release_still_load() {
        let legacy = r#"
format_version = 1
id = "1c0f6a64-6f43-4a39-9b55-4dcf1f7d9c11"
name = "Development"
created_at = "2026-10-23T08:00:00Z"

[[data_sources]]
id = "7a4e9a2b-3c1d-4f5e-8a6b-9c0d1e2f3a4b"
name = "Main PG"
driver = "postgres"

[data_sources.settings]
host = "db.internal"
port = 5432
save_password = true

[data_sources.settings.tls]
mode = "verify_full"
"#;
        let project: Project = toml::from_str(legacy).unwrap();
        let source = &project.data_sources[0];
        assert!(source.network.is_direct());
        assert!(source.settings.properties.is_empty());
        assert_eq!(source.settings.query_timeout(), None);
        let saved = toml::to_string_pretty(&project).unwrap();
        let reloaded: Project = toml::from_str(&saved).unwrap();
        assert_eq!(reloaded, project);
    }

    #[test]
    fn removing_default_source_clears_default() {
        let mut project = Project::new("Dev").unwrap();
        let source = DataSourceDefinition::new("Notes", DriverId::from("sqlite"));
        let id = source.id;
        project.upsert_data_source(source).unwrap();
        project.settings.default_data_source = Some(id);
        assert!(project.remove_data_source(id).is_some());
        assert_eq!(project.settings.default_data_source, None);
    }
}
