/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod capabilities;
pub mod connection;
pub mod driver;
pub mod error;
pub mod ipc;
pub mod key_value;
pub mod sql;
pub mod sql_builder;
pub mod timeout;

pub use async_trait::async_trait;
pub use capabilities::DriverCapabilities;
pub use connection::{DatabaseConnection, ServerInfo};
pub use driver::{
    ConnectionTarget, DRIVER_API_VERSION, DatabaseDriver, DriverMetadata, DriverOrigin,
};
pub use error::{DriverError, ErrorKind};
pub use key_value::KeyValueConnection;
pub use sql::{ExecuteOptions, SqlConnection, SqlExecutor, SqlIntrospector, TableDataAccess};
pub use sql_builder::{BuiltStatement, ParamStyle, SqlBuilder};
pub use timeout::with_timeout;

pub type DriverResult<T> = Result<T, DriverError>;
