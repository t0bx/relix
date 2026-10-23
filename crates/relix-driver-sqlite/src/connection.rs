/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use crate::shared_connection::SharedConnection;
use async_trait::async_trait;
use relix_driver_api::{DatabaseConnection, DriverError, ServerInfo, SqlConnection};

pub(crate) struct SqliteConnection {
    pub(crate) shared: SharedConnection,
}

impl SqliteConnection {
    pub(crate) fn new(connection: rusqlite::Connection) -> Self {
        Self {
            shared: SharedConnection::new(connection),
        }
    }
}

#[async_trait]
impl DatabaseConnection for SqliteConnection {
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            product: "SQLite".to_owned(),
            version: rusqlite::version().to_owned(),
        }
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.shared
            .run(|connection| {
                connection
                    .query_row("SELECT 1", [], |_| Ok(()))
                    .map_err(map_error)
            })
            .await
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.shared.close().await
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}
