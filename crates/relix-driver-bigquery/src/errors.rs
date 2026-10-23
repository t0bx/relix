/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::{StatusCode, char_offset, map_status_error};
use serde::Deserialize;

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorBody,
}

#[derive(Deserialize)]
struct ErrorBody {
    message: String,
    #[serde(default)]
    errors: Vec<ErrorItem>,
}

#[derive(Deserialize)]
struct ErrorItem {
    #[serde(default)]
    reason: Option<String>,
}

/// Maps a Google API error (`{"error": {"code", "message", "errors": [{"reason"}]}}`) to
/// a driver error. Cancelled jobs report the reason `stopped`; query errors carry the
/// position as `[line:column]` (1-based), which is converted for the editor.
pub(crate) fn failure(status: StatusCode, body: &str, statement: Option<&str>) -> DriverError {
    let Ok(envelope) = serde_json::from_str::<ErrorEnvelope>(body) else {
        return map_status_error(status.as_u16(), body);
    };
    let reason = envelope
        .error
        .errors
        .iter()
        .find_map(|item| item.reason.clone())
        .unwrap_or_default();
    let kind = match (status.as_u16(), reason.as_str()) {
        (_, "stopped") => ErrorKind::Cancelled,
        (408 | 504, _) | (_, "timeout") => ErrorKind::Timeout,
        (401, _) => ErrorKind::Authentication,
        (400..=499, _) => ErrorKind::Query,
        _ => ErrorKind::Internal,
    };
    let mut detail = format!("HTTP {}", status.as_u16());
    if !reason.is_empty() {
        detail.push_str(&format!(", reason {reason}"));
    }
    let message = envelope.error.message;
    let mut error = DriverError::new(kind, message.trim()).with_detail(detail);
    if let Some(position) = statement.and_then(|statement| position(statement, &message)) {
        error = error.with_position(position);
    }
    error
}

pub(crate) fn unexpected_reply(error: serde_json::Error) -> DriverError {
    DriverError::new(
        ErrorKind::Protocol,
        "unexpected response from the BigQuery API",
    )
    .with_detail(error.to_string())
}

fn position(statement: &str, message: &str) -> Option<usize> {
    message.match_indices('[').find_map(|(index, _)| {
        let inside = message[index + 1..].split(']').next()?;
        let location = inside.strip_prefix("at ").unwrap_or(inside);
        let (line, column) = location.split_once(':')?;
        let column: usize = column.trim().parse().ok()?;
        char_offset(statement, line.trim().parse().ok()?, column.checked_sub(1)?)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_errors_point_at_the_reported_location() {
        let body = r#"{"error":{"code":400,"message":"Syntax error: Unexpected identifier \"FORM\" at [2:1]","errors":[{"message":"x","domain":"global","reason":"invalidQuery","location":"q","locationType":"other"}],"status":"INVALID_ARGUMENT"}}"#;
        let error = failure(StatusCode::BAD_REQUEST, body, Some("SELECT *\nFORM t"));
        assert_eq!(error.kind, ErrorKind::Query);
        assert_eq!(error.position, Some(9));
        assert!(error.detail.unwrap_or_default().contains("invalidQuery"));
        let emulator = r#"{"error":{"code":400,"message":"Syntax error: Unexpected identifier \"SELEC\" [at 1:1]","errors":[{"reason":"jobInternalError"}]}}"#;
        assert_eq!(
            failure(StatusCode::BAD_REQUEST, emulator, Some("SELEC 1")).position,
            Some(0)
        );
    }

    #[test]
    fn stopped_jobs_and_credentials_map_to_their_kinds() {
        let stopped = r#"{"error":{"code":400,"message":"Job execution was cancelled: User requested cancellation","errors":[{"reason":"stopped"}]}}"#;
        assert_eq!(
            failure(StatusCode::BAD_REQUEST, stopped, None).kind,
            ErrorKind::Cancelled
        );
        let unauthorized = r#"{"error":{"code":401,"message":"Request had invalid authentication credentials.","errors":[{"reason":"authError"}],"status":"UNAUTHENTICATED"}}"#;
        assert_eq!(
            failure(StatusCode::UNAUTHORIZED, unauthorized, None).kind,
            ErrorKind::Authentication
        );
        assert_eq!(
            failure(StatusCode::BAD_GATEWAY, "<html/>", None).kind,
            ErrorKind::Internal
        );
    }
}
