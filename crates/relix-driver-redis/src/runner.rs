/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::{map_parsing_error, map_redis_error};
use redis::aio::{ConnectionLike, ConnectionManager};
use redis::{Cmd, FromRedisValue, Pipeline, Value};
use relix_driver_api::{DriverError, ErrorKind, with_timeout};
use std::sync::Mutex;
use std::time::Duration;

pub const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

/// Sends commands over a shared, auto-reconnecting multiplexed connection. A failed
/// command is never retried because its effect on the server is unknown; only the
/// following commands benefit from the manager's reconnect.
pub struct CommandRunner {
    manager: Mutex<Option<ConnectionManager>>,
}

impl CommandRunner {
    pub fn new(manager: ConnectionManager) -> Self {
        Self {
            manager: Mutex::new(Some(manager)),
        }
    }

    pub async fn query<T: FromRedisValue>(&self, command: &Cmd) -> Result<T, DriverError> {
        let value = self.raw(command).await?;
        let value = value.extract_error().map_err(map_redis_error)?;
        redis::from_redis_value(value).map_err(map_parsing_error)
    }

    pub async fn query_pipeline<T: FromRedisValue>(
        &self,
        pipeline: &Pipeline,
    ) -> Result<T, DriverError> {
        let values = self.raw_pipeline(pipeline).await?;
        let value = Value::Array(values)
            .extract_error()
            .map_err(map_redis_error)?;
        redis::from_redis_value(value).map_err(map_parsing_error)
    }

    pub async fn raw(&self, command: &Cmd) -> Result<Value, DriverError> {
        let mut manager = self.manager()?;
        with_timeout(COMMAND_TIMEOUT, async move {
            manager
                .req_packed_command(command)
                .await
                .map_err(map_redis_error)
        })
        .await
    }

    pub async fn raw_pipeline(&self, pipeline: &Pipeline) -> Result<Vec<Value>, DriverError> {
        let mut manager = self.manager()?;
        let count = pipeline.len();
        with_timeout(COMMAND_TIMEOUT, async move {
            manager
                .req_packed_commands(pipeline, 0, count)
                .await
                .map_err(map_redis_error)
        })
        .await
    }

    pub fn close(&self) -> Result<(), DriverError> {
        self.lock()?.take();
        Ok(())
    }

    fn manager(&self) -> Result<ConnectionManager, DriverError> {
        self.lock()?
            .clone()
            .ok_or_else(|| DriverError::connection("the Redis connection has been closed"))
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Option<ConnectionManager>>, DriverError> {
        self.manager
            .lock()
            .map_err(|_| DriverError::new(ErrorKind::Internal, "connection state is poisoned"))
    }
}
