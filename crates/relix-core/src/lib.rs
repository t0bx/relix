/*
 * @author t0bx
 * @since 23.10.2026
 */

pub mod change;
pub mod datasource;
pub mod dialect;
pub mod document;
pub mod ids;
pub mod keyvalue;
pub mod metadata;
pub mod network;
pub mod result;
pub mod servers;
pub mod table_query;
pub mod value;

pub use change::{CellChange, ChangeReport, ChangeSet, RowChange};
pub use datasource::{
    ConnectionConfig, ConnectionSettings, DataSourceDefinition, Environment, Secret, TlsMode,
    TlsSettings,
};
pub use dialect::{DataModel, Dialect, StringEscape};
pub use document::{
    CollectionDetails, CollectionInfo, CollectionKind, CollectionRef, Document, DocumentIndex,
    DocumentPage, DocumentQuery, DocumentWrite, DocumentWriteOutcome,
};
pub use ids::{DataSourceId, DriverId, ProjectId, ServerConfigId};
pub use metadata::{
    ColumnInfo, DatabaseInfo, ForeignKeyInfo, IndexInfo, ObjectInfo, ObjectKind, TableDetails,
    TableRef,
};
pub use network::{NetworkMode, NetworkSettings};
pub use result::{ColumnMeta, RowBatch, StatementOutcome};
pub use servers::{
    ConfigScope, HostKeyPolicy, ReconnectPolicy, SshAuthMethod, SshServerConfig, VpnMode,
    VpnProfile, VpnProvider,
};
pub use table_query::{SortDirection, SortKey, TableQuery};
pub use value::Value;
