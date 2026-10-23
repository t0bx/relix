/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod change;
pub mod datasource;
pub mod dialect;
pub mod ids;
pub mod keyvalue;
pub mod metadata;
pub mod result;
pub mod table_query;
pub mod value;

pub use change::{CellChange, ChangeReport, ChangeSet, RowChange};
pub use datasource::{
    ConnectionConfig, ConnectionSettings, DataSourceDefinition, Environment, Secret, TlsMode,
    TlsSettings,
};
pub use dialect::{DataModel, Dialect};
pub use ids::{DataSourceId, DriverId, ProjectId};
pub use metadata::{
    ColumnInfo, DatabaseInfo, ForeignKeyInfo, IndexInfo, ObjectInfo, ObjectKind, TableDetails,
    TableRef,
};
pub use result::{ColumnMeta, RowBatch, StatementOutcome};
pub use table_query::{SortDirection, SortKey, TableQuery};
pub use value::Value;
