/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::dynamodb_json::expression_value;
use aws_sdk_dynamodb::types::AttributeValue;
use relix_core::DocumentQuery;
use relix_driver_api::DriverError;
use serde_json::{Map, Value as Json};
use std::collections::HashMap;

pub const FILTER_FIELDS: [&str; 8] = [
    "IndexName",
    "KeyConditionExpression",
    "FilterExpression",
    "ProjectionExpression",
    "ExpressionAttributeNames",
    "ExpressionAttributeValues",
    "ScanIndexForward",
    "ConsistentRead",
];

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReadRequest {
    pub index_name: Option<String>,
    pub key_condition: Option<String>,
    pub filter_expression: Option<String>,
    pub projection_expression: Option<String>,
    pub names: Option<HashMap<String, String>>,
    pub values: Option<HashMap<String, AttributeValue>>,
    pub scan_forward: Option<bool>,
    pub consistent_read: Option<bool>,
}

impl ReadRequest {
    pub fn is_query(&self) -> bool {
        self.key_condition.is_some()
    }
}

/// Translates a document query into Query/Scan parameters. The filter is an object with
/// the native request fields listed in [`FILTER_FIELDS`]; with `KeyConditionExpression`
/// a Query runs, otherwise a Scan. The projection may be an expression string or a list
/// of attribute names, and the sort (`1`/`-1`, `"asc"`/`"desc"` or
/// `{"ScanIndexForward": bool}`) is only valid for a Query.
pub fn read_request(query: &DocumentQuery) -> Result<ReadRequest, DriverError> {
    let mut request = match &query.filter {
        None | Some(Json::Null) => ReadRequest::default(),
        Some(Json::Object(fields)) => filter_request(fields)?,
        Some(_) => {
            return Err(DriverError::query(
                "the filter must be an object such as {\"KeyConditionExpression\": \"pk = :id\", \
                 \"ExpressionAttributeValues\": {\":id\": \"user#1\"}}",
            ));
        }
    };
    apply_projection(&mut request, query.projection.as_ref())?;
    if let Some(forward) = sort_direction(query.sort.as_ref())? {
        request.scan_forward = Some(forward);
    }
    if request.scan_forward.is_some() && !request.is_query() {
        return Err(DriverError::unsupported(
            "Scan results have no order: sorting needs a KeyConditionExpression (Query)",
        ));
    }
    Ok(request)
}

fn filter_request(fields: &Map<String, Json>) -> Result<ReadRequest, DriverError> {
    if let Some(unknown) = fields
        .keys()
        .find(|name| !FILTER_FIELDS.contains(&name.as_str()))
    {
        return Err(DriverError::query(format!(
            "unknown filter field \"{unknown}\": use {}",
            FILTER_FIELDS.join(", ")
        )));
    }
    Ok(ReadRequest {
        index_name: text(fields, "IndexName")?,
        key_condition: text(fields, "KeyConditionExpression")?,
        filter_expression: text(fields, "FilterExpression")?,
        projection_expression: text(fields, "ProjectionExpression")?,
        names: attribute_names(fields.get("ExpressionAttributeNames"))?,
        values: attribute_values(fields.get("ExpressionAttributeValues"))?,
        scan_forward: flag(fields, "ScanIndexForward")?,
        consistent_read: flag(fields, "ConsistentRead")?,
    })
}

fn text(fields: &Map<String, Json>, name: &str) -> Result<Option<String>, DriverError> {
    match fields.get(name) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::String(value)) if value.trim().is_empty() => Ok(None),
        Some(Json::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(DriverError::query(format!("{name} must be a string"))),
    }
}

fn flag(fields: &Map<String, Json>, name: &str) -> Result<Option<bool>, DriverError> {
    match fields.get(name) {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err(DriverError::query(format!("{name} must be true or false"))),
    }
}

fn attribute_names(value: Option<&Json>) -> Result<Option<HashMap<String, String>>, DriverError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let invalid = || DriverError::query("ExpressionAttributeNames must map \"#name\" to strings");
    let Json::Object(map) = value else {
        return Err(invalid());
    };
    map.iter()
        .map(|(placeholder, name)| {
            let name = name.as_str().ok_or_else(invalid)?;
            Ok((placeholder.clone(), name.to_owned()))
        })
        .collect::<Result<_, _>>()
        .map(Some)
}

fn attribute_values(
    value: Option<&Json>,
) -> Result<Option<HashMap<String, AttributeValue>>, DriverError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let Json::Object(map) = value else {
        return Err(DriverError::query(
            "ExpressionAttributeValues must map \":value\" placeholders to values",
        ));
    };
    map.iter()
        .map(|(placeholder, value)| Ok((placeholder.clone(), expression_value(value)?)))
        .collect::<Result<_, _>>()
        .map(Some)
}

fn apply_projection(
    request: &mut ReadRequest,
    projection: Option<&Json>,
) -> Result<(), DriverError> {
    let expression = match projection {
        None | Some(Json::Null) => return Ok(()),
        Some(Json::String(expression)) => expression.clone(),
        Some(Json::Array(attributes)) => {
            let names = request.names.get_or_insert_with(HashMap::new);
            let mut placeholders = Vec::with_capacity(attributes.len());
            for (index, attribute) in attributes.iter().enumerate() {
                let attribute = attribute
                    .as_str()
                    .ok_or_else(|| DriverError::query("projection attributes must be strings"))?;
                let placeholder = format!("#relix_p{index}");
                names.insert(placeholder.clone(), attribute.to_owned());
                placeholders.push(placeholder);
            }
            placeholders.join(", ")
        }
        Some(_) => {
            return Err(DriverError::query(
                "the projection must be an expression string or a list of attribute names",
            ));
        }
    };
    if request.projection_expression.is_some() {
        return Err(DriverError::query(
            "give the projection either in the filter's ProjectionExpression or as projection",
        ));
    }
    request.projection_expression = Some(expression);
    Ok(())
}

fn sort_direction(sort: Option<&Json>) -> Result<Option<bool>, DriverError> {
    let invalid = || {
        DriverError::query("sort must be 1, -1, \"asc\", \"desc\" or {\"ScanIndexForward\": bool}")
    };
    match sort {
        None | Some(Json::Null) => Ok(None),
        Some(Json::Number(direction)) => match direction.as_i64() {
            Some(1) => Ok(Some(true)),
            Some(-1) => Ok(Some(false)),
            _ => Err(invalid()),
        },
        Some(Json::String(direction)) => match direction.to_ascii_lowercase().as_str() {
            "asc" => Ok(Some(true)),
            "desc" => Ok(Some(false)),
            _ => Err(invalid()),
        },
        Some(Json::Object(fields)) if fields.is_empty() => Ok(None),
        Some(Json::Object(fields)) => match fields.get("ScanIndexForward") {
            Some(Json::Bool(forward)) if fields.len() == 1 => Ok(Some(*forward)),
            _ => Err(invalid()),
        },
        Some(_) => Err(invalid()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use relix_driver_api::ErrorKind;
    use serde_json::json;

    fn query(filter: Json) -> DocumentQuery {
        DocumentQuery {
            filter: Some(filter),
            ..DocumentQuery::first_page(10)
        }
    }

    #[test]
    fn empty_filters_scan() {
        let request = read_request(&DocumentQuery::first_page(10)).unwrap();
        assert!(!request.is_query());
        assert_eq!(request, ReadRequest::default());
    }

    #[test]
    fn key_conditions_query_with_values_in_both_notations() {
        let request = read_request(&DocumentQuery {
            sort: Some(json!(-1)),
            ..query(json!({
                "IndexName": "byCity",
                "KeyConditionExpression": "#c = :city AND age > :min",
                "ExpressionAttributeNames": {"#c": "city"},
                "ExpressionAttributeValues": {":city": "Berlin", ":min": {"N": "18"}},
                "ConsistentRead": false
            }))
        })
        .unwrap();
        assert!(request.is_query());
        assert_eq!(request.index_name.as_deref(), Some("byCity"));
        assert_eq!(request.scan_forward, Some(false));
        let values = request.values.unwrap();
        assert_eq!(values[":city"], AttributeValue::S("Berlin".into()));
        assert_eq!(values[":min"], AttributeValue::N("18".into()));
        assert_eq!(request.names.unwrap()["#c"], "city");
    }

    #[test]
    fn attribute_list_projections_use_placeholders() {
        let request = read_request(&DocumentQuery {
            projection: Some(json!(["name", "size"])),
            ..DocumentQuery::first_page(10)
        })
        .unwrap();
        assert_eq!(
            request.projection_expression.as_deref(),
            Some("#relix_p0, #relix_p1")
        );
        assert_eq!(request.names.unwrap()["#relix_p1"], "size");
    }

    #[test]
    fn invalid_filters_are_explained() {
        let unknown = read_request(&query(json!({"KeyCondition": "x"}))).unwrap_err();
        assert!(unknown.message.contains("KeyConditionExpression"));
        let scan_sort = read_request(&DocumentQuery {
            sort: Some(json!("desc")),
            ..DocumentQuery::first_page(10)
        })
        .unwrap_err();
        assert_eq!(scan_sort.kind, ErrorKind::Unsupported);
        assert!(read_request(&query(json!("pk = 1"))).is_err());
    }
}
