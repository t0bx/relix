/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod cancel_signal;
pub mod capabilities;
mod change_checks;
pub mod connection;
pub mod document;
pub mod document_rows;
pub mod driver;
pub mod error;
pub mod ipc;
pub mod key_value;
pub mod sql;
pub mod sql_builder;
mod sql_dialect_rules;
pub mod timeout;

pub use async_trait::async_trait;
pub use cancel_signal::CancelSignal;
pub use capabilities::{DatabaseCapability, DriverCapabilities};
pub use change_checks::ensure_single_row;
pub use connection::{DatabaseConnection, ServerInfo};
pub use document::DocumentConnection;
pub use document_rows::{document_fields, document_rows, json_cell};
pub use driver::{
    ConnectionProperty, ConnectionTarget, DRIVER_API_VERSION, DatabaseDriver, DependencyStatus,
    DriverMetadata, DriverOrigin,
};
pub use error::{DriverError, ErrorKind};
pub use key_value::KeyValueConnection;
pub use sql::{ExecuteOptions, QueryExecutor, SqlConnection, SqlIntrospector, TableDataAccess};
pub use sql_builder::{BuiltStatement, ParamStyle, SqlBuilder};
pub use timeout::with_timeout;

pub type DriverResult<T> = Result<T, DriverError>;
