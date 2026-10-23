/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::api::models::QueryStatus;
use relix_driver_api::{DriverError, ErrorKind};
use relix_driver_http::{StatusCode, char_offset, map_status_error};

const STATEMENT_CANCELLED: &str = "000604";
const STATEMENT_TIMED_OUT: &str = "000630";
const INVALID_JWT: &str = "390144";

/// Maps an unsuccessful SQL API response to a driver error, keeping Snowflake's error
/// code, SQLSTATE and the statement position of compilation errors.
pub(crate) fn failure(status: StatusCode, body: &str, statement: Option<&str>) -> DriverError {
    let Ok(reply) = serde_json::from_str::<QueryStatus>(body) else {
        return map_status_error(status.as_u16(), body);
    };
    let Some(message) = reply.message.filter(|message| !message.trim().is_empty()) else {
        return map_status_error(status.as_u16(), body);
    };
    let code = reply.code.unwrap_or_default();
    let kind = match (status.as_u16(), code.as_str()) {
        (_, STATEMENT_CANCELLED) => ErrorKind::Cancelled,
        (408, _) | (_, STATEMENT_TIMED_OUT) => ErrorKind::Timeout,
        (401 | 403, _) => ErrorKind::Authentication,
        (400..=499, _) => ErrorKind::Query,
        _ => ErrorKind::Internal,
    };
    let mut error = DriverError::new(kind, message.trim()).with_detail(detail(
        status,
        &code,
        reply.sql_state.as_deref(),
    ));
    if let Some(position) = statement.and_then(|statement| position(statement, &message)) {
        error = error.with_position(position);
    }
    error
}

pub(crate) fn unexpected_reply(error: serde_json::Error) -> DriverError {
    DriverError::new(
        ErrorKind::Protocol,
        "unexpected response from the Snowflake SQL API",
    )
    .with_detail(error.to_string())
}

fn detail(status: StatusCode, code: &str, sql_state: Option<&str>) -> String {
    let mut detail = format!("HTTP {}", status.as_u16());
    if !code.is_empty() {
        detail.push_str(&format!(", Snowflake error {code}"));
    }
    if let Some(state) = sql_state.filter(|state| !state.is_empty()) {
        detail.push_str(&format!(", SQLSTATE {state}"));
    }
    if code == INVALID_JWT {
        detail.push_str(
            "; check the account identifier, the user name and that the user's RSA_PUBLIC_KEY \
             belongs to private_key_file (compare RSA_PUBLIC_KEY_FP in DESC USER)",
        );
    }
    detail
}

/// Snowflake reports compilation errors as `line <n> at position <m>` with a 1-based
/// line and a 0-based column.
fn position(statement: &str, message: &str) -> Option<usize> {
    let after_line = message.split("line ").nth(1)?;
    let (line, rest) = after_line.split_once(" at position ")?;
    let column: String = rest.chars().take_while(char::is_ascii_digit).collect();
    char_offset(statement, line.trim().parse().ok()?, column.parse().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compilation_errors_keep_the_position() {
        let body = r#"{"code":"001003","sqlState":"42000","message":"SQL compilation error:\nsyntax error line 2 at position 0 unexpected 'FORM'.","statementHandle":"h"}"#;
        let error = failure(
            StatusCode::UNPROCESSABLE_ENTITY,
            body,
            Some("SELECT *\nFORM t"),
        );
        assert_eq!(error.kind, ErrorKind::Query);
        assert_eq!(error.position, Some(9));
        assert!(error.detail.unwrap_or_default().contains("SQLSTATE 42000"));
    }

    #[test]
    fn cancellation_timeout_and_authentication_codes() {
        let cancelled = r#"{"code":"000604","message":"SQL execution canceled"}"#;
        assert_eq!(
            failure(StatusCode::UNPROCESSABLE_ENTITY, cancelled, None).kind,
            ErrorKind::Cancelled
        );
        let timeout = r#"{"code":"000630","message":"Statement reached its statement or warehouse timeout of 1 second(s) and was canceled."}"#;
        assert_eq!(
            failure(StatusCode::UNPROCESSABLE_ENTITY, timeout, None).kind,
            ErrorKind::Timeout
        );
        let jwt = r#"{"code":"390144","message":"JWT token is invalid."}"#;
        let error = failure(StatusCode::UNAUTHORIZED, jwt, None);
        assert_eq!(error.kind, ErrorKind::Authentication);
        assert!(
            error
                .detail
                .unwrap_or_default()
                .contains("RSA_PUBLIC_KEY_FP")
        );
    }

    #[test]
    fn non_json_bodies_fall_back_to_status_mapping() {
        let error = failure(StatusCode::BAD_GATEWAY, "<html>bad gateway</html>", None);
        assert_eq!(error.kind, ErrorKind::Internal);
    }
}
