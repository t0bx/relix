/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::DriverError;
use crate::key_value::KeyValueConnection;
use crate::sql::SqlConnection;
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

    fn as_key_value(&self) -> Option<&dyn KeyValueConnection> {
        None
    }
}
