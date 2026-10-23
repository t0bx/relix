/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::attribute_json::{Item, item_to_json, json_to_item};
use crate::connection::DynamoConnection;
use crate::errors::map_sdk_error;
use crate::read_request::{ReadRequest, read_request};
use aws_sdk_dynamodb::Client;
use relix_core::{CollectionRef, Document, DocumentPage, DocumentQuery};
use relix_driver_api::DriverError;
use serde_json::Value as Json;

pub const DEFAULT_PAGE_SIZE: u32 = 100;
const MAX_REQUESTS_PER_PAGE: usize = 10;

struct ReadBatch {
    items: Vec<Item>,
    last_key: Option<Item>,
}

impl DynamoConnection {
    /// Reads one page with Scan or Query. DynamoDB's `Limit` caps evaluated items, so a
    /// selective filter can return fewer matches than requested; further requests
    /// continue from `LastEvaluatedKey` until the page is full, the table is exhausted
    /// or ten requests were made. The page token is that key as JSON. Totals are not
    /// reported because counting needs a full scan.
    pub(crate) async fn find_page(
        &self,
        collection: &CollectionRef,
        query: &DocumentQuery,
    ) -> Result<DocumentPage, DriverError> {
        let key_fields = self.key_fields(&collection.name).await?;
        let mut request = read_request(query)?;
        include_key_attributes(&mut request, &key_fields);
        let limit = if query.limit == 0 {
            DEFAULT_PAGE_SIZE as usize
        } else {
            query.limit as usize
        };
        let client = self.client()?;
        let mut start = parse_page_token(query.page_token.as_deref())?;
        let mut items = Vec::new();
        for _ in 0..MAX_REQUESTS_PER_PAGE {
            let remaining = limit - items.len();
            let batch = read_batch(&client, &collection.name, &request, remaining, start).await?;
            items.extend(batch.items);
            start = batch.last_key;
            if items.len() >= limit || start.is_none() {
                break;
            }
        }
        Ok(DocumentPage {
            documents: items
                .iter()
                .map(|item| page_document(item, &key_fields))
                .collect(),
            next_page: start.as_ref().map(|key| item_to_json(key).to_string()),
            total: None,
        })
    }
}

async fn read_batch(
    client: &Client,
    table: &str,
    request: &ReadRequest,
    limit: usize,
    start: Option<Item>,
) -> Result<ReadBatch, DriverError> {
    let limit = i32::try_from(limit).unwrap_or(i32::MAX);
    if request.is_query() {
        let output = client
            .query()
            .table_name(table)
            .set_index_name(request.index_name.clone())
            .set_key_condition_expression(request.key_condition.clone())
            .set_filter_expression(request.filter_expression.clone())
            .set_projection_expression(request.projection_expression.clone())
            .set_expression_attribute_names(request.names.clone())
            .set_expression_attribute_values(request.values.clone())
            .set_scan_index_forward(request.scan_forward)
            .set_consistent_read(request.consistent_read)
            .set_exclusive_start_key(start)
            .limit(limit)
            .send()
            .await
            .map_err(map_sdk_error)?;
        return Ok(ReadBatch {
            items: output.items.unwrap_or_default(),
            last_key: output.last_evaluated_key.filter(|key| !key.is_empty()),
        });
    }
    let output = client
        .scan()
        .table_name(table)
        .set_index_name(request.index_name.clone())
        .set_filter_expression(request.filter_expression.clone())
        .set_projection_expression(request.projection_expression.clone())
        .set_expression_attribute_names(request.names.clone())
        .set_expression_attribute_values(request.values.clone())
        .set_consistent_read(request.consistent_read)
        .set_exclusive_start_key(start)
        .limit(limit)
        .send()
        .await
        .map_err(map_sdk_error)?;
    Ok(ReadBatch {
        items: output.items.unwrap_or_default(),
        last_key: output.last_evaluated_key.filter(|key| !key.is_empty()),
    })
}

/// Adds the table's key attributes to an explicit projection so every returned item can
/// still be identified for editing and deleting.
fn include_key_attributes(request: &mut ReadRequest, key_fields: &[String]) {
    let Some(expression) = request.projection_expression.as_mut() else {
        return;
    };
    let names = request.names.get_or_insert_with(Default::default);
    for (index, field) in key_fields.iter().enumerate() {
        let placeholder = format!("#relix_k{index}");
        expression.push_str(", ");
        expression.push_str(&placeholder);
        names.insert(placeholder, field.clone());
    }
}

pub fn page_document(item: &Item, key_fields: &[String]) -> Document {
    Document {
        key: item_key_json(item, key_fields),
        body: item_to_json(item),
    }
}

pub fn item_key_json(item: &Item, key_fields: &[String]) -> Json {
    let key: Item = key_fields
        .iter()
        .filter_map(|field| Some((field.clone(), item.get(field)?.clone())))
        .collect();
    item_to_json(&key)
}

pub fn parse_page_token(token: Option<&str>) -> Result<Option<Item>, DriverError> {
    let Some(token) = token.map(str::trim).filter(|token| !token.is_empty()) else {
        return Ok(None);
    };
    let json: Json = serde_json::from_str(token)
        .map_err(|_| DriverError::query("invalid page token: expected the last evaluated key"))?;
    json_to_item(&json).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_dynamodb::types::AttributeValue;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    fn item() -> Item {
        Item::from([
            ("pk".to_owned(), AttributeValue::S("user#1".into())),
            ("sk".to_owned(), AttributeValue::N("7".into())),
            ("name".to_owned(), AttributeValue::S("Ada".into())),
        ])
    }

    #[test]
    fn documents_are_keyed_by_the_table_key() {
        let document = page_document(&item(), &["pk".to_owned(), "sk".to_owned()]);
        assert_eq!(document.key, json!({"pk": "user#1", "sk": 7}));
        assert_eq!(document.body["name"], json!("Ada"));
    }

    #[test]
    fn page_tokens_round_trip_the_last_evaluated_key() {
        let key = Item::from([
            ("pk".to_owned(), AttributeValue::S("user#1".into())),
            ("sk".to_owned(), AttributeValue::N("7".into())),
        ]);
        let token = item_to_json(&key).to_string();
        assert_eq!(parse_page_token(Some(&token)).unwrap(), Some(key));
        assert_eq!(parse_page_token(None).unwrap(), None);
        assert!(parse_page_token(Some("not json")).is_err());
    }

    #[test]
    fn explicit_projections_keep_the_key_attributes() {
        let mut request = ReadRequest {
            projection_expression: Some("#n".into()),
            ..ReadRequest::default()
        };
        include_key_attributes(&mut request, &["pk".to_owned()]);
        assert_eq!(
            request.projection_expression.as_deref(),
            Some("#n, #relix_k0")
        );
        assert_eq!(request.names.unwrap()["#relix_k0"], "pk");
        let mut unprojected = ReadRequest::default();
        include_key_attributes(&mut unprojected, &["pk".to_owned()]);
        assert_eq!(unprojected, ReadRequest::default());
    }
}
