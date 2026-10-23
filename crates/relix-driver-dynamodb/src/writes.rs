/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::attribute_json::{Item, json_to_item};
use crate::connection::DynamoConnection;
use crate::errors::{is_conditional_failure, map_sdk_error};
use crate::find::item_key_json;
use crate::index_definition::global_index_definition;
use aws_sdk_dynamodb::types::{
    DeleteGlobalSecondaryIndexAction, GlobalSecondaryIndexUpdate, ProvisionedThroughput,
    TableDescription,
};
use relix_core::{CollectionRef, DocumentWrite, DocumentWriteOutcome};
use relix_driver_api::{DriverError, ErrorKind};
use serde_json::Value as Json;

const KEY_PLACEHOLDER: &str = "#relix_key";

impl DynamoConnection {
    pub(crate) async fn apply_write(
        &self,
        collection: &CollectionRef,
        write: &DocumentWrite,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let table = collection.name.as_str();
        match write {
            DocumentWrite::Insert { document } => self.insert(table, document).await,
            DocumentWrite::Replace { key, document } => self.replace(table, key, document).await,
            DocumentWrite::Delete { key } => self.delete(table, key).await,
            DocumentWrite::CreateIndex { definition } => self.create_index(table, definition).await,
            DocumentWrite::DropIndex { name } => self.drop_index(table, name).await,
        }
    }

    async fn insert(
        &self,
        table: &str,
        document: &Json,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let key_fields = self.key_fields(table).await?;
        let item = json_to_item(document)?;
        let key = item_key_json(&item, &key_fields);
        require_key(&item, &key_fields)?;
        let result = self
            .client()?
            .put_item()
            .table_name(table)
            .set_item(Some(item))
            .condition_expression(format!("attribute_not_exists({KEY_PLACEHOLDER})"))
            .expression_attribute_names(KEY_PLACEHOLDER, &key_fields[0])
            .send()
            .await;
        match result {
            Ok(_) => Ok(written(key)),
            Err(error) if is_conditional_failure(&error) => Err(DriverError::query(format!(
                "an item with key {key} already exists"
            ))),
            Err(error) => Err(map_sdk_error(error)),
        }
    }

    async fn replace(
        &self,
        table: &str,
        key: &Json,
        document: &Json,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let key_fields = self.key_fields(table).await?;
        let key_item = json_to_item(key)?;
        require_key(&key_item, &key_fields)?;
        let mut item = json_to_item(document)?;
        for field in &key_fields {
            match (item.get(field), key_item.get(field)) {
                (Some(new), Some(old)) if new != old => {
                    return Err(DriverError::query(format!(
                        "key attribute \"{field}\" cannot be changed: insert a new item and \
                         delete the old one instead"
                    )));
                }
                (None, Some(old)) => {
                    item.insert(field.clone(), old.clone());
                }
                _ => {}
            }
        }
        let result = self
            .client()?
            .put_item()
            .table_name(table)
            .set_item(Some(item))
            .condition_expression(format!("attribute_exists({KEY_PLACEHOLDER})"))
            .expression_attribute_names(KEY_PLACEHOLDER, &key_fields[0])
            .send()
            .await;
        match result {
            Ok(_) => Ok(written(key.clone())),
            Err(error) if is_conditional_failure(&error) => Err(missing_item(key)),
            Err(error) => Err(map_sdk_error(error)),
        }
    }

    async fn delete(&self, table: &str, key: &Json) -> Result<DocumentWriteOutcome, DriverError> {
        let key_fields = self.key_fields(table).await?;
        let key_item = json_to_item(key)?;
        require_key(&key_item, &key_fields)?;
        let result = self
            .client()?
            .delete_item()
            .table_name(table)
            .set_key(Some(key_item))
            .condition_expression(format!("attribute_exists({KEY_PLACEHOLDER})"))
            .expression_attribute_names(KEY_PLACEHOLDER, &key_fields[0])
            .send()
            .await;
        match result {
            Ok(_) => Ok(written(key.clone())),
            Err(error) if is_conditional_failure(&error) => Err(missing_item(key)),
            Err(error) => Err(map_sdk_error(error)),
        }
    }

    async fn create_index(
        &self,
        table: &str,
        definition: &Json,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let description = self.describe_table(table).await?;
        let parsed = global_index_definition(
            definition,
            description.attribute_definitions(),
            table_throughput(&description),
        )?;
        let name = parsed.index.index_name().to_owned();
        self.client()?
            .update_table()
            .table_name(table)
            .set_attribute_definitions(Some(parsed.attribute_definitions))
            .global_secondary_index_updates(
                GlobalSecondaryIndexUpdate::builder()
                    .create(parsed.index)
                    .build(),
            )
            .send()
            .await
            .map_err(map_sdk_error)?;
        Ok(index_changed(format!(
            "creating global secondary index {name}; it is usable once its status is ACTIVE"
        )))
    }

    async fn drop_index(
        &self,
        table: &str,
        name: &str,
    ) -> Result<DocumentWriteOutcome, DriverError> {
        let action = DeleteGlobalSecondaryIndexAction::builder()
            .index_name(name)
            .build()
            .map_err(|error| DriverError::query(error.to_string()))?;
        self.client()?
            .update_table()
            .table_name(table)
            .global_secondary_index_updates(
                GlobalSecondaryIndexUpdate::builder().delete(action).build(),
            )
            .send()
            .await
            .map_err(map_sdk_error)?;
        Ok(index_changed(format!(
            "deleting global secondary index {name} (local secondary indexes cannot be dropped)"
        )))
    }
}

fn require_key(item: &Item, key_fields: &[String]) -> Result<(), DriverError> {
    match key_fields.iter().find(|field| !item.contains_key(*field)) {
        Some(missing) => Err(DriverError::query(format!(
            "the item needs its key attribute \"{missing}\""
        ))),
        None if key_fields.is_empty() => Err(DriverError::internal("the table has no key schema")),
        None => Ok(()),
    }
}

fn table_throughput(table: &TableDescription) -> Option<ProvisionedThroughput> {
    let throughput = table.provisioned_throughput()?;
    let read = throughput.read_capacity_units.filter(|units| *units > 0)?;
    let write = throughput.write_capacity_units.filter(|units| *units > 0)?;
    ProvisionedThroughput::builder()
        .read_capacity_units(read)
        .write_capacity_units(write)
        .build()
        .ok()
}

fn written(key: Json) -> DocumentWriteOutcome {
    DocumentWriteOutcome {
        affected: 1,
        key: Some(key),
        message: None,
    }
}

fn index_changed(message: String) -> DocumentWriteOutcome {
    DocumentWriteOutcome {
        affected: 1,
        key: None,
        message: Some(message),
    }
}

fn missing_item(key: &Json) -> DriverError {
    DriverError::new(
        ErrorKind::ConcurrentModification,
        format!("no item with key {key} exists any more"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;
    use std::collections::HashMap;

    #[test]
    fn items_must_carry_every_key_attribute() {
        let item: Item = HashMap::from([("pk".to_owned(), AttributeValue::S("a".into()))]);
        assert!(require_key(&item, &["pk".to_owned()]).is_ok());
        let error = require_key(&item, &["pk".to_owned(), "sk".to_owned()]).unwrap_err();
        assert!(error.message.contains("\"sk\""));
        assert!(require_key(&item, &[]).is_err());
    }
}
