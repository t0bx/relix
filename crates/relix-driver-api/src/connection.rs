/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::document::DocumentConnection;
use crate::error::DriverError;
use crate::key_value::KeyValueConnection;
use crate::sql::{QueryExecutor, SqlConnection};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ServerInfo {
    pub product: String,
    pub version: String,
}

#[async_trait]
pub trait DatabaseConnection: Send + Sync {
    fn server_info(&self) -> ServerInfo;

    async fn ping(&self) -> Result<(), DriverError>;

    async fn close(&self) -> Result<(), DriverError>;

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        None
    }

    fn as_query(&self) -> Option<&dyn QueryExecutor> {
        self.as_sql().map(|sql| sql as &dyn QueryExecutor)
    }

    fn as_key_value(&self) -> Option<&dyn KeyValueConnection> {
        None
    }

    fn as_document(&self) -> Option<&dyn DocumentConnection> {
        None
    }

    fn network_route(&self) -> Option<String> {
        None
    }

    fn network_status(&self) -> Option<String> {
        None
    }
}
