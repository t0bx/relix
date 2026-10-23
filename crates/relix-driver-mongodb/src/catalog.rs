/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::command_reply::bson_number;
use crate::connect::ADMIN_DATABASE;
use crate::connection::MongoConnection;
use crate::errors::map_error;
use futures_util::{StreamExt, TryStreamExt, stream};
use mongodb::bson::{Document, doc};
use relix_core::{CollectionInfo, CollectionKind, CollectionRef, DatabaseInfo};
use relix_driver_api::{DriverError, ErrorKind};

const CONCURRENT_COUNTS: usize = 8;

impl MongoConnection {
    /// Lists the databases the user may see. `authorizedDatabases` keeps users without
    /// the cluster-wide `listDatabases` privilege working; if even that is refused, the
    /// configured database is the only namespace.
    pub(crate) async fn list_namespaces(&self) -> Result<Vec<DatabaseInfo>, DriverError> {
        let reply = self
            .database(ADMIN_DATABASE)?
            .run_command(doc! { "listDatabases": 1, "authorizedDatabases": true })
            .await
            .map_err(map_error);
        let reply = match reply {
            Ok(reply) => reply,
            Err(error) if error.kind == ErrorKind::Query => Document::new(),
            Err(error) => return Err(error),
        };
        let mut names: Vec<String> = reply
            .get_array("databases")
            .map(Vec::as_slice)
            .unwrap_or(&[])
            .iter()
            .filter_map(|entry| entry.as_document()?.get_str("name").ok())
            .map(str::to_owned)
            .collect();
        if !names.contains(&self.default_database) {
            names.push(self.default_database.clone());
        }
        names.sort();
        Ok(names
            .into_iter()
            .map(|name| DatabaseInfo {
                is_default: name == self.default_database,
                name,
            })
            .collect())
    }

    pub(crate) async fn list_collections(
        &self,
        namespace: &str,
    ) -> Result<Vec<CollectionInfo>, DriverError> {
        let database = self.database(namespace)?;
        let specifications: Vec<Document> = database
            .run_cursor_command(doc! { "listCollections": 1, "authorizedCollections": true })
            .await
            .map_err(map_error)?
            .try_collect()
            .await
            .map_err(map_error)?;
        let mut collections: Vec<CollectionInfo> = stream::iter(specifications)
            .map(|specification| self.describe_entry(namespace, specification))
            .buffer_unordered(CONCURRENT_COUNTS)
            .collect()
            .await;
        collections.sort_by(|a, b| a.collection.name.cmp(&b.collection.name));
        Ok(collections)
    }

    async fn describe_entry(&self, namespace: &str, specification: Document) -> CollectionInfo {
        let name = specification.get_str("name").unwrap_or_default().to_owned();
        let kind = collection_kind(&specification);
        let collection = CollectionRef::new(namespace, name);
        let document_count = match kind {
            CollectionKind::Collection => self.estimated_count(&collection).await,
            _ => None,
        };
        CollectionInfo {
            collection,
            kind,
            document_count,
            detail: collection_detail(&specification),
        }
    }

    pub(crate) async fn estimated_count(&self, collection: &CollectionRef) -> Option<u64> {
        self.collection(collection)
            .ok()?
            .estimated_document_count()
            .await
            .ok()
    }
}

pub fn collection_kind(specification: &Document) -> CollectionKind {
    let options = collection_options(specification);
    match specification.get_str("type").unwrap_or("collection") {
        "view" => CollectionKind::View,
        "timeseries" => CollectionKind::TimeSeries,
        _ if options.contains_key("timeseries") => CollectionKind::TimeSeries,
        _ => CollectionKind::Collection,
    }
}

pub fn collection_options(specification: &Document) -> Document {
    specification
        .get_document("options")
        .cloned()
        .unwrap_or_default()
}

fn collection_detail(specification: &Document) -> Option<String> {
    let options = collection_options(specification);
    if let Ok(source) = options.get_str("viewOn") {
        return Some(format!("view on {source}"));
    }
    if let Ok(series) = options.get_document("timeseries") {
        let field = series.get_str("timeField").unwrap_or("?");
        return Some(format!("time field {field}"));
    }
    if options.get_bool("capped").unwrap_or(false) {
        let size = options.get("size").and_then(bson_number).unwrap_or(0);
        return Some(format!("capped, {size} bytes"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collection_types_are_recognised() {
        assert_eq!(
            collection_kind(&doc! { "name": "v", "type": "view", "options": { "viewOn": "a" } }),
            CollectionKind::View
        );
        assert_eq!(
            collection_kind(&doc! { "name": "t", "type": "timeseries" }),
            CollectionKind::TimeSeries
        );
        assert_eq!(
            collection_kind(&doc! { "name": "c", "type": "collection" }),
            CollectionKind::Collection
        );
    }

    #[test]
    fn details_describe_views_series_and_capped_collections() {
        let view = doc! { "options": { "viewOn": "orders", "pipeline": [] } };
        assert_eq!(collection_detail(&view).as_deref(), Some("view on orders"));
        let series = doc! { "options": { "timeseries": { "timeField": "ts" } } };
        assert_eq!(collection_detail(&series).as_deref(), Some("time field ts"));
        let capped = doc! { "options": { "capped": true, "size": 4096 } };
        assert_eq!(
            collection_detail(&capped).as_deref(),
            Some("capped, 4096 bytes")
        );
        assert_eq!(collection_detail(&doc! {}), None);
    }
}
