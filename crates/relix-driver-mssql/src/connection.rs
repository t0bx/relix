/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog_rows::run_batch;
use crate::connector::Connector;
use crate::data_clients::DataClients;
use crate::server_details::server_details;
use crate::session::Session;
use async_trait::async_trait;
use relix_core::{ConnectionConfig, Dialect, TableRef};
use relix_driver_api::{
    DatabaseConnection, DriverError, ParamStyle, QueryExecutor, ServerInfo, SqlBuilder,
    SqlConnection,
};
use std::sync::Arc;

pub struct MsSqlConnection {
    pub(crate) server_info: ServerInfo,
    pub(crate) session: Session,
    pub(crate) data: DataClients,
    pub(crate) default_database: String,
    pub(crate) default_schema: String,
}

impl MsSqlConnection {
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let (connector, mut client) = Connector::establish(config).await?;
        let details = server_details(&mut client).await?;
        let connector = Arc::new(connector);
        Ok(Self {
            server_info: details.info,
            session: Session::new(Arc::clone(&connector), client, details.process_id),
            data: DataClients::new(connector),
            default_database: details.database,
            default_schema: details.schema,
        })
    }

    pub(crate) fn resolve_database(&self, database: Option<&str>) -> String {
        database
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.default_database)
            .to_owned()
    }

    pub(crate) fn resolve_table(&self, table: &TableRef) -> TableRef {
        let schema = table
            .schema
            .as_deref()
            .filter(|name| !name.is_empty())
            .unwrap_or(&self.default_schema);
        TableRef::new(
            Some(self.resolve_database(table.database.as_deref())),
            Some(schema.to_owned()),
            table.name.clone(),
        )
    }
}

pub(crate) fn sql_builder() -> SqlBuilder {
    SqlBuilder::new(Dialect::MsSql, ParamStyle::AtNumbered)
}

#[async_trait]
impl DatabaseConnection for MsSqlConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let mut lease = self.data.lease().await?;
        let result = run_batch(lease.client()?, "SELECT 1").await;
        lease.finish(result)
    }

    async fn close(&self) -> Result<(), DriverError> {
        let slot = match self.session.try_lock() {
            Some(slot) => slot,
            None => {
                let _ = self.cancel().await;
                self.session.lock().await
            }
        };
        let closed = self.session.close(slot).await;
        self.data.close().await;
        closed
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
