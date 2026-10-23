/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::client::IpcClient;
use async_trait::async_trait;
use relix_core::{
    ChangeReport, ChangeSet, DatabaseInfo, ObjectInfo, RowBatch, StatementOutcome, TableDetails,
    TableQuery, TableRef,
};
use relix_driver_api::ipc::IpcCall;
use relix_driver_api::{
    DatabaseConnection, DriverError, ExecuteOptions, ServerInfo, SqlConnection, SqlExecutor,
    SqlIntrospector, TableDataAccess,
};
use std::time::Duration;

const CLOSE_GRACE: Duration = Duration::from_secs(2);

pub struct ExternalConnection {
    client: IpcClient,
    server_info: ServerInfo,
}

impl ExternalConnection {
    pub(crate) fn new(client: IpcClient, server_info: ServerInfo) -> Self {
        Self {
            client,
            server_info,
        }
    }
}

#[async_trait]
impl DatabaseConnection for ExternalConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        self.client.call(IpcCall::Ping {}).await
    }

    async fn close(&self) -> Result<(), DriverError> {
        self.client.shutdown(CLOSE_GRACE).await
    }

    fn as_sql(&self) -> Option<&dyn SqlConnection> {
        Some(self)
    }
}

#[async_trait]
impl SqlIntrospector for ExternalConnection {
    async fn databases(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        self.client.call(IpcCall::Databases {}).await
    }

    async fn schemas(&self, database: &str) -> Result<Vec<String>, DriverError> {
        let database = database.to_owned();
        self.client.call(IpcCall::Schemas { database }).await
    }

    async fn objects(
        &self,
        database: &str,
        schema: Option<&str>,
    ) -> Result<Vec<ObjectInfo>, DriverError> {
        let call = IpcCall::Objects {
            database: database.to_owned(),
            schema: schema.map(str::to_owned),
        };
        self.client.call(call).await
    }

    async fn table_details(&self, table: &TableRef) -> Result<TableDetails, DriverError> {
        let table = table.clone();
        self.client.call(IpcCall::TableDetails { table }).await
    }

    async fn object_definition(&self, object: &ObjectInfo) -> Result<Option<String>, DriverError> {
        let object = object.clone();
        self.client.call(IpcCall::ObjectDefinition { object }).await
    }
}

#[async_trait]
impl SqlExecutor for ExternalConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        let call = IpcCall::Execute {
            statement: statement.to_owned(),
            options: *options,
        };
        self.client.call(call).await
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        self.client.call(IpcCall::Cancel {}).await
    }
}

#[async_trait]
impl TableDataAccess for ExternalConnection {
    async fn fetch_table_page(
        &self,
        table: &TableRef,
        query: &TableQuery,
    ) -> Result<RowBatch, DriverError> {
        let call = IpcCall::FetchTablePage {
            table: table.clone(),
            query: query.clone(),
        };
        self.client.call(call).await
    }

    async fn count_rows(&self, table: &TableRef, filter: Option<&str>) -> Result<u64, DriverError> {
        let call = IpcCall::CountRows {
            table: table.clone(),
            filter: filter.map(str::to_owned),
        };
        self.client.call(call).await
    }

    async fn apply_changes(&self, changes: &ChangeSet) -> Result<ChangeReport, DriverError> {
        let changes = changes.clone();
        self.client.call(IpcCall::ApplyChanges { changes }).await
    }
}
