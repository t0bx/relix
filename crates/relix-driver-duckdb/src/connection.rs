/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::open::OpenedDatabase;
use crate::shared_connection::SharedConnection;
use async_trait::async_trait;
use relix_driver_api::{DatabaseConnection, DriverError, ServerInfo, SqlConnection};

const PRODUCT_NAME: &str = "DuckDB";

pub(crate) struct DuckDbConnection {
    pub(crate) session: SharedConnection,
    pub(crate) data: SharedConnection,
    version: String,
}

impl DuckDbConnection {
    pub(crate) fn new(opened: OpenedDatabase) -> Self {
        Self {
            session: SharedConnection::new(opened.session),
            data: SharedConnection::new(opened.data),
            version: opened.version,
        }
    }
}

#[async_trait]
impl DatabaseConnection for DuckDbConnection {
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            product: PRODUCT_NAME.to_owned(),
            version: self.version.clone(),
        }
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.data
            .run(|connection| {
                connection
                    .query_row("SELECT 1", [], |_| Ok(()))
                    .map_err(map_error)
            })
            .await
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.session.close().await?;
        self.data.close().await
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
