/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::catalog::{collection_kind, collection_options};
use crate::command_reply::bson_number;
use crate::connection::MongoConnection;
use crate::errors::map_error;
use crate::extended_json::{document_to_editable_json, to_editable_json};
use futures_util::TryStreamExt;
use mongodb::bson::{Bson, Document, doc};
use relix_core::{CollectionDetails, CollectionKind, CollectionRef, DocumentIndex};
use relix_driver_api::DriverError;

const KEY_FIELD: &str = "_id";
const STATISTICS: [(&str, &str); 6] = [
    ("count", "documents"),
    ("size", "data size (bytes)"),
    ("avgObjSize", "average document size (bytes)"),
    ("storageSize", "storage size (bytes)"),
    ("totalIndexSize", "index size (bytes)"),
    ("nindexes", "indexes"),
];
const OPTION_PROPERTIES: [&str; 8] = [
    "capped",
    "size",
    "max",
    "viewOn",
    "expireAfterSeconds",
    "validationLevel",
    "validationAction",
    "clusteredIndex",
];

impl MongoConnection {
    pub(crate) async fn describe(
        &self,
        collection: &CollectionRef,
    ) -> Result<CollectionDetails, DriverError> {
        let specification = self.specification(collection).await?;
        let kind = collection_kind(&specification);
        let options = collection_options(&specification);
        let indexes = match kind {
            CollectionKind::View => Vec::new(),
            _ => self.indexes(collection).await?,
        };
        let mut properties = vec![("type".to_owned(), kind.label().to_owned())];
        properties.extend(option_properties(&options));
        if kind != CollectionKind::View {
            properties.extend(self.statistics(collection).await);
        }
        Ok(CollectionDetails {
            collection: collection.clone(),
            kind,
            key_fields: vec![KEY_FIELD.to_owned()],
            indexes,
            schema: validator_schema(&options),
            properties,
        })
    }

    async fn specification(&self, collection: &CollectionRef) -> Result<Document, DriverError> {
        let database = self.database_of(collection)?;
        let mut specifications: Vec<Document> = database
            .run_cursor_command(doc! {
                "listCollections": 1,
                "filter": { "name": &collection.name },
                "authorizedCollections": true,
            })
            .await
            .map_err(map_error)?
            .try_collect()
            .await
            .map_err(map_error)?;
        specifications
            .pop()
            .ok_or_else(|| DriverError::query(format!("collection {collection} does not exist")))
    }

    async fn indexes(&self, collection: &CollectionRef) -> Result<Vec<DocumentIndex>, DriverError> {
        let database = self.database_of(collection)?;
        let specifications: Vec<Document> = database
            .run_cursor_command(doc! { "listIndexes": &collection.name })
            .await
            .map_err(map_error)?
            .try_collect()
            .await
            .map_err(map_error)?;
        Ok(specifications.into_iter().map(index_info).collect())
    }

    async fn statistics(&self, collection: &CollectionRef) -> Vec<(String, String)> {
        let Ok(handle) = self.collection(collection) else {
            return Vec::new();
        };
        let pipeline = [doc! { "$collStats": { "storageStats": {} } }];
        let Ok(cursor) = handle.aggregate(pipeline).await else {
            return Vec::new();
        };
        let Ok(mut reports) = cursor.try_collect::<Vec<Document>>().await else {
            return Vec::new();
        };
        let Some(storage) = reports
            .pop()
            .and_then(|report| report.get_document("storageStats").ok().cloned())
        else {
            return Vec::new();
        };
        STATISTICS
            .iter()
            .filter_map(|(field, label)| {
                let value = storage.get(*field).and_then(bson_number)?;
                Some(((*label).to_owned(), value.to_string()))
            })
            .collect()
    }
}

fn index_info(mut specification: Document) -> DocumentIndex {
    specification.remove("v");
    let name = specification.get_str("name").unwrap_or_default().to_owned();
    let unique = specification.get_bool("unique").unwrap_or(false);
    let detail = index_detail(&specification);
    DocumentIndex {
        name,
        definition: document_to_editable_json(specification),
        unique,
        detail,
    }
}

fn index_detail(specification: &Document) -> Option<String> {
    let mut traits = Vec::new();
    if let Some(seconds) = specification
        .get("expireAfterSeconds")
        .and_then(bson_number)
    {
        traits.push(format!("TTL {seconds} s"));
    }
    for flag in ["sparse", "hidden"] {
        if specification.get_bool(flag).unwrap_or(false) {
            traits.push(flag.to_owned());
        }
    }
    if specification.contains_key("partialFilterExpression") {
        traits.push("partial".to_owned());
    }
    let special_types = specification
        .get_document("key")
        .map(|keys| {
            keys.values()
                .filter_map(Bson::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    traits.extend(special_types);
    (!traits.is_empty()).then(|| traits.join(", "))
}

fn option_properties(options: &Document) -> Vec<(String, String)> {
    let mut properties: Vec<(String, String)> = OPTION_PROPERTIES
        .iter()
        .filter_map(|name| Some(((*name).to_owned(), display(options.get(*name)?))))
        .collect();
    if let Ok(series) = options.get_document("timeseries") {
        for (name, value) in series {
            properties.push((format!("timeseries.{name}"), display(value)));
        }
    }
    for name in ["pipeline", "collation"] {
        if let Some(value) = options.get(name) {
            properties.push((name.to_owned(), to_editable_json(value.clone()).to_string()));
        }
    }
    properties
}

fn display(value: &Bson) -> String {
    match value {
        Bson::String(text) => text.clone(),
        other => other.clone().into_relaxed_extjson().to_string(),
    }
}

fn validator_schema(options: &Document) -> Option<serde_json::Value> {
    let validator = options.get_document("validator").ok()?;
    match validator.get_document("$jsonSchema") {
        Ok(schema) => Some(document_to_editable_json(schema.clone())),
        Err(_) if validator.is_empty() => None,
        Err(_) => Some(document_to_editable_json(validator.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn index_details_list_ttl_and_special_types() {
        let index = index_info(doc! {
            "v": 2,
            "key": { "createdAt": 1, "body": "text" },
            "name": "ttl_text",
            "expireAfterSeconds": 3600,
            "sparse": true,
            "unique": true,
        });
        assert_eq!(index.name, "ttl_text");
        assert!(index.unique);
        assert_eq!(index.detail.as_deref(), Some("TTL 3600 s, sparse, text"));
        assert!(index.definition.get("v").is_none());
        assert_eq!(
            index.definition["key"],
            json!({"body": "text", "createdAt": 1})
        );
    }

    #[test]
    fn json_schema_validators_are_the_collection_schema() {
        let options = doc! {
            "validator": { "$jsonSchema": { "bsonType": "object", "required": ["name"] } },
        };
        assert_eq!(
            validator_schema(&options),
            Some(json!({"bsonType": "object", "required": ["name"]}))
        );
        let query_validator = doc! { "validator": { "age": { "$gte": 0 } } };
        assert_eq!(
            validator_schema(&query_validator),
            Some(json!({"age": {"$gte": 0}}))
        );
        assert_eq!(validator_schema(&doc! {}), None);
    }

    #[test]
    fn collection_options_become_properties() {
        let properties = option_properties(&doc! {
            "capped": true,
            "size": 4096,
            "timeseries": { "timeField": "ts", "granularity": "seconds" },
        });
        assert_eq!(
            properties,
            vec![
                ("capped".to_owned(), "true".to_owned()),
                ("size".to_owned(), "4096".to_owned()),
                ("timeseries.timeField".to_owned(), "ts".to_owned()),
                ("timeseries.granularity".to_owned(), "seconds".to_owned()),
            ]
        );
    }
}
