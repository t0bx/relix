/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::DriverError;
use async_trait::async_trait;
use relix_core::{
    CollectionDetails, CollectionInfo, CollectionRef, DatabaseInfo, DocumentPage, DocumentQuery,
    DocumentWrite, DocumentWriteOutcome,
};

#[async_trait]
pub trait DocumentConnection: Send + Sync {
    async fn namespaces(&self) -> Result<Vec<DatabaseInfo>, DriverError>;

    async fn collections(&self, namespace: &str) -> Result<Vec<CollectionInfo>, DriverError>;

    async fn describe_collection(
        &self,
        collection: &CollectionRef,
    ) -> Result<CollectionDetails, DriverError>;

    async fn find(
        &self,
        collection: &CollectionRef,
        query: &DocumentQuery,
    ) -> Result<DocumentPage, DriverError>;

    async fn write(
        &self,
        collection: &CollectionRef,
        write: &DocumentWrite,
    ) -> Result<DocumentWriteOutcome, DriverError>;

    fn filter_hint(&self) -> &'static str;
}
