/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ProjectError;
use crate::server_catalog::ServerCatalog;
use crate::servers::{SERVERS_FORMAT_VERSION, ServerRegistry};
use relix_config::ConfigError;
use relix_config::files::{quarantine, read_toml, write_toml};
use relix_core::{ConfigScope, ProjectId};
use std::path::PathBuf;

const PROJECT_SERVERS_FILE: &str = "servers.toml";

#[derive(Debug, Clone)]
pub struct ServerConfigStore {
    global_file: PathBuf,
    projects_root: PathBuf,
}

#[derive(Debug, Default)]
pub struct RegistryLoad {
    pub registry: ServerRegistry,
    pub read_only: bool,
    pub warnings: Vec<String>,
}

impl ServerConfigStore {
    pub fn new(global_file: impl Into<PathBuf>, projects_root: impl Into<PathBuf>) -> Self {
        Self {
            global_file: global_file.into(),
            projects_root: projects_root.into(),
        }
    }

    pub fn path(&self, scope: ConfigScope, project: Option<ProjectId>) -> Option<PathBuf> {
        match scope {
            ConfigScope::Global => Some(self.global_file.clone()),
            ConfigScope::Project => project.map(|id| {
                self.projects_root
                    .join(id.to_string())
                    .join(PROJECT_SERVERS_FILE)
            }),
        }
    }

    /// Loads one scope. A file that cannot be parsed is moved aside and an empty registry
    /// is used, so the user's data survives for manual recovery. A file written by a
    /// newer format version is loaded read-only and never overwritten.
    pub fn load(&self, scope: ConfigScope, project: Option<ProjectId>) -> RegistryLoad {
        let Some(path) = self.path(scope, project) else {
            return RegistryLoad::default();
        };
        match read_toml::<ServerRegistry>(&path) {
            Ok(None) => RegistryLoad::default(),
            Ok(Some(registry)) if registry.format_version > SERVERS_FORMAT_VERSION => {
                RegistryLoad {
                    warnings: vec![format!(
                        "{} was written by a newer Relix (format {}); server configurations \
                         are read-only until you upgrade",
                        path.display(),
                        registry.format_version
                    )],
                    registry,
                    read_only: true,
                }
            }
            Ok(Some(mut registry)) => {
                registry.format_version = SERVERS_FORMAT_VERSION;
                RegistryLoad {
                    registry,
                    ..RegistryLoad::default()
                }
            }
            Err(ConfigError::Parse { message, .. }) => match quarantine(&path) {
                Ok(backup) => RegistryLoad {
                    warnings: vec![format!(
                        "{} is corrupt ({message}); it was moved to {}",
                        path.display(),
                        backup.display()
                    )],
                    ..RegistryLoad::default()
                },
                Err(error) => RegistryLoad {
                    read_only: true,
                    warnings: vec![format!(
                        "{} is corrupt and could not be moved aside: {error}",
                        path.display()
                    )],
                    ..RegistryLoad::default()
                },
            },
            Err(error) => RegistryLoad {
                read_only: true,
                warnings: vec![error.to_string()],
                ..RegistryLoad::default()
            },
        }
    }

    pub fn save(
        &self,
        scope: ConfigScope,
        project: Option<ProjectId>,
        registry: &ServerRegistry,
    ) -> Result<(), ProjectError> {
        let path = self
            .path(scope, project)
            .ok_or_else(|| ProjectError::InvalidName("no project is open".into()))?;
        write_toml(&path, registry).map_err(ProjectError::from)
    }

    pub fn load_catalog(&self, project: Option<ProjectId>) -> (ServerCatalog, Vec<String>) {
        let global = self.load(ConfigScope::Global, None);
        let local = self.load(ConfigScope::Project, project);
        let warnings = global
            .warnings
            .iter()
            .chain(&local.warnings)
            .cloned()
            .collect();
        let mut catalog = ServerCatalog::new(global.registry, local.registry, project);
        catalog.set_read_only(ConfigScope::Global, global.read_only);
        catalog.set_read_only(ConfigScope::Project, local.read_only || project.is_none());
        (catalog, warnings)
    }

    pub fn save_catalog(
        &self,
        catalog: &ServerCatalog,
        scope: ConfigScope,
    ) -> Result<(), ProjectError> {
        if catalog.is_read_only(scope) {
            return Err(ProjectError::InvalidName(format!(
                "{} server configurations are read-only",
                scope.label()
            )));
        }
        self.save(scope, catalog.project_id(), catalog.registry(scope))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::servers::ServerConfig;
    use relix_core::SshServerConfig;
    use std::fs;

    fn store() -> (tempfile::TempDir, ServerConfigStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ServerConfigStore::new(
            dir.path().join("config/servers.toml"),
            dir.path().join("projects"),
        );
        (dir, store)
    }

    #[test]
    fn global_and_project_scopes_roundtrip_separately() {
        let (_dir, store) = store();
        let project = ProjectId::new();
        let (mut catalog, warnings) = store.load_catalog(Some(project));
        assert!(warnings.is_empty());
        let global = SshServerConfig::new("Personal VPS", "vps.example.com");
        let local = SshServerConfig::new("Production Bastion", "bastion.customer-a");
        catalog
            .registry_mut(ConfigScope::Global)
            .upsert(ServerConfig::Ssh(global.clone()))
            .unwrap();
        catalog
            .registry_mut(ConfigScope::Project)
            .upsert(ServerConfig::Ssh(local.clone()))
            .unwrap();
        store.save_catalog(&catalog, ConfigScope::Global).unwrap();
        store.save_catalog(&catalog, ConfigScope::Project).unwrap();

        let (reloaded, _) = store.load_catalog(Some(project));
        assert_eq!(
            reloaded.registry(ConfigScope::Global).ssh(global.id),
            Some(&global)
        );
        assert_eq!(
            reloaded.registry(ConfigScope::Project).ssh(local.id),
            Some(&local)
        );
        let (other_project, _) = store.load_catalog(Some(ProjectId::new()));
        assert!(other_project.registry(ConfigScope::Project).is_empty());
        assert!(other_project.ssh(local.id).is_none());
    }

    #[test]
    fn corrupt_files_are_quarantined_not_discarded() {
        let (_dir, store) = store();
        let path = store.path(ConfigScope::Global, None).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "ssh = [ broken").unwrap();
        let load = store.load(ConfigScope::Global, None);
        assert!(load.registry.is_empty());
        assert_eq!(load.warnings.len(), 1);
        assert!(!path.exists());
        assert!(path.with_extension("corrupt-0").exists());
    }

    #[test]
    fn newer_formats_are_read_only() {
        let (_dir, store) = store();
        let path = store.path(ConfigScope::Global, None).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "format_version = 99\n").unwrap();
        let (catalog, warnings) = store.load_catalog(None);
        assert!(catalog.is_read_only(ConfigScope::Global));
        assert!(warnings[0].contains("newer Relix"));
        assert!(store.save_catalog(&catalog, ConfigScope::Global).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "format_version = 99\n");
    }

    #[test]
    fn project_scope_requires_an_open_project() {
        let (_dir, store) = store();
        let (catalog, _) = store.load_catalog(None);
        assert!(catalog.is_read_only(ConfigScope::Project));
    }
}
