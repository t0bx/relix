/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::blocking::run_blocking;
use crate::connect_target::connect_target;
use crate::connector::OracleConnector;
use crate::data_connections::DataConnections;
use crate::errors::map_error;
use crate::session::Session;
use async_trait::async_trait;
use oracle::Connection;
use relix_core::{ConnectionConfig, Dialect, TableRef};
use relix_driver_api::{
    DatabaseConnection, DriverError, ParamStyle, ServerInfo, SqlBuilder, SqlConnection,
};
use std::sync::Arc;

const PRODUCT_NAME: &str = "Oracle Database";
const CURRENT_SCHEMA_SQL: &str = "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM dual";

pub struct OracleConnection {
    pub(crate) server_info: ServerInfo,
    pub(crate) session: Session,
    pub(crate) data: DataConnections,
    pub(crate) service_name: String,
    pub(crate) default_schema: String,
}

impl OracleConnection {
    pub(crate) async fn open(config: &ConnectionConfig) -> Result<Self, DriverError> {
        let connector = Arc::new(OracleConnector::new(connect_target(config)?));
        let opener = Arc::clone(&connector);
        let (connection, version, default_schema) = run_blocking(move || {
            let connection = opener.open(&[])?;
            let (version, _) = connection.server_version().map_err(map_error)?;
            let schema = current_schema(&connection)?;
            Ok((connection, version.to_string(), schema))
        })
        .await?;
        Ok(Self {
            server_info: ServerInfo {
                product: PRODUCT_NAME.to_owned(),
                version,
            },
            session: Session::new(connection),
            service_name: connector.service_name().to_owned(),
            data: DataConnections::new(connector),
            default_schema,
        })
    }

    pub(crate) fn resolve_schema(&self, schema: Option<&str>) -> String {
        schema
            .filter(|schema| !schema.is_empty())
            .unwrap_or(&self.default_schema)
            .to_owned()
    }

    pub(crate) fn resolve_table(&self, table: &TableRef) -> TableRef {
        TableRef::new(
            Some(self.service_name.clone()),
            Some(self.resolve_schema(table.schema.as_deref())),
            table.name.clone(),
        )
    }
}

pub(crate) fn sql_builder() -> SqlBuilder {
    SqlBuilder::new(Dialect::Oracle, ParamStyle::ColonNumbered)
}

fn current_schema(connection: &Connection) -> Result<String, DriverError> {
    connection
        .query_row_as::<String>(CURRENT_SCHEMA_SQL, &[])
        .map_err(map_error)
}

#[async_trait]
impl DatabaseConnection for OracleConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.data
            .run(|connection| connection.ping().map_err(map_error))
            .await
    }

    async fn close(&self) -> Result<(), DriverError> {
        let closed = self.session.close().await;
        self.data.close().await?;
        closed
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
