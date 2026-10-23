/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::App;
use crate::events::AppEvent;
use crate::loaders;
use crate::notifications::Level;
use crate::overlay::{Overlay, ViewerState};
use crate::tab::Tab;
use relix_core::{DataSourceId, Dialect};
use relix_driver_api::DriverError;
use relix_editor::{CatalogColumn, CompletionCatalog};

impl App {
    pub(crate) fn dispatch_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::Connected {
                source,
                request,
                result,
            } => self.on_connected(source, request, result),
            AppEvent::ConnectionTested { result, plan } => self.on_connection_tested(result, plan),
            AppEvent::SshTested { plan, result } => self.on_ssh_tested(plan, result),
            AppEvent::VpnDiagnosed { profile, lines } => self.on_vpn_diagnosed(profile, lines),
            AppEvent::VpnUpdated { profile, result } => self.on_vpn_updated(profile, result),
            AppEvent::DriverOffer {
                source,
                driver,
                result,
            } => self.on_driver_offer(source, driver, result),
            AppEvent::DriverInstalled {
                driver,
                retry,
                result,
            } => self.on_driver_installed(driver, retry, result),
            AppEvent::DriverUpdates(result) => self.on_driver_updates(result),
            AppEvent::ExplorerLoaded { key, result } => self.on_explorer_loaded(&key, result),
            AppEvent::Statement {
                tab,
                request,
                report,
            } => self.on_statement_report(tab, request, report),
            AppEvent::ScriptFinished {
                tab,
                request,
                outcome,
            } => self.on_script_finished(tab, request, outcome),
            AppEvent::ResultPage {
                tab,
                index,
                request,
                result,
            } => self.on_result_page(tab, index, request, result),
            AppEvent::TableDetails { tab, result } => self.on_table_details(tab, result),
            AppEvent::TablePage {
                tab,
                request,
                result,
            } => self.on_table_page(tab, request, result),
            AppEvent::RowCount { tab, result } => self.on_row_count(tab, result),
            AppEvent::ChangesApplied { tab, result } => self.on_changes_applied(tab, result),
            AppEvent::CatalogLoaded { source, result } => self.on_catalog_loaded(source, result),
            AppEvent::CatalogColumns {
                source,
                schema,
                table,
                result,
            } => self.on_catalog_columns(source, schema, table, result),
            AppEvent::Definition { title, result } => self.on_definition(title, result),
            AppEvent::KeysScanned {
                tab,
                request,
                result,
            } => self.on_keys_scanned(tab, request, result),
            AppEvent::KeyLoaded {
                tab,
                request,
                key,
                description,
                result,
            } => {
                self.on_key_loaded(tab, request, key, description, result);
            }
            AppEvent::KeyWritten {
                tab,
                result,
                rescan,
            } => self.on_key_written(tab, result, rescan),
            AppEvent::CommandReplied { tab, result } => self.on_command_replied(tab, result),
            AppEvent::ServerDetails { tab, result } => self.on_server_details(tab, result),
            AppEvent::DocumentsLoaded {
                tab,
                request,
                result,
            } => self.on_documents_loaded(tab, request, result),
            AppEvent::DocumentWritten { tab, result } => self.on_document_written(tab, result),
            AppEvent::CollectionDescribed {
                tab,
                choose_index_to_drop,
                result,
            } => self.on_collection_described(tab, choose_index_to_drop, result),
        }
    }

    pub(crate) fn load_catalog(&mut self, source: DataSourceId) {
        let Some(capabilities) = self
            .capabilities(source)
            .filter(|capabilities| capabilities.is_tabular())
        else {
            return;
        };
        let Some(connection) = self.connection(source) else {
            return;
        };
        let dialect: Dialect = self.dialect(source);
        let database = self
            .data_source(source)
            .and_then(|definition| definition.settings.database.clone());
        self.spawn(async move {
            let result =
                loaders::load_catalog(connection.as_ref(), capabilities, dialect, database).await;
            AppEvent::CatalogLoaded { source, result }
        });
    }

    fn on_catalog_loaded(
        &mut self,
        source: DataSourceId,
        result: Result<CompletionCatalog, DriverError>,
    ) {
        match result {
            Ok(catalog) => {
                self.catalogs.insert(source, catalog);
                self.catalog_requests
                    .retain(|(requested, _, _)| *requested != source);
            }
            Err(error) => tracing::warn!(%error, "completion metadata could not be loaded"),
        }
    }

    /// Requests a table's columns for completion once; results update the catalog and the
    /// open completion popup.
    pub(crate) fn request_catalog_columns(
        &mut self,
        source: DataSourceId,
        schema: Option<String>,
        table: String,
    ) {
        let request = (source, schema.clone(), table.clone());
        if self.catalog_requests.contains(&request) {
            return;
        }
        let Some(connection) = self.connection(source) else {
            return;
        };
        let reference = self
            .catalogs
            .get(&source)
            .and_then(|catalog| catalog.find_table(schema.as_deref(), &table))
            .map(|found| relix_core::TableRef::new(None, found.schema.clone(), found.name.clone()));
        let Some(mut reference) = reference else {
            return;
        };
        let capabilities = self.capabilities(source);
        if capabilities.is_some_and(|capabilities| !capabilities.schemas) {
            reference.database = reference.schema.take();
        } else if let Some(database) = self
            .data_source(source)
            .and_then(|definition| definition.settings.database.clone())
        {
            reference.database = Some(database);
        }
        self.catalog_requests.push(request);
        self.spawn(async move {
            let result = match loaders::sql(connection.as_ref()) {
                Ok(sql) => sql
                    .table_details(&reference)
                    .await
                    .map(|details| loaders::catalog_columns(&details)),
                Err(error) => Err(error),
            };
            AppEvent::CatalogColumns {
                source,
                schema,
                table,
                result,
            }
        });
    }

    fn on_catalog_columns(
        &mut self,
        source: DataSourceId,
        schema: Option<String>,
        table: String,
        result: Result<Vec<CatalogColumn>, DriverError>,
    ) {
        let Ok(columns) = result else {
            return;
        };
        if let Some(catalog) = self.catalogs.get_mut(&source) {
            let resolved_schema = catalog
                .find_table(schema.as_deref(), &table)
                .and_then(|found| found.schema.clone());
            catalog.set_columns(resolved_schema.as_deref(), &table, columns);
        }
        let waiting = matches!(
            self.active_tab(),
            Some(Tab::Editor(editor)) if editor.data_source == Some(source) && !editor.buffer.has_selection()
        );
        if waiting {
            self.update_completion(false);
        }
    }

    fn on_definition(&mut self, title: String, result: Result<Option<String>, DriverError>) {
        match result {
            Ok(Some(text)) => self
                .overlays
                .push(Overlay::Viewer(ViewerState::new(&title, &text))),
            Ok(None) => self.notify(Level::Info, "no definition available for this object"),
            Err(error) => self.notify(Level::Error, error.to_string()),
        }
    }
}
