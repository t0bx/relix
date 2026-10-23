/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{ColumnMeta, Value};
use std::collections::HashMap;

pub fn to_csv(columns: &[ColumnMeta], rows: &[Vec<Value>]) -> String {
    let mut out = String::new();
    push_csv_line(
        &mut out,
        columns.iter().map(|column| Some(column.name.clone())),
    );
    for row in rows {
        push_csv_line(&mut out, row.iter().map(Value::to_plain_string));
    }
    out
}

pub fn to_json(columns: &[ColumnMeta], rows: &[Vec<Value>]) -> String {
    let names = unique_names(columns);
    let objects: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let object = names
                .iter()
                .zip(row)
                .map(|(name, value)| (name.clone(), value.to_json()))
                .collect::<serde_json::Map<_, _>>();
            serde_json::Value::Object(object)
        })
        .collect();
    serde_json::to_string_pretty(&objects).unwrap_or_else(|_| "[]".to_owned())
}

/// Makes column names usable as JSON keys by suffixing duplicates (`id`, `id_2`, …), which
/// is common for joins selecting `*` from several tables.
pub fn unique_names(columns: &[ColumnMeta]) -> Vec<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    columns
        .iter()
        .map(|column| {
            let count = counts.entry(column.name.as_str()).or_insert(0);
            *count += 1;
            if *count == 1 {
                column.name.clone()
            } else {
                format!("{}_{}", column.name, count)
            }
        })
        .collect()
}

fn push_csv_line(out: &mut String, fields: impl Iterator<Item = Option<String>>) {
    for (index, field) in fields.enumerate() {
        if index > 0 {
            out.push(',');
        }
        if let Some(field) = field {
            out.push_str(&csv_field(&field));
        }
    }
    out.push_str("\r\n");
}

fn csv_field(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) || field.starts_with(' ') || field.ends_with(' ') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn columns() -> Vec<ColumnMeta> {
        vec![
            ColumnMeta::new("id", None),
            ColumnMeta::new("note", None),
            ColumnMeta::new("id", None),
        ]
    }

    #[test]
    fn csv_quotes_special_fields_and_leaves_null_empty() {
        let rows = vec![vec![
            Value::Int(1),
            Value::Text("a, \"b\"\nc".into()),
            Value::Null,
        ]];
        assert_eq!(
            to_csv(&columns(), &rows),
            "id,note,id\r\n1,\"a, \"\"b\"\"\nc\",\r\n"
        );
    }

    #[test]
    fn json_uses_unique_keys_and_typed_values() {
        let rows = vec![vec![Value::Int(1), Value::Null, Value::Int(2)]];
        let parsed: serde_json::Value = serde_json::from_str(&to_json(&columns(), &rows)).unwrap();
        assert_eq!(
            parsed,
            serde_json::json!([{"id": 1, "note": null, "id_2": 2}])
        );
    }
}
