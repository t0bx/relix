/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::extended_json::to_display_fields;
use mongodb::bson::{Bson, Document};
use relix_core::StatementOutcome;
use relix_driver_api::{DriverError, document_rows};

const REPLY_NOISE: [&str; 4] = ["ok", "$clusterTime", "operationTime", "electionId"];

/// Turns the reply of a non-cursor command into a statement outcome. Write commands
/// report the affected documents (or fail with their first write error, because the
/// server answers partial failures with `ok: 1`); replies carrying an unexpected cursor
/// show its batch; every other reply becomes a single row of its top-level fields.
pub fn reply_outcome(
    command_name: &str,
    mut reply: Document,
    row_limit: usize,
) -> Result<StatementOutcome, DriverError> {
    reject_write_errors(&reply)?;
    if let Some(rows) = affected_documents(command_name, &reply) {
        return Ok(StatementOutcome::Affected { rows });
    }
    if let Ok(cursor) = reply.get_document("cursor") {
        return Ok(StatementOutcome::Rows(cursor_batch(cursor, row_limit)));
    }
    for noise in REPLY_NOISE {
        reply.remove(noise);
    }
    if reply.is_empty() {
        return Ok(StatementOutcome::Completed {
            message: format!("{command_name}: ok"),
        });
    }
    Ok(StatementOutcome::Rows(document_rows(
        [to_display_fields(reply)],
        false,
    )))
}

fn reject_write_errors(reply: &Document) -> Result<(), DriverError> {
    if let Some(first) = reply
        .get_array("writeErrors")
        .ok()
        .and_then(|errors| errors.first())
        .and_then(Bson::as_document)
    {
        let count = reply.get_array("writeErrors").map_or(0, Vec::len);
        let written = count_field(reply, "n").unwrap_or(0);
        return Err(
            DriverError::query(error_message(first)).with_detail(format!(
                "{count} write error(s), {written} document(s) written"
            )),
        );
    }
    if let Ok(concern) = reply.get_document("writeConcernError") {
        return Err(DriverError::query(error_message(concern)).with_detail("write concern error"));
    }
    Ok(())
}

fn error_message(error: &Document) -> String {
    let message = error.get_str("errmsg").unwrap_or("write failed");
    match error.get("code").and_then(bson_number) {
        Some(code) => format!("{message} (code {code})"),
        None => message.to_owned(),
    }
}

fn affected_documents(command_name: &str, reply: &Document) -> Option<u64> {
    match command_name {
        "insert" | "delete" => count_field(reply, "n"),
        "update" => count_field(reply, "nModified"),
        _ => None,
    }
}

fn cursor_batch(cursor: &Document, row_limit: usize) -> relix_core::RowBatch {
    let batch = cursor
        .get_array("firstBatch")
        .or_else(|_| cursor.get_array("nextBatch"))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let open = cursor.get("id").and_then(bson_number).unwrap_or(0) != 0;
    let rows = batch
        .iter()
        .take(row_limit)
        .filter_map(Bson::as_document)
        .map(|document| to_display_fields(document.clone()));
    document_rows(rows, open || batch.len() > row_limit)
}

fn count_field(reply: &Document, field: &str) -> Option<u64> {
    reply
        .get(field)
        .and_then(bson_number)
        .and_then(|count| u64::try_from(count).ok())
}

pub fn bson_number(value: &Bson) -> Option<i64> {
    match value {
        Bson::Int32(number) => Some(i64::from(*number)),
        Bson::Int64(number) => Some(*number),
        Bson::Double(number) if number.fract() == 0.0 => Some(*number as i64),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;
    use relix_core::Value;

    #[test]
    fn write_commands_report_affected_documents() {
        let outcome = reply_outcome("update", doc! { "n": 3, "nModified": 2, "ok": 1.0 }, 10);
        assert_eq!(outcome.unwrap(), StatementOutcome::Affected { rows: 2 });
        let outcome = reply_outcome("insert", doc! { "n": 1, "ok": 1.0 }, 10);
        assert_eq!(outcome.unwrap(), StatementOutcome::Affected { rows: 1 });
    }

    #[test]
    fn partial_write_failures_are_errors() {
        let reply = doc! {
            "n": 1,
            "writeErrors": [{ "index": 1, "code": 11000, "errmsg": "E11000 duplicate key" }],
            "ok": 1.0,
        };
        let error = reply_outcome("insert", reply, 10).unwrap_err();
        assert!(error.message.contains("E11000"));
        assert!(error.detail.unwrap().contains("1 document(s) written"));
    }

    #[test]
    fn plain_replies_become_a_single_row_or_a_message() {
        let StatementOutcome::Rows(batch) =
            reply_outcome("buildInfo", doc! { "version": "8.0.0", "ok": 1.0 }, 10).unwrap()
        else {
            panic!("expected rows");
        };
        assert_eq!(batch.rows, vec![vec![Value::Text("8.0.0".into())]]);
        let outcome = reply_outcome("drop", doc! { "ok": 1.0 }, 10).unwrap();
        assert!(matches!(outcome, StatementOutcome::Completed { .. }));
    }

    #[test]
    fn unexpected_cursor_batches_are_truncated() {
        let reply = doc! {
            "cursor": { "id": 0_i64, "nextBatch": [{ "a": 1 }, { "a": 2 }] },
            "ok": 1.0,
        };
        let StatementOutcome::Rows(batch) = reply_outcome("getMore", reply, 1).unwrap() else {
            panic!("expected rows");
        };
        assert_eq!(batch.rows.len(), 1);
        assert!(batch.has_more);
    }
}
