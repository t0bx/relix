/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cells::{batch, cell, push_column};
use relix_core::{RowBatch, Value};
use serde_json::{Map, Value as Json};

/// Renders any JSON response as rows: an array of objects (such as `_cat/*?format=json`)
/// becomes a table with one column per field, an object becomes `key`/`value` rows,
/// and anything else a single `value` cell.
pub fn json_rows(json: &Json, row_limit: usize) -> RowBatch {
    match json {
        Json::Array(items) if !items.is_empty() && items.iter().all(Json::is_object) => {
            object_table(items, row_limit)
        }
        Json::Array(items) => batch(
            vec!["value".to_owned()],
            items.iter().map(|item| vec![cell(item)]).collect(),
            row_limit,
            false,
        ),
        Json::Object(fields) => key_value_rows(fields, row_limit),
        scalar => batch(
            vec!["value".to_owned()],
            vec![vec![cell(scalar)]],
            row_limit,
            false,
        ),
    }
}

pub fn key_value_rows(fields: &Map<String, Json>, row_limit: usize) -> RowBatch {
    batch(
        vec!["key".to_owned(), "value".to_owned()],
        fields
            .iter()
            .map(|(key, value)| vec![Value::Text(key.clone()), cell(value)])
            .collect(),
        row_limit,
        false,
    )
}

pub fn text_rows(text: &str, row_limit: usize) -> RowBatch {
    batch(
        vec!["response".to_owned()],
        text.lines()
            .map(|line| vec![Value::Text(line.to_owned())])
            .collect(),
        row_limit,
        false,
    )
}

/// Bulk responses become one row per item with its action, target, status and the
/// failure reason, so partial failures are visible.
pub fn bulk_rows(items: &[Json], row_limit: usize) -> RowBatch {
    let columns = ["action", "_index", "_id", "status", "result", "error"]
        .map(str::to_owned)
        .to_vec();
    let rows = items
        .iter()
        .filter_map(|item| item.as_object()?.iter().next())
        .map(|(action, outcome)| {
            let error = match outcome.get("error") {
                Some(error) => error.get("reason").map(cell).unwrap_or_else(|| cell(error)),
                None => Value::Null,
            };
            vec![
                Value::Text(action.clone()),
                outcome.get("_index").map(cell).unwrap_or(Value::Null),
                outcome.get("_id").map(cell).unwrap_or(Value::Null),
                outcome.get("status").map(cell).unwrap_or(Value::Null),
                outcome.get("result").map(cell).unwrap_or(Value::Null),
                error,
            ]
        })
        .collect();
    batch(columns, rows, row_limit, false)
}

fn object_table(items: &[Json], row_limit: usize) -> RowBatch {
    let mut columns = Vec::new();
    for item in items.iter().take(row_limit) {
        if let Json::Object(fields) = item {
            for key in fields.keys() {
                push_column(&mut columns, key);
            }
        }
    }
    let rows = items
        .iter()
        .map(|item| {
            columns
                .iter()
                .map(|column| item.get(column).map(cell).unwrap_or(Value::Null))
                .collect()
        })
        .collect();
    batch(columns, rows, row_limit, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn arrays_of_objects_become_tables() {
        let rows = json_rows(
            &json!([{"index": "a", "docs.count": "1"}, {"index": "b", "health": "green"}]),
            10,
        );
        let names: Vec<_> = rows
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect();
        assert_eq!(names.len(), 3);
        assert_eq!(names[2], "health");
        assert_eq!(rows.rows[1][2], Value::Text("green".into()));
        assert_eq!(rows.rows[0][2], Value::Null);
    }

    #[test]
    fn objects_become_key_value_rows() {
        let rows = json_rows(&json!({"status": "green", "nodes": {"total": 1}}), 1);
        assert_eq!(rows.columns.len(), 2);
        assert_eq!(rows.rows.len(), 1);
        assert!(rows.has_more);
        assert_eq!(
            json_rows(&json!(true), 5).rows,
            vec![vec![Value::Bool(true)]]
        );
    }

    #[test]
    fn bulk_items_show_failures() {
        let items = json!([
            {"index": {"_index": "a", "_id": "1", "status": 201, "result": "created"}},
            {"create": {"_index": "a", "_id": "2", "status": 409,
                "error": {"type": "version_conflict_engine_exception", "reason": "exists"}}}
        ]);
        let rows = bulk_rows(items.as_array().unwrap(), 10);
        assert_eq!(rows.rows[0][0], Value::Text("index".into()));
        assert_eq!(rows.rows[1][3], Value::Int(409));
        assert_eq!(rows.rows[1][5], Value::Text("exists".into()));
    }

    #[test]
    fn text_responses_are_split_into_lines() {
        let rows = text_rows("health status index\ngreen open a\n", 10);
        assert_eq!(rows.rows.len(), 2);
    }
}
