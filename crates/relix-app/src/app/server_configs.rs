/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use crate::datasource_form::{ServerChoice, ServerChoices};
use crate::notifications::Level;
use relix_core::{ConfigScope, ProjectId};
use relix_project::{ServerConfig, ServerConfigStore};

impl App {
    pub(crate) fn server_store(&self) -> ServerConfigStore {
        ServerConfigStore::new(
            self.context.paths.servers_file(),
            self.context.paths.projects_dir(),
        )
    }

    pub(crate) fn load_server_catalog(&mut self, project: Option<ProjectId>) {
        let (catalog, warnings) = self.server_store().load_catalog(project);
        self.servers = catalog;
        for warning in warnings {
            self.notify(Level::Warning, warning);
        }
    }

    pub(crate) fn server_choices(&self) -> ServerChoices {
        let mut choices = ServerChoices::default();
        for entry in self.servers.entries() {
            let label = format!("{} · {}", entry.config.name(), entry.scope.label());
            let choice = ServerChoice {
                id: entry.config.id(),
                label,
            };
            match entry.config {
                ServerConfig::Ssh(_) => choices.ssh.push(choice),
                ServerConfig::Vpn(_) => choices.vpn.push(choice),
            }
        }
        choices
    }

    pub(crate) fn save_server_scope(&mut self, scope: ConfigScope) -> bool {
        match self.server_store().save_catalog(&self.servers, scope) {
            Ok(()) => true,
            Err(error) => {
                self.notify(
                    Level::Error,
                    format!(
                        "could not save {} server configurations: {error}",
                        scope.label()
                    ),
                );
                false
            }
        }
    }
}
