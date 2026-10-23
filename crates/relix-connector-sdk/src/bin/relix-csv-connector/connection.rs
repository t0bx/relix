/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::statement_guard::ensure_read_only;
use async_trait::async_trait;
use relix_core::{
    ChangeReport, ChangeSet, DatabaseInfo, ObjectInfo, RowBatch, StatementOutcome, TableDetails,
    TableQuery, TableRef,
};
use relix_driver_api::{
    DatabaseConnection, DriverError, ExecuteOptions, ServerInfo, SqlConnection, SqlExecutor,
    SqlIntrospector, TableDataAccess,
};

pub(crate) struct CsvConnection {
    inner: Box<dyn DatabaseConnection>,
}

impl CsvConnection {
    pub(crate) fn new(inner: Box<dyn DatabaseConnection>) -> Self {
        Self { inner }
    }

    pub(crate) fn sql(&self) -> Result<&dyn SqlConnection, DriverError> {
        self.inner
            .as_sql()
            .ok_or_else(|| DriverError::internal("the SQLite connection is not relational"))
    }
}

#[async_trait]
impl DatabaseConnection for CsvConnection {
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            product: "CSV Files".to_owned(),
            version: format!(
                "{} (SQLite {})",
                env!("CARGO_PKG_VERSION"),
                self.inner.server_info().version
            ),
        }
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.inner.ping().await
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.inner.close().await
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}

#[async_trait]
impl SqlIntrospector for CsvConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        self.sql()?.databases().await
    }

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError> {
        self.sql()?.schemas(database).await
    }

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        self.sql()?.objects(database, schema).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        self.sql()?.table_details(table).await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        self.sql()?.object_definition(object).await
    }
}

#[async_trait]
impl SqlExecutor for CsvConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        ensure_read_only(statement)?;
        self.sql()?.execute(statement, options).await
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.sql()?.cancel().await
    }
}

#[async_trait]
impl TableDataAccess for CsvConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        self.sql()?.fetch_table_page(table, query).await
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        self.sql()?.count_rows(table, filter).await
    }

    async fn apply_changes(&self, _changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        Err(DriverError::unsupported(
            "CSV data sources are read-only and cannot be edited",
        ))
    }
}
