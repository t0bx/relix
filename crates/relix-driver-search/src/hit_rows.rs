/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::cells::{batch, cell, push_column};
use crate::hits::{SearchHits, hit_body};
use relix_core::{RowBatch, Value};
use serde_json::Value as Json;

const META_COLUMNS: [&str; 3] = ["_index", "_id", "_score"];

/// One row per hit: `_index`, `_id`, `_score`, then the top-level `_source` fields in
/// order of first appearance (nested values as JSON text). `has_more` is set when the
/// server matched more hits than it returned or the row limit cut the page.
pub fn hit_rows(hits: &SearchHits<'_>, row_limit: usize) -> RowBatch {
    let shown = &hits.hits[..hits.hits.len().min(row_limit)];
    let mut columns: Vec<String> = META_COLUMNS.iter().map(|name| (*name).to_owned()).collect();
    let bodies: Vec<Json> = shown.iter().map(hit_body).collect();
    for body in &bodies {
        if let Json::Object(fields) = body {
            for field in fields.keys() {
                push_column(&mut columns, field);
            }
        }
    }
    let rows = shown
        .iter()
        .zip(&bodies)
        .map(|(hit, body)| {
            columns
                .iter()
                .enumerate()
                .map(|(position, column)| match position {
                    0..=2 => hit.get(column).map(cell).unwrap_or(Value::Null),
                    _ => body.get(column).map(cell).unwrap_or(Value::Null),
                })
                .collect()
        })
        .collect();
    let more = hits.hits.len() > row_limit || hits.total.exceeds(hits.hits.len() as u64);
    batch(columns, rows, row_limit, more)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn hits_flatten_into_source_columns() {
        let response = json!({"hits": {"total": {"value": 3, "relation": "eq"}, "hits": [
            {"_index": "a", "_id": "1", "_score": 1.0, "_source": {"name": "x", "tags": ["t"]}},
            {"_index": "a", "_id": "2", "_score": 0.5, "_source": {"price": 3}}
        ]}});
        let hits = SearchHits::from_response(&response).unwrap();
        let rows = hit_rows(&hits, 10);
        let names: Vec<_> = rows
            .columns
            .iter()
            .map(|column| column.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["_index", "_id", "_score", "name", "tags", "price"]
        );
        assert_eq!(rows.rows[0][4], Value::Text("[\"t\"]".into()));
        assert_eq!(rows.rows[1][5], Value::Int(3));
        assert_eq!(rows.rows[1][3], Value::Null);
        assert!(rows.has_more);
    }

    #[test]
    fn complete_results_have_no_more_rows() {
        let response = json!({"hits": {"total": {"value": 1, "relation": "eq"}, "hits": [
            {"_index": "a", "_id": "1", "_score": null, "_source": {}}
        ]}});
        let hits = SearchHits::from_response(&response).unwrap();
        let rows = hit_rows(&hits, 10);
        assert!(!rows.has_more);
        assert_eq!(rows.rows.len(), 1);
        assert!(hit_rows(&hits, 0).has_more);
    }
}
