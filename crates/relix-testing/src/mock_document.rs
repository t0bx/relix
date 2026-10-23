/*
 * @author t0bx
 * @since 23.10.2026
 */

use async_trait::async_trait;
use relix_core::{
    CollectionDetails, CollectionInfo, CollectionKind, CollectionRef, ConnectionConfig, DataModel,
    DatabaseInfo, Document, DocumentIndex, DocumentPage, DocumentQuery, DocumentWrite,
    DocumentWriteOutcome,
};
use relix_driver_api::{
    ConnectionTarget, DatabaseConnection, DatabaseDriver, DocumentConnection, DriverCapabilities,
    DriverError, DriverMetadata, ServerInfo,
};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

type Collections = Arc<Mutex<BTreeMap<String, Vec<Json>>>>;

/// An in-memory document store for UI and workflow tests. Filters match top-level fields
/// by equality and page tokens are offsets; it is never registered in the application.
#[derive(Debug, Clone, Default)]
pub struct MockDocumentDriver {
    collections: Collections,
}

impl MockDocumentDriver {
    pub fn with_collection(name: &str, documents: Vec<Json>) -> Self {
        let driver = Self::default();
        driver.lock().insert(name.to_owned(), documents);
        driver
    }

    pub fn documents(&self, name: &str) -> Vec<Json> {
        self.lock().get(name).cloned().unwrap_or_default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, Vec<Json>>> {
        self.collections
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

#[async_trait]
impl DatabaseDriver for MockDocumentDriver {
    fn metadata(&self) -> DriverMetadata {
        DriverMetadata::built_in(
            "mock-doc",
            "Mock Documents",
            Some(relix_core::Dialect::MongoCommand),
            ConnectionTarget::Network { default_port: 2 },
        )
    }

    fn capabilities(&self) -> DriverCapabilities {
        DriverCapabilities {
            query_execution: false,
            index_management: true,
            ..DriverCapabilities::of_model(DataModel::Document)
        }
    }

    async fn connect(
        &self,
        _config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        Ok(Box::new(self.clone()))
    }
}

#[async_trait]
impl DatabaseConnection for MockDocumentDriver {
    fn server_info(&self) -> ServerInfo {
        ServerInfo {
            product: "Mock Documents".into(),
            version: "1".into(),
        }
    }

    async fn ping(&self) -> Result<(), DriverError> {
        Ok(())
    }

    async fn close(&self) -> Result<(), DriverError> {
        Ok(())
    }

    fn as_document(&self) -> Option<&dyn DocumentConnection> {
        Some(self)
    }
}

#[async_trait]
impl DocumentConnection for MockDocumentDriver {
    async fn namespaces(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        Ok(vec![DatabaseInfo {
            name: "app".into(),
            is_default: true,
        }])
    }

    async fn collections(&self, namespace: &str) -> Result<Vec<CollectionInfo>, DriverError> {
        Ok(self
            .lock()
            .iter()
            .map(|(name, documents)| CollectionInfo {
                collection: CollectionRef::new(namespace, name.clone()),
                kind: CollectionKind::Collection,
                document_count: Some(documents.len() as u64),
                detail: None,
            })
            .collect())
    }

    async fn describe_collection(
        &self,
        collection: &CollectionRef,
    ) -> Result<CollectionDetails, DriverError> {
        Ok(CollectionDetails {
            collection: collection.clone(),
            kind: CollectionKind::Collection,
            key_fields: vec!["_id".into()],
            indexes: vec![DocumentIndex {
                name: "_id_".into(),
                definition: json!({"_id": 1}),
                unique: true,
                detail: None,
            }],
            schema: None,
            properties: vec![(
                "documents".into(),
                self.documents(&collection.name).len().to_string(),
            )],
        })
    }

    async fn find(
        &self,
        collection: &CollectionRef,
        query: &DocumentQuery,
    ) -> Result<DocumentPage, DriverError> {
        let matching: Vec<Json> = self
            .documents(&collection.name)
            .into_iter()
            .filter(|document| matches_filter(document, query.filter.as_ref()))
            .collect();
        let offset: usize = query
            .page_token
            .as_deref()
            .and_then(|token| token.parse().ok())
            .unwrap_or(0);
        let limit = query.limit.max(1) as usize;
        let documents = matching
            .iter()
            .skip(offset)
            .take(limit)
            .map(|body| Document {
                key: body.get("_id").cloned().unwrap_or(Json::Null),
                body: body.clone(),
            })
            .collect();
        let next = offset + limit;
        Ok(DocumentPage {
            documents,
            next_page: (next < matching.len()).then(|| next.to_string()),
            total: Some(matching.len() as u64),
        })
    }

    async fn write(
        &self,
        collection: &CollectionRef,
        write: &DocumentWrite,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let mut collections = self.lock();
        let documents = collections.entry(collection.name.clone()).or_default();
        let affected = match write {
            DocumentWrite::Insert { document } => {
                let mut document = document.clone();
                if document.get("_id").is_none()
                    && let Some(object) = document.as_object_mut()
                {
                    object.insert("_id".into(), json!(documents.len() + 1000));
                }
                documents.push(document);
                1
            }
            DocumentWrite::Replace { key, document } => {
                let found = documents
                    .iter_mut()
                    .find(|existing| existing.get("_id") == Some(key));
                match found {
                    Some(existing) => {
                        *existing = document.clone();
                        1
                    }
                    None => 0,
                }
            }
            DocumentWrite::Delete { key } => {
                let before = documents.len();
                documents.retain(|existing| existing.get("_id") != Some(key));
                (before - documents.len()) as u64
            }
            DocumentWrite::CreateIndex { .. } | DocumentWrite::DropIndex { .. } => {
                return Err(DriverError::unsupported(
                    "the mock store has no secondary indexes",
                ));
            }
        };
        Ok(DocumentWriteOutcome {
            affected,
            key: None,
            message: None,
        })
    }

    fn filter_hint(&self) -> &'static str {
        "{\"field\": value}"
    }
}

fn matches_filter(document: &Json, filter: Option<&Json>) -> bool {
    let Some(conditions) = filter.and_then(Json::as_object) else {
        return true;
    };
    conditions
        .iter()
        .all(|(field, expected)| document.get(field) == Some(expected))
}
