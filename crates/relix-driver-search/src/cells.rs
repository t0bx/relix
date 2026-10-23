/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{ColumnMeta, RowBatch, Value};
use serde_json::Value as Json;

pub fn cell(value: &Json) -> Value {
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

/// Builds a batch from rows of the given columns, keeping at most `row_limit` rows;
/// `more_available` marks results the server holds beyond the rows it sent.
pub fn batch(
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
    row_limit: usize,
    more_available: bool,
) -> RowBatch {
    let truncated = rows.len() > row_limit;
    RowBatch {
        columns: columns
            .into_iter()
            .map(|name| ColumnMeta::new(name, None))
            .collect(),
        rows: rows.into_iter().take(row_limit).collect(),
        has_more: truncated || more_available,
    }
}

pub fn push_column(columns: &mut Vec<String>, name: &str) {
    if !columns.iter().any(|column| column == name) {
        columns.push(name.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_values_map_to_cells() {
        assert_eq!(cell(&json!(null)), Value::Null);
        assert_eq!(cell(&json!(-3)), Value::Int(-3));
        assert_eq!(cell(&json!(u64::MAX)), Value::UInt(u64::MAX));
        assert_eq!(cell(&json!(1.5)), Value::Float(1.5));
        assert_eq!(cell(&json!("a")), Value::Text("a".into()));
        assert_eq!(cell(&json!({"a": [1]})), Value::Text("{\"a\":[1]}".into()));
    }

    #[test]
    fn batches_truncate_at_the_row_limit() {
        let rows = vec![
            vec![Value::Int(1)],
            vec![Value::Int(2)],
            vec![Value::Int(3)],
        ];
        let limited = batch(vec!["n".into()], rows.clone(), 2, false);
        assert_eq!(limited.rows.len(), 2);
        assert!(limited.has_more);
        assert!(!batch(vec!["n".into()], rows, 3, false).has_more);
    }
}
