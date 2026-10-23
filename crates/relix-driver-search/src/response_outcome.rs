/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::client::RawResponse;
use crate::hit_rows::hit_rows;
use crate::hits::SearchHits;
use crate::json_rows::{bulk_rows, json_rows, key_value_rows, text_rows};
use crate::writes::outcome;
use relix_core::StatementOutcome;
use relix_driver_api::DriverError;
use relix_driver_http::{Method, StatusCode};
use serde_json::Value as Json;

const BY_QUERY_COUNTS: [&str; 3] = ["created", "updated", "deleted"];

/// Turns a console response into an outcome: search hits become rows (aggregations
/// when a search returned no hits), bulk responses one row per item, acknowledged and
/// single-document operations a completion message, `_by_query`/`_reindex` an affected
/// count, other JSON key/value or table rows and plain text one row per line.
pub fn response_outcome(
    method: &Method,
    response: RawResponse,
    row_limit: usize,
) -> Result<StatementOutcome, DriverError> {
    let status = response.status;
    if *method == Method::HEAD {
        if status.is_success() || status == StatusCode::NOT_FOUND {
            return Ok(completed(format!("HTTP {status}")));
        }
        return Err(response.into_error());
    }
    if !status.is_success() {
        return Err(response.into_error());
    }
    let text = response.body.trim();
    if text.is_empty() {
        return Ok(completed(format!("HTTP {status}")));
    }
    Ok(match serde_json::from_str::<Json>(text) {
        Ok(json) => json_outcome(&json, row_limit),
        Err(_) => StatementOutcome::Rows(text_rows(text, row_limit)),
    })
}

fn json_outcome(json: &Json, row_limit: usize) -> StatementOutcome {
    if let Some(hits) = SearchHits::from_response(json) {
        if hits.hits.is_empty()
            && let Some(aggregations) = json.get("aggregations").and_then(Json::as_object)
        {
            return StatementOutcome::Rows(key_value_rows(aggregations, row_limit));
        }
        return StatementOutcome::Rows(hit_rows(&hits, row_limit));
    }
    if let (Some(items), Some(_)) = (json["items"].as_array(), json.get("errors")) {
        return StatementOutcome::Rows(bulk_rows(items, row_limit));
    }
    if json["acknowledged"].as_bool() == Some(true) {
        return completed(match json["index"].as_str() {
            Some(index) => format!("acknowledged (index {index})"),
            None => "acknowledged".to_owned(),
        });
    }
    if is_by_query(json) {
        let rows = BY_QUERY_COUNTS
            .iter()
            .filter_map(|field| json[*field].as_u64())
            .sum();
        return StatementOutcome::Affected { rows };
    }
    if json["result"].is_string() && json.get("_id").is_some() {
        return completed(outcome(json).message.unwrap_or_default());
    }
    StatementOutcome::Rows(json_rows(json, row_limit))
}

fn is_by_query(json: &Json) -> bool {
    json.get("batches").is_some()
        && json["failures"]
            .as_array()
            .is_some_and(|failures| failures.is_empty())
}

fn completed(message: String) -> StatementOutcome {
    StatementOutcome::Completed { message }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::ErrorKind;
    use serde_json::json;

    fn render(status: u16, body: Json) -> StatementOutcome {
        let response = RawResponse {
            status: StatusCode::from_u16(status).unwrap(),
            body: body.to_string(),
        };
        response_outcome(&Method::POST, response, 100).unwrap()
    }

    #[test]
    fn searches_without_hits_show_aggregations() {
        let outcome = render(
            200,
            json!({"hits": {"total": {"value": 5, "relation": "eq"}, "hits": []},
                "aggregations": {"by_status": {"buckets": []}}}),
        );
        let StatementOutcome::Rows(rows) = outcome else {
            panic!("expected rows");
        };
        assert_eq!(rows.columns[0].name, "key");
        assert_eq!(rows.rows.len(), 1);
    }

    #[test]
    fn acknowledged_and_document_operations_complete() {
        assert_eq!(
            render(200, json!({"acknowledged": true, "index": "orders"})),
            completed("acknowledged (index orders)".into())
        );
        assert_eq!(
            render(201, json!({"_index": "o", "_id": "1", "result": "created"})),
            completed("created o/1".into())
        );
        assert_eq!(
            render(
                200,
                json!({"deleted": 4, "batches": 1, "failures": [], "took": 3})
            ),
            StatementOutcome::Affected { rows: 4 }
        );
    }

    #[test]
    fn head_requests_report_the_status() {
        let missing = RawResponse {
            status: StatusCode::NOT_FOUND,
            body: String::new(),
        };
        assert_eq!(
            response_outcome(&Method::HEAD, missing, 10).unwrap(),
            completed("HTTP 404 Not Found".into())
        );
    }

    #[test]
    fn error_statuses_become_driver_errors() {
        let failed = RawResponse {
            status: StatusCode::BAD_REQUEST,
            body: json!({"error": {"reason": "unknown query [mach]"}, "status": 400}).to_string(),
        };
        let error = response_outcome(&Method::POST, failed, 10).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Query);
        assert_eq!(error.message, "unknown query [mach]");
    }
}
