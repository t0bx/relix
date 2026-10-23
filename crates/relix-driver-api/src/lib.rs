/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod capabilities;
pub mod connection;
pub mod document;
pub mod driver;
pub mod error;
pub mod ipc;
pub mod key_value;
pub mod sql;
pub mod sql_builder;
mod sql_dialect_rules;
pub mod timeout;

pub use async_trait::async_trait;
pub use capabilities::{DatabaseCapability, DriverCapabilities};
pub use connection::{DatabaseConnection, ServerInfo};
pub use document::DocumentConnection;
pub use driver::{
    ConnectionTarget, DRIVER_API_VERSION, DatabaseDriver, DriverMetadata, DriverOrigin,
};
pub use error::{DriverError, ErrorKind};
pub use key_value::KeyValueConnection;
pub use sql::{ExecuteOptions, QueryExecutor, SqlConnection, SqlIntrospector, TableDataAccess};
pub use sql_builder::{BuiltStatement, ParamStyle, SqlBuilder};
pub use timeout::with_timeout;

pub type DriverResult<T> = Result<T, DriverError>;
