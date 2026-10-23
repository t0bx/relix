/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::DriverError;
use async_trait::async_trait;
use relix_core::{
    ChangeReport, ChangeSet, DatabaseInfo, ObjectInfo, RowBatch, StatementOutcome, TableDetails,
    TableQuery, TableRef,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecuteOptions {
    pub row_limit: usize,
}

impl Default for ExecuteOptions {
    fn default() -> Self {
        Self { row_limit: 1000 }
    }
}

#[async_trait]
pub trait SqlIntrospector: Send + Sync {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError>;

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError>;

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError>;

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError>;

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError>;
}

#[async_trait]
pub trait SqlExecutor: Send + Sync {
    /// Executes exactly one statement on the session connection. Session state such as an
    /// open transaction started with `BEGIN` is preserved between calls.
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError>;

    async fn cancel(&self) -> Result<(), DriverError>;
}

#[async_trait]
pub trait TableDataAccess: Send + Sync {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError>;

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError>;

    /// Applies all changes inside one transaction. Every update and delete must affect
    /// exactly one row, otherwise the transaction is rolled back.
    async fn apply_changes(&self, changes: &ChangeSet) -> Result<ChangeReport, DriverError>;
}

pub trait SqlConnection: SqlIntrospector + SqlExecutor + TableDataAccess {}

impl<T: SqlIntrospector + SqlExecutor + TableDataAccess> SqlConnection for T {}
