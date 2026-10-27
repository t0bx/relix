/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::DataModel;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DatabaseCapability {
    SqlExecution,
    QueryExecution,
    Transactions,
    MultipleDatabases,
    Schemas,
    Collections,
    KeyValueOperations,
    DocumentOperations,
    GraphQueries,
    TimeSeriesQueries,
    TableEditing,
    IndexManagement,
    QueryCancellation,
    StreamingResults,
    Routines,
    Tls,
    ReadOnlySessions,
}

impl DatabaseCapability {
    pub fn label(self) -> &'static str {
        match self {
            Self::SqlExecution => "SQL execution",
            Self::QueryExecution => "query console",
            Self::Transactions => "transactions",
            Self::MultipleDatabases => "multiple databases",
            Self::Schemas => "schemas",
            Self::Collections => "collections",
            Self::KeyValueOperations => "key-value operations",
            Self::DocumentOperations => "document operations",
            Self::GraphQueries => "graph queries",
            Self::TimeSeriesQueries => "time-series queries",
            Self::TableEditing => "table editing",
            Self::IndexManagement => "index management",
            Self::QueryCancellation => "query cancellation",
            Self::StreamingResults => "streaming results",
            Self::Routines => "routines",
            Self::Tls => "TLS",
            Self::ReadOnlySessions => "database-enforced read-only sessions",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverCapabilities {
    pub data_model: DataModel,
    pub multiple_databases: bool,
    pub schemas: bool,
    pub transactions: bool,
    pub cancellation: bool,
    pub table_editing: bool,
    pub routines: bool,
    pub tls: bool,
    #[serde(default = "default_true")]
    pub query_execution: bool,
    #[serde(default)]
    pub index_management: bool,
    #[serde(default)]
    pub streaming_results: bool,
    #[serde(default)]
    pub read_only_sessions: bool,
}

impl DriverCapabilities {
    pub fn relational() -> Self {
        Self {
            data_model: DataModel::Relational,
            multiple_databases: false,
            schemas: false,
            transactions: true,
            cancellation: false,
            table_editing: true,
            routines: false,
            tls: false,
            query_execution: true,
            index_management: false,
            streaming_results: false,
            read_only_sessions: false,
        }
    }

    pub fn key_value() -> Self {
        Self {
            data_model: DataModel::KeyValue,
            transactions: false,
            table_editing: false,
            query_execution: false,
            ..Self::relational()
        }
    }

    pub fn of_model(data_model: DataModel) -> Self {
        Self {
            data_model,
            transactions: false,
            table_editing: false,
            ..Self::relational()
        }
    }

    pub fn is_relational(&self) -> bool {
        self.data_model == DataModel::Relational
    }

    pub fn is_tabular(&self) -> bool {
        matches!(
            self.data_model,
            DataModel::Relational
                | DataModel::WideColumn
                | DataModel::TimeSeries
                | DataModel::Graph
        )
    }

    pub fn is_key_value(&self) -> bool {
        self.data_model == DataModel::KeyValue
    }

    pub fn is_document(&self) -> bool {
        matches!(self.data_model, DataModel::Document | DataModel::Search)
    }

    pub fn supports(&self, capability: DatabaseCapability) -> bool {
        match capability {
            DatabaseCapability::SqlExecution => {
                self.query_execution
                    && matches!(
                        self.data_model,
                        DataModel::Relational | DataModel::WideColumn | DataModel::TimeSeries
                    )
            }
            DatabaseCapability::QueryExecution => self.query_execution,
            DatabaseCapability::Transactions => self.transactions,
            DatabaseCapability::MultipleDatabases => self.multiple_databases,
            DatabaseCapability::Schemas => self.schemas,
            DatabaseCapability::Collections => self.is_document(),
            DatabaseCapability::KeyValueOperations => self.data_model == DataModel::KeyValue,
            DatabaseCapability::DocumentOperations => self.is_document(),
            DatabaseCapability::GraphQueries => self.data_model == DataModel::Graph,
            DatabaseCapability::TimeSeriesQueries => self.data_model == DataModel::TimeSeries,
            DatabaseCapability::TableEditing => self.table_editing,
            DatabaseCapability::IndexManagement => self.index_management,
            DatabaseCapability::QueryCancellation => self.cancellation,
            DatabaseCapability::StreamingResults => self.streaming_results,
            DatabaseCapability::Routines => self.routines,
            DatabaseCapability::Tls => self.tls,
            DatabaseCapability::ReadOnlySessions => self.read_only_sessions,
        }
    }

    pub fn list(&self) -> Vec<DatabaseCapability> {
        ALL_CAPABILITIES
            .iter()
            .copied()
            .filter(|capability| self.supports(*capability))
            .collect()
    }
}

const ALL_CAPABILITIES: [DatabaseCapability; 17] = [
    DatabaseCapability::SqlExecution,
    DatabaseCapability::QueryExecution,
    DatabaseCapability::Transactions,
    DatabaseCapability::MultipleDatabases,
    DatabaseCapability::Schemas,
    DatabaseCapability::Collections,
    DatabaseCapability::KeyValueOperations,
    DatabaseCapability::DocumentOperations,
    DatabaseCapability::GraphQueries,
    DatabaseCapability::TimeSeriesQueries,
    DatabaseCapability::TableEditing,
    DatabaseCapability::IndexManagement,
    DatabaseCapability::QueryCancellation,
    DatabaseCapability::StreamingResults,
    DatabaseCapability::Routines,
    DatabaseCapability::Tls,
    DatabaseCapability::ReadOnlySessions,
];

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshakes_from_version_one_connectors_still_parse() {
        let legacy = r#"{"data_model":"relational","multiple_databases":false,"schemas":false,
            "transactions":true,"cancellation":false,"table_editing":true,"routines":false,"tls":false}"#;
        let capabilities: DriverCapabilities = serde_json::from_str(legacy).unwrap();
        assert!(capabilities.query_execution);
        assert!(!capabilities.index_management);
        assert!(!capabilities.read_only_sessions);
    }

    #[test]
    fn capabilities_follow_the_data_model() {
        let graph = DriverCapabilities::of_model(DataModel::Graph);
        assert!(graph.supports(DatabaseCapability::GraphQueries));
        assert!(!graph.supports(DatabaseCapability::SqlExecution));
        assert!(!graph.supports(DatabaseCapability::TableEditing));
        let documents = DriverCapabilities {
            index_management: true,
            ..DriverCapabilities::of_model(DataModel::Document)
        };
        assert!(
            documents
                .list()
                .contains(&DatabaseCapability::IndexManagement)
        );
        assert!(DriverCapabilities::key_value().supports(DatabaseCapability::KeyValueOperations));
    }
}
