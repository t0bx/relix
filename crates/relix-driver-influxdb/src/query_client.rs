/*
 * @author t0bx
 * @since 24.10.2026
 */

use crate::json_lines::{Record, read_records};
use crate::query_language::QueryLanguage;
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::{HttpEndpoint, Method};
use serde_json::json;

pub(crate) async fn query_records(
    endpoint: &HttpEndpoint,
    language: QueryLanguage,
    database: &str,
    statement: &str,
    limit: usize,
) -> Result<(Vec<Record>, bool), DriverError> {
    let body = json!({"db": database, "q": statement, "format": "jsonl"});
    let request = endpoint
        .request(Method::POST, language.endpoint())
        .json(&body);
    let response = endpoint
        .send(request)
        .await
        .map_err(|error| with_position(error, statement))?;
    read_records(response, limit).await
}

/// Moves the `Line: L, Column: C` location of a parser error into the error position
/// (a character offset in the statement) so the editor can underline it.
fn with_position(error: DriverError, statement: &str) -> DriverError {
    if error.kind != ErrorKind::Query {
        return error;
    }
    match parser_offset(&error.message, statement) {
        Some(offset) => error.with_position(offset),
        None => error,
    }
}

fn parser_offset(message: &str, statement: &str) -> Option<usize> {
    let number_after = |label: &str| -> Option<usize> {
        let start = message.find(label)? + label.len();
        message[start..]
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .ok()
    };
    let line = number_after("Line:")?;
    let column = number_after("Column:")?;
    let preceding: usize = statement
        .split('\n')
        .take(line.checked_sub(1)?)
        .map(|text| text.chars().count() + 1)
        .sum();
    Some(preceding + column.checked_sub(1)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_locations_become_offsets() {
        let message = "SQL error: ParserError(\"Expected: an SQL statement, found: SELEC at Line: 2, Column: 3\")";
        assert_eq!(parser_offset(message, "-- x\n  SELEC 1"), Some(7));
        assert_eq!(parser_offset("table not found", "SELECT"), None);
        let error = with_position(DriverError::query(message), "-- x\n  SELEC 1");
        assert_eq!(error.position, Some(7));
    }
}
