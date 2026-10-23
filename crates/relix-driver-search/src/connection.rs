/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cancel::RunningRequests;
use crate::catalog::collections;
use crate::client::SearchClient;
use crate::cluster::RootInfo;
use crate::console;
use crate::describe::describe_collection;
use crate::find::find;
use crate::index_admin::{create_index, drop_index};
use crate::options::SearchOptions;
use crate::result_window::ResultWindows;
use crate::writes::{delete, insert, replace};
use async_trait::async_trait;
use relix_core::{
    CollectionDetails, CollectionInfo, CollectionRef, DatabaseInfo, DocumentPage, DocumentQuery,
    DocumentWrite, DocumentWriteOutcome, StatementOutcome,
};
use relix_driver_api::{
    DatabaseConnection, DocumentConnection, DriverError, ExecuteOptions, QueryExecutor, ServerInfo,
};
use relix_driver_http::Method;

pub const FILTER_HINT: &str =
    "Query DSL query, e.g. {\"match\": {\"title\": \"rust\"}} — empty matches all documents";

pub struct SearchConnection {
    client: SearchClient,
    root: RootInfo,
    server_info: ServerInfo,
    options: SearchOptions,
    windows: ResultWindows,
    running: RunningRequests,
}

impl SearchConnection {
    pub fn new(
        client: SearchClient,
        root: RootInfo,
        server_info: ServerInfo,
        options: SearchOptions,
    ) -> Self {
        Self {
            client,
            root,
            server_info,
            options,
            windows: ResultWindows::default(),
            running: RunningRequests::default(),
        }
    }
}

#[async_trait]
impl DatabaseConnection for SearchConnection {
    fn server_info(&self) -> ServerInfo {
        self.server_info.clone()
    }

    async fn ping(&self) -> Result<(), DriverError> {
        let response = self
            .client
            .exchange(self.client.request(Method::HEAD, "/")?)
            .await?;
        if response.status.is_success() {
            Ok(())
        } else {
            Err(response.into_error())
        }
    }

    async fn close(&self) -> Result<(), DriverError> {
        let _ = console::cancel(&self.client, &self.running).await;
        self.client.close();
        Ok(())
    }

    fn as_query(&self) -> Option<&dyn QueryExecutor> {
        Some(self)
    }

    fn as_document(&self) -> Option<&dyn DocumentConnection> {
        Some(self)
    }
}

#[async_trait]
impl DocumentConnection for SearchConnection {
    async fn namespaces(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        Ok(vec![DatabaseInfo {
            name: self.root.cluster_name.clone(),
            is_default: true,
        }])
    }

    async fn collections(&self, namespace: &str) -> Result<Vec<CollectionInfo>, DriverError> {
        collections(&self.client, namespace, self.options).await
    }

    async fn describe_collection(
        &self,
        collection: &CollectionRef,
    ) -> Result<CollectionDetails, DriverError> {
        describe_collection(&self.client, &self.root, collection).await
    }

    async fn find(
        &self,
        collection: &CollectionRef,
        query: &DocumentQuery,
    ) -> Result<DocumentPage, DriverError> {
        find(
            &self.client,
            &self.windows,
            self.options.exact_totals,
            collection,
            query,
        )
        .await
    }

    async fn write(
        &self,
        collection: &CollectionRef,
        write: &DocumentWrite,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let name = collection.name.as_str();
        match write {
            DocumentWrite::Insert { document } => insert(&self.client, name, document).await,
            DocumentWrite::Replace { key, document } => {
                replace(&self.client, name, key, document).await
            }
            DocumentWrite::Delete { key } => delete(&self.client, name, key).await,
            DocumentWrite::CreateIndex { definition } => {
                create_index(&self.client, definition).await
            }
            DocumentWrite::DropIndex { name: index } => {
                drop_index(&self.client, collection, index).await
            }
        }
    }

    fn filter_hint(&self) -> &'static str {
        FILTER_HINT
    }
}

#[async_trait]
impl QueryExecutor for SearchConnection {
    async fn execute(
        &self,
        statement: &str,
        options: &ExecuteOptions,
    ) -> Result<StatementOutcome, DriverError> {
        console::execute(&self.client, &self.running, statement, options).await
    }

    async fn cancel(&self) -> Result<(), DriverError> {
        console::cancel(&self.client, &self.running).await
    }
}
