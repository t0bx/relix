/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::describe::describe_key;
use crate::execute::execute_command;
use crate::read_value::read_value;
use crate::runner::CommandRunner;
use crate::scan::scan_keys;
use crate::server_details::server_details;
use crate::writes::apply_write;
use async_trait::async_trait;
use relix_core::keyvalue::{
    CommandReply, KeyDescription, KeyName, KeyScanPage, KeyType, KeyValueWrite, ValuePage,
    ValueRange,
};
use relix_driver_api::{DatabaseConnection, DriverError, KeyValueConnection, ServerInfo};

pub struct RedisConnection {
    runner: CommandRunner,
    server_info: ServerInfo,
}

impl RedisConnection {
    pub fn new(runner: CommandRunner, server_info: ServerInfo) -> Self {
        Self {
            runner,
            server_info,
        }
    }
}

#[async_trait]
impl DatabaseConnection for RedisConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.runner
            .query::<String>(&redis::cmd("PING"))
            .await
            .map(|_| ())
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.runner.close()
    }

    fn as_key_value(&self) -> Option<&dyn KeyValueConnection> {
        Some(self)
    }
}

#[async_trait]
impl KeyValueConnection for RedisConnection {
    async fn scan_keys(
        &self,
        cursor: u64,
        pattern: &str,
        count: u32,
    ) -> Result<KeyScanPage, DriverError> {
        scan_keys(&self.runner, cursor, pattern, count).await
    }

    async fn describe_key(&self, key: &KeyName) -> Result<KeyDescription, DriverError> {
        describe_key(&self.runner, key).await
    }

    async fn read_value(
        &self,
        key: &KeyName,
        key_type: &KeyType,
        range: ValueRange,
    ) -> Result<ValuePage, DriverError> {
        read_value(&self.runner, key, key_type, range).await
    }

    async fn write(&self, operation: &KeyValueWrite) -> Result<(), DriverError> {
        apply_write(&self.runner, operation).await
    }

    async fn execute_command(&self, arguments: &[String]) -> Result<CommandReply, DriverError> {
        execute_command(&self.runner, arguments).await
    }

    async fn server_details(&self) -> Result<Vec<(String, String)>, DriverError> {
        server_details(&self.runner).await
    }
}
