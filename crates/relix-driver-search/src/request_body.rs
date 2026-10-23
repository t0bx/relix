/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::request::char_position;
use relix_driver_api::DriverError;
use serde::de::IgnoredAny;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestBody {
    pub content_type: &'static str,
    pub text: String,
}

/// Validates the body that starts at `body_start` in the statement. A JSON body is sent
/// exactly as written (so large numbers keep their precision); NDJSON bodies accept
/// pretty-printed values and send each one compacted onto its own line. Syntax errors
/// carry their position in the statement.
pub fn parse_body(
    statement: &str,
    body_start: usize,
    ndjson: bool,
) -> Result<Option<RequestBody>, DriverError> {
    let rest = statement.get(body_start..).unwrap_or_default();
    let text = rest.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let text_start = body_start + (rest.len() - rest.trim_start().len());
    let syntax_error = |error: serde_json::Error| {
        let byte = text_start + line_column_offset(text, error.line(), error.column());
        DriverError::query(format!("invalid JSON body: {error}"))
            .with_position(char_position(statement, byte))
    };
    if !ndjson {
        serde_json::from_str::<IgnoredAny>(text).map_err(syntax_error)?;
        return Ok(Some(RequestBody {
            content_type: "application/json",
            text: text.to_owned(),
        }));
    }
    let mut lines = String::with_capacity(text.len() + 1);
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<IgnoredAny>();
    let mut start = 0;
    while let Some(item) = stream.next() {
        item.map_err(syntax_error)?;
        let end = stream.byte_offset();
        lines.push_str(&compact(&text[start..end]));
        lines.push('\n');
        start = end;
    }
    Ok(Some(RequestBody {
        content_type: "application/x-ndjson",
        text: lines,
    }))
}

fn line_column_offset(text: &str, line: usize, column: usize) -> usize {
    let line_start: usize = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(str::len)
        .sum();
    (line_start + column.saturating_sub(1)).min(text.len())
}

fn compact(json: &str) -> String {
    let mut compacted = String::with_capacity(json.len());
    let mut in_string = false;
    let mut escaped = false;
    for ch in json.chars() {
        if in_string {
            compacted.push(ch);
            match ch {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
        } else if ch == '"' {
            in_string = true;
            compacted.push(ch);
        } else if !ch.is_whitespace() {
            compacted.push(ch);
        }
    }
    compacted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bodies_are_omitted() {
        assert_eq!(parse_body("GET /\n   \n", 6, false).unwrap(), None);
    }

    #[test]
    fn json_bodies_keep_their_exact_text() {
        let statement = "PUT /a/_doc/1\n{\"n\": 123456789012345678901234567890}\n";
        let body = parse_body(statement, 14, false).unwrap().unwrap();
        assert_eq!(body.text, "{\"n\": 123456789012345678901234567890}");
    }

    #[test]
    fn syntax_errors_carry_the_statement_position() {
        let statement = "POST /a/_search\n{\n  \"query\": {\"match_all\": {}\n}";
        let error = parse_body(statement, 16, false).unwrap_err();
        let position = error.position.unwrap();
        assert!(
            position >= 16 && position <= statement.chars().count(),
            "{position}"
        );
        assert!(parse_body("POST _bulk\n{\"a\":1}\n{bad}\n", 11, true).is_err());
    }

    #[test]
    fn compaction_keeps_string_whitespace() {
        assert_eq!(
            compact("{ \"a b\" : \"c \\\" d\" ,\n \"e\": [1, 2] }"),
            "{\"a b\":\"c \\\" d\",\"e\":[1,2]}"
        );
    }
}
