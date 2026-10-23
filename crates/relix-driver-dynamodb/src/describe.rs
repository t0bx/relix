/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::connection::{DynamoConnection, table_key_fields};
use crate::schema_json::{
    attribute_definitions_json, key_schema_json, projection_json, projection_label, throughput_json,
};
use aws_sdk_dynamodb::types::{
    GlobalSecondaryIndexDescription, LocalSecondaryIndexDescription, TableDescription,
};
use relix_core::{CollectionDetails, CollectionKind, CollectionRef, DocumentIndex};
use relix_driver_api::DriverError;
use serde_json::json;

impl DynamoConnection {
    pub(crate) async fn describe(
        &self,
        collection: &CollectionRef,
    ) -> Result<CollectionDetails, DriverError> {
        let table = self.describe_table(&collection.name).await?;
        let mut properties = table_properties(&table);
        if let Some(ttl) = self.time_to_live(&collection.name).await {
            properties.push(("time to live".to_owned(), ttl));
        }
        let mut indexes: Vec<DocumentIndex> = table
            .global_secondary_indexes()
            .iter()
            .map(global_index)
            .collect();
        indexes.extend(table.local_secondary_indexes().iter().map(local_index));
        Ok(CollectionDetails {
            collection: collection.clone(),
            kind: CollectionKind::Table,
            key_fields: table_key_fields(&table),
            indexes,
            schema: Some(json!({
                "KeySchema": key_schema_json(table.key_schema()),
                "AttributeDefinitions": attribute_definitions_json(table.attribute_definitions()),
            })),
            properties,
        })
    }

    async fn time_to_live(&self, table: &str) -> Option<String> {
        let output = self
            .client()
            .ok()?
            .describe_time_to_live()
            .table_name(table)
            .send()
            .await
            .ok()?;
        let description = output.time_to_live_description?;
        let status = description.time_to_live_status?.as_str().to_lowercase();
        Some(match description.attribute_name {
            Some(attribute) => format!("{status} on {attribute}"),
            None => status,
        })
    }
}

fn global_index(index: &GlobalSecondaryIndexDescription) -> DocumentIndex {
    let mut definition = json!({
        "IndexName": index.index_name(),
        "KeySchema": key_schema_json(index.key_schema()),
        "Projection": projection_json(index.projection()),
    });
    if let Some(throughput) = throughput_json(index.provisioned_throughput()) {
        definition["ProvisionedThroughput"] = throughput;
    }
    let status = index
        .index_status()
        .map(|status| status.as_str().to_lowercase())
        .unwrap_or_default();
    DocumentIndex {
        name: index.index_name().unwrap_or_default().to_owned(),
        definition,
        unique: false,
        detail: Some(format!(
            "global, {status}, {} projection",
            projection_label(index.projection())
        )),
    }
}

fn local_index(index: &LocalSecondaryIndexDescription) -> DocumentIndex {
    DocumentIndex {
        name: index.index_name().unwrap_or_default().to_owned(),
        definition: json!({
            "IndexName": index.index_name(),
            "KeySchema": key_schema_json(index.key_schema()),
            "Projection": projection_json(index.projection()),
        }),
        unique: false,
        detail: Some(format!(
            "local, {} projection",
            projection_label(index.projection())
        )),
    }
}

fn table_properties(table: &TableDescription) -> Vec<(String, String)> {
    let mut properties = Vec::new();
    let mut push = |name: &str, value: Option<String>| {
        if let Some(value) = value {
            properties.push((name.to_owned(), value));
        }
    };
    push(
        "status",
        table
            .table_status()
            .map(|status| status.as_str().to_owned()),
    );
    push(
        "billing mode",
        Some(
            table
                .billing_mode_summary()
                .and_then(|summary| summary.billing_mode())
                .map(|mode| mode.as_str().to_owned())
                .unwrap_or_else(|| "PROVISIONED".to_owned()),
        ),
    );
    push(
        "item count (approximate)",
        table.item_count().map(|count| count.to_string()),
    );
    push(
        "table size (bytes, approximate)",
        table.table_size_bytes().map(|size| size.to_string()),
    );
    push(
        "provisioned throughput",
        throughput_json(table.provisioned_throughput()).map(|value| value.to_string()),
    );
    push("stream", stream_label(table));
    push(
        "table class",
        table
            .table_class_summary()
            .and_then(|summary| summary.table_class())
            .map(|class| class.as_str().to_owned()),
    );
    push(
        "deletion protection",
        table
            .deletion_protection_enabled()
            .map(|flag| flag.to_string()),
    );
    push(
        "created",
        table
            .creation_date_time()
            .map(|created| created.to_string()),
    );
    push("arn", table.table_arn().map(str::to_owned));
    properties
}

fn stream_label(table: &TableDescription) -> Option<String> {
    let specification = table.stream_specification()?;
    if !specification.stream_enabled() {
        return Some("disabled".to_owned());
    }
    let view = specification
        .stream_view_type()
        .map(|view| view.as_str().to_owned())
        .unwrap_or_default();
    Some(match table.latest_stream_arn() {
        Some(arn) => format!("{view} ({arn})"),
        None => view,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::{
        BillingMode, BillingModeSummary, IndexStatus, KeySchemaElement, KeyType, Projection,
        ProjectionType, StreamSpecification, StreamViewType, TableStatus,
    };

    fn key(name: &str, kind: KeyType) -> KeySchemaElement {
        KeySchemaElement::builder()
            .attribute_name(name)
            .key_type(kind)
            .build()
            .unwrap()
    }

    #[test]
    fn table_descriptions_become_properties() {
        let table = TableDescription::builder()
            .table_status(TableStatus::Active)
            .billing_mode_summary(
                BillingModeSummary::builder()
                    .billing_mode(BillingMode::PayPerRequest)
                    .build(),
            )
            .item_count(12)
            .stream_specification(
                StreamSpecification::builder()
                    .stream_enabled(true)
                    .stream_view_type(StreamViewType::NewImage)
                    .build()
                    .unwrap(),
            )
            .build();
        let properties = table_properties(&table);
        assert!(properties.contains(&("status".to_owned(), "ACTIVE".to_owned())));
        assert!(properties.contains(&("billing mode".to_owned(), "PAY_PER_REQUEST".to_owned())));
        assert!(properties.contains(&("item count (approximate)".to_owned(), "12".to_owned())));
        assert!(properties.contains(&("stream".to_owned(), "NEW_IMAGE".to_owned())));
    }

    #[test]
    fn global_indexes_describe_keys_and_projection() {
        let index = GlobalSecondaryIndexDescription::builder()
            .index_name("byEmail")
            .key_schema(key("email", KeyType::Hash))
            .projection(
                Projection::builder()
                    .projection_type(ProjectionType::KeysOnly)
                    .build(),
            )
            .index_status(IndexStatus::Active)
            .build();
        let described = global_index(&index);
        assert_eq!(described.name, "byEmail");
        assert_eq!(
            described.detail.as_deref(),
            Some("global, active, KEYS_ONLY projection")
        );
        assert_eq!(
            described.definition["KeySchema"],
            json!([{"AttributeName": "email", "KeyType": "HASH"}])
        );
    }
}
