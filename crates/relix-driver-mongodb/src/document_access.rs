/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::MongoConnection;
use async_trait::async_trait;
use relix_core::{
    CollectionDetails, CollectionInfo, CollectionRef, DatabaseInfo, DocumentPage, DocumentQuery,
    DocumentWrite, DocumentWriteOutcome,
};
use relix_driver_api::{DocumentConnection, DriverError};

pub const FILTER_HINT: &str = "MongoDB query in Extended JSON, e.g. {\"age\": {\"$gt\": 30}}";

#[async_trait]
impl DocumentConnection for MongoConnection {
    async fn namespaces(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        self.list_namespaces().await
    }

    async fn collections(&self, namespace: &str) -> Result<Vec<CollectionInfo>, DriverError> {
        self.list_collections(namespace).await
    }

    async fn describe_collection(
        &self,
        collection: &CollectionRef,
    ) -> Result<CollectionDetails, DriverError> {
        self.describe(collection).await
    }

    async fn find(
        &self,
        collection: &CollectionRef,
        query: &DocumentQuery,
    ) -> Result<DocumentPage, DriverError> {
        self.find_page(collection, query).await
    }

    async fn write(
        &self,
        collection: &CollectionRef,
        write: &DocumentWrite,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        self.apply_write(collection, write).await
    }

    fn filter_hint(&self) -> &'static str {
        FILTER_HINT
    }
}
