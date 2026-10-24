/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::json_lines::{Record, column_order};
use crate::values::{is_server_timestamp, to_value};
use relix_core::{ColumnMeta, RowBatch};
use serde_json::Value as Json;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResultColumn {
    pub(crate) name: String,
    pub(crate) type_name: Option<String>,
    pub(crate) timestamp: bool,
}

pub(crate) fn build_batch(
    records: &[Record],
    columns: Vec<ResultColumn>,
    has_more: bool,
) -> RowBatch {
    let rows = records
        .iter()
        .map(|record| {
            columns
                .iter()
                .map(|column| to_value(record.get(&column.name), column.timestamp))
                .collect()
        })
        .collect();
    RowBatch {
        columns: columns
            .into_iter()
            .map(|column| ColumnMeta::new(column.name, column.type_name))
            .collect(),
        rows,
        has_more,
    }
}

/// Derives result columns for ad-hoc queries, whose JSON Lines output carries no types.
/// A column counts as a timestamp when every value in it is rendered the way the server
/// renders timestamps; other columns are typed by their first non-null value.
pub(crate) fn inferred_columns(records: &[Record]) -> Vec<ResultColumn> {
    column_order(records)
        .into_iter()
        .map(|name| {
            let values: Vec<&Json> = records
                .iter()
                .filter_map(|record| record.get(&name))
                .filter(|value| !value.is_null())
                .collect();
            let timestamp = !values.is_empty()
                && values
                    .iter()
                    .all(|value| value.as_str().is_some_and(is_server_timestamp));
            let type_name = if timestamp {
                Some("Timestamp".to_owned())
            } else {
                values.first().map(|value| json_type(value).to_owned())
            };
            ResultColumn {
                name,
                type_name,
                timestamp,
            }
        })
        .collect()
}

fn json_type(value: &Json) -> &'static str {
    match value {
        Json::Bool(_) => "Boolean",
        Json::Number(number) if number.is_i64() => "Int64",
        Json::Number(number) if number.is_u64() => "UInt64",
        Json::Number(_) => "Float64",
        Json::String(_) => "Utf8",
        _ => "Json",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::Value;

    fn record(line: &str) -> Record {
        serde_json::from_str(line).unwrap()
    }

    #[test]
    fn ad_hoc_results_infer_timestamps_and_types() {
        let records = vec![
            record(r#"{"host":"a","time":"2024-01-01T00:00:00","usage":1.5}"#),
            record(r#"{"host":"b","time":"2024-01-01T00:00:01.5","usage":2,"note":"x"}"#),
        ];
        let columns = inferred_columns(&records);
        let batch = build_batch(&records, columns, false);
        let names: Vec<&str> = batch
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect();
        assert_eq!(names, ["host", "time", "usage", "note"]);
        assert_eq!(batch.columns[1].type_name.as_deref(), Some("Timestamp"));
        assert_eq!(batch.columns[2].type_name.as_deref(), Some("Float64"));
        assert_eq!(
            batch.rows[1][1],
            Value::Text("2024-01-01T00:00:01.5Z".into())
        );
        assert_eq!(batch.rows[0][3], Value::Null);
    }
}
