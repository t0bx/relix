/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{ColumnMeta, RowBatch, Value};
use serde_json::Value as Json;
use std::collections::HashMap;

pub const VALUE_COLUMN: &str = "value";

/// Flattens documents into a row batch for query consoles. Every top-level field becomes
/// a column in order of first appearance, scalars become plain values and nested objects
/// or arrays are rendered as compact JSON text. A document that is not an object fills a
/// single `value` column.
pub fn document_rows<I>(documents: I, has_more: bool) -> RowBatch
where
    I: IntoIterator<Item = Vec<(String, Json)>>,
{
    let mut columns: Vec<ColumnMeta> = Vec::new();
    let mut positions: HashMap<String, usize> = HashMap::new();
    let mut cells: Vec<Vec<(usize, Value)>> = Vec::new();
    for fields in documents {
        let row = fields
            .into_iter()
            .map(|(name, value)| {
                let position = *positions.entry(name.clone()).or_insert_with(|| {
                    columns.push(ColumnMeta::new(name, None));
                    columns.len() - 1
                });
                (position, json_cell(&value))
            })
            .collect();
        cells.push(row);
    }
    let width = columns.len();
    let rows = cells
        .into_iter()
        .map(|row| {
            let mut values = vec![Value::Null; width];
            for (position, value) in row {
                values[position] = value;
            }
            values
        })
        .collect();
    RowBatch {
        columns,
        rows,
        has_more,
    }
}

pub fn document_fields(document: Json) -> Vec<(String, Json)> {
    match document {
        Json::Object(map) => map.into_iter().collect(),
        other => vec![(VALUE_COLUMN.to_owned(), other)],
    }
}

pub fn json_cell(value: &Json) -> Value {
    match value {
        Json::Null => Value::Null,
        Json::Bool(flag) => Value::Bool(*flag),
        Json::Number(number) => number
            .as_i64()
            .map(Value::Int)
            .or_else(|| number.as_u64().map(Value::UInt))
            .or_else(|| number.as_f64().map(Value::Float))
            .unwrap_or_else(|| Value::Text(number.to_string())),
        Json::String(text) => Value::Text(text.clone()),
        nested => Value::Text(nested.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn columns_follow_first_appearance_and_missing_fields_are_null() {
        let batch = document_rows(
            vec![
                vec![
                    ("_id".to_owned(), json!(1)),
                    ("name".to_owned(), json!("a")),
                ],
                vec![
                    ("_id".to_owned(), json!(2)),
                    ("tags".to_owned(), json!(["x"])),
                ],
            ],
            true,
        );
        let names: Vec<_> = batch.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["_id", "name", "tags"]);
        assert_eq!(
            batch.rows[1],
            vec![Value::Int(2), Value::Null, Value::Text("[\"x\"]".into())]
        );
        assert!(batch.has_more);
    }

    #[test]
    fn scalars_map_to_values_and_nested_values_to_json_text() {
        assert_eq!(json_cell(&json!(u64::MAX)), Value::UInt(u64::MAX));
        assert_eq!(json_cell(&json!(1.5)), Value::Float(1.5));
        assert_eq!(json_cell(&json!(true)), Value::Bool(true));
        assert_eq!(
            json_cell(&json!({"$oid": "65f0"})),
            Value::Text("{\"$oid\":\"65f0\"}".into())
        );
    }

    #[test]
    fn non_object_documents_fill_the_value_column() {
        let batch = document_rows(vec![document_fields(json!("plain"))], false);
        assert_eq!(batch.columns[0].name, VALUE_COLUMN);
        assert_eq!(batch.rows[0], vec![Value::Text("plain".into())]);
    }
}
