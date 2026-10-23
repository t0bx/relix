/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ProjectError;
use crate::model::Project;
use crate::servers::ServerRegistry;
use crate::store::ProjectStore;
use chrono::Utc;
use relix_core::{DataSourceId, ProjectId, ServerConfigId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const EXPORT_FORMAT: &str = "relix-project";

#[derive(Debug, Serialize, Deserialize)]
struct ProjectExport {
    format: String,
    project: Project,
    #[serde(default)]
    servers: ServerRegistry,
}

pub struct ImportedProject {
    pub project: Project,
    pub servers: ServerRegistry,
}

/// Serializes a project and its project-scoped server configurations for sharing.
/// Credentials are never part of these files, and the export additionally disables
/// password saving so importers are prompted instead.
pub fn export_project(project: &Project, servers: &ServerRegistry) -> Result<String, ProjectError> {
    let mut project = project.clone();
    for source in &mut project.data_sources {
        source.settings.save_password = false;
    }
    let mut servers = servers.clone();
    for config in &mut servers.ssh {
        config.save_secret = false;
    }
    let export = ProjectExport {
        format: EXPORT_FORMAT.to_owned(),
        project,
        servers,
    };
    toml::to_string_pretty(&export).map_err(|error| ProjectError::Corrupt {
        path: "export".into(),
        message: error.to_string(),
    })
}

/// Imports a project as an independent copy. Data sources and project-scoped server
/// configurations get fresh identifiers and every reference between them is rewritten;
/// references to global configurations are kept and resolve if they exist locally.
pub fn import_project(
    store: &ProjectStore,
    text: &str,
    name: Option<&str>,
) -> Result<ImportedProject, ProjectError> {
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
    let (servers, renamed) = with_fresh_ids(export.servers);
    let remap = |id: Option<ServerConfigId>| id.map(|id| renamed.get(&id).copied().unwrap_or(id));
    for mut source in export.project.data_sources {
        source.id = DataSourceId::new();
        source.network.ssh = remap(source.network.ssh);
        source.network.vpn = remap(source.network.vpn);
        project.upsert_data_source(source)?;
    }
    store.save(&project)?;
    Ok(ImportedProject { project, servers })
}

fn with_fresh_ids(
    mut servers: ServerRegistry,
) -> (ServerRegistry, HashMap<ServerConfigId, ServerConfigId>) {
    let mut renamed = HashMap::new();
    for config in &mut servers.ssh {
        let fresh = ServerConfigId::new();
        renamed.insert(config.id, fresh);
        config.id = fresh;
    }
    for profile in &mut servers.vpn {
        let fresh = ServerConfigId::new();
        renamed.insert(profile.id, fresh);
        profile.id = fresh;
    }
    for config in &mut servers.ssh {
        config.jump_host = config
            .jump_host
            .map(|id| renamed.get(&id).copied().unwrap_or(id));
    }
    (servers, renamed)
}

pub fn exported_project_id(text: &str) -> Option<ProjectId> {
    toml::from_str::<ProjectExport>(text)
        .ok()
        .map(|export| export.project.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::servers::ServerConfig;
    use relix_core::{DataSourceDefinition, DriverId, NetworkMode, SshServerConfig};

    #[test]
    fn export_import_creates_independent_copy_without_saved_passwords() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path());
        let mut original = store.create("Production").unwrap();
        let mut source = DataSourceDefinition::new("Main", DriverId::from("postgres"));
        source.settings.save_password = true;
        original.upsert_data_source(source).unwrap();
        store.save(&original).unwrap();

        let text = export_project(&original, &ServerRegistry::default()).unwrap();
        assert!(text.contains("save_password = false"));
        assert_eq!(exported_project_id(&text), Some(original.id));
        let imported = import_project(&store, &text, Some("Production (shared)"))
            .unwrap()
            .project;
        assert_ne!(imported.id, original.id);
        assert_eq!(imported.data_sources.len(), 1);
        assert_ne!(imported.data_sources[0].id, original.data_sources[0].id);
        assert!(import_project(&store, "format = \"other\"", None).is_err());
    }

    #[test]
    fn project_servers_travel_with_the_export_and_references_are_remapped() {
        let dir = tempfile::tempdir().unwrap();
        let store = ProjectStore::new(dir.path());
        let mut original = store.create("Customer A").unwrap();
        let gateway = SshServerConfig::new("Gateway", "gw.customer-a");
        let mut bastion = SshServerConfig::new("Bastion", "bastion.customer-a");
        bastion.jump_host = Some(gateway.id);
        bastion.save_secret = true;
        let mut servers = ServerRegistry::default();
        servers.upsert(ServerConfig::Ssh(gateway.clone())).unwrap();
        servers.upsert(ServerConfig::Ssh(bastion.clone())).unwrap();
        let mut source = DataSourceDefinition::new("Main", DriverId::from("postgres"));
        source.network.mode = NetworkMode::SshTunnel;
        source.network.ssh = Some(bastion.id);
        original.upsert_data_source(source).unwrap();

        let text = export_project(&original, &servers).unwrap();
        let imported = import_project(&store, &text, Some("Customer A copy")).unwrap();
        let new_bastion = imported
            .servers
            .ssh
            .iter()
            .find(|config| config.name == "Bastion")
            .unwrap();
        let new_gateway = imported
            .servers
            .ssh
            .iter()
            .find(|config| config.name == "Gateway")
            .unwrap();
        assert_ne!(new_bastion.id, bastion.id);
        assert!(!new_bastion.save_secret);
        assert_eq!(new_bastion.jump_host, Some(new_gateway.id));
        assert_eq!(
            imported.project.data_sources[0].network.ssh,
            Some(new_bastion.id)
        );
    }
}
