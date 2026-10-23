/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::DriverError;
use async_trait::async_trait;
use relix_core::keyvalue::{
    CommandReply, KeyDescription, KeyName, KeyScanPage, KeyType, KeyValueWrite, ValuePage,
    ValueRange,
};

#[async_trait]
pub trait KeyValueConnection: Send + Sync {
    async fn scan_keys(
        &self,
        cursor: u64,
        pattern: &str,
        count: u32,
    ) -> Result<KeyScanPage, DriverError>;

    async fn describe_key(&self, key: &KeyName) -> Result<KeyDescription, DriverError>;

    async fn read_value(
        &self,
        key: &KeyName,
        key_type: &KeyType,
        range: ValueRange,
    ) -> Result<ValuePage, DriverError>;

    async fn write(&self, operation: &KeyValueWrite) -> Result<(), DriverError>;

    async fn execute_command(&self, arguments: &[String]) -> Result<CommandReply, DriverError>;

    async fn server_details(&self) -> Result<Vec<(String, String)>, DriverError>;
}
