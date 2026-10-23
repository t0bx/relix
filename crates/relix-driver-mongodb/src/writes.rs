/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::MongoConnection;
use crate::errors::map_error;
use crate::extended_json::{document_from_json, to_editable_json};
use crate::find::key_filter;
use crate::index_spec::index_specification;
use mongodb::bson::doc;
use relix_core::{CollectionRef, DocumentWrite, DocumentWriteOutcome};
use relix_driver_api::{DriverError, ErrorKind};
use serde_json::Value as Json;

impl MongoConnection {
    pub(crate) async fn apply_write(
        &self,
        collection: &CollectionRef,
        write: &DocumentWrite,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        match write {
            DocumentWrite::Insert { document } => self.insert(collection, document).await,
            DocumentWrite::Replace { key, document } => {
                self.replace(collection, key, document).await
            }
            DocumentWrite::Delete { key } => self.delete(collection, key).await,
            DocumentWrite::CreateIndex { definition } => {
                self.create_index(collection, definition).await
            }
            DocumentWrite::DropIndex { name } => self.drop_index(collection, name).await,
        }
    }

    async fn insert(
        &self,
        collection: &CollectionRef,
        document: &Json,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let document = document_from_json(document, "document")?;
        let result = self
            .collection(collection)?
            .insert_one(document)
            .await
            .map_err(map_error)?;
        Ok(DocumentWriteOutcome {
            affected: 1,
            key: Some(to_editable_json(result.inserted_id)),
            message: None,
        })
    }

    async fn replace(
        &self,
        collection: &CollectionRef,
        key: &Json,
        document: &Json,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let replacement = document_from_json(document, "document")?;
        let result = self
            .collection(collection)?
            .replace_one(key_filter(key)?, replacement)
            .await
            .map_err(map_error)?;
        if result.matched_count == 0 {
            return Err(missing_document(key));
        }
        Ok(DocumentWriteOutcome {
            affected: result.modified_count,
            key: Some(key.clone()),
            message: (result.modified_count == 0).then(|| "document unchanged".to_owned()),
        })
    }

    async fn delete(
        &self,
        collection: &CollectionRef,
        key: &Json,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let result = self
            .collection(collection)?
            .delete_one(key_filter(key)?)
            .await
            .map_err(map_error)?;
        if result.deleted_count == 0 {
            return Err(missing_document(key));
        }
        Ok(DocumentWriteOutcome {
            affected: result.deleted_count,
            key: Some(key.clone()),
            message: None,
        })
    }

    async fn create_index(
        &self,
        collection: &CollectionRef,
        definition: &Json,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let specification = index_specification(definition)?;
        let name = specification.get_str("name").unwrap_or_default().to_owned();
        self.database_of(collection)?
            .run_command(doc! {
                "createIndexes": &collection.name,
                "indexes": [specification],
            })
            .await
            .map_err(map_error)?;
        Ok(DocumentWriteOutcome {
            affected: 1,
            key: None,
            message: Some(format!("created index {name}")),
        })
    }

    async fn drop_index(
        &self,
        collection: &CollectionRef,
        name: &str,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        self.database_of(collection)?
            .run_command(doc! { "dropIndexes": &collection.name, "index": name })
            .await
            .map_err(map_error)?;
        Ok(DocumentWriteOutcome {
            affected: 1,
            key: None,
            message: Some(format!("dropped index {name}")),
        })
    }
}

fn missing_document(key: &Json) -> DriverError {
    DriverError::new(
        ErrorKind::ConcurrentModification,
        format!("no document with _id {key} exists any more"),
    )
}
