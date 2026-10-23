/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use scylla::errors::{DbError, ExecutionError, NewSessionError, RequestAttemptError};

pub(crate) fn connection_closed() -> DriverError {
    DriverError::connection("connection is closed")
}

pub(crate) fn map_session_error(error: &NewSessionError) -> DriverError {
    let message = error.to_string();
    DriverError::new(classify_message(&message), message)
}

pub(crate) fn map_execution_error(error: &ExecutionError) -> DriverError {
    match error {
        ExecutionError::LastAttemptError(RequestAttemptError::DbError(db_error, message)) => {
            map_db_error(db_error, message)
        }
        ExecutionError::RequestTimeout(_) => {
            DriverError::new(ErrorKind::Timeout, error.to_string())
        }
        ExecutionError::BadQuery(_) => DriverError::query(error.to_string()),
        ExecutionError::ConnectionPoolError(_) | ExecutionError::EmptyPlan => {
            DriverError::connection(error.to_string())
        }
        other => {
            let message = other.to_string();
            DriverError::new(classify_message(&message), message)
        }
    }
}

/// Maps an execution error of a statement typed by the user and, for syntax errors,
/// converts the `line L:C` location reported by the server into a character offset
/// within the statement so the editor can underline it.
pub(crate) fn map_statement_error(error: &ExecutionError, statement: &str) -> DriverError {
    let mapped = map_execution_error(error);
    match syntax_position(&mapped.message, statement) {
        Some(position) if mapped.kind == ErrorKind::Query => mapped.with_position(position),
        _ => mapped,
    }
}

pub(crate) fn protocol_error(error: impl std::fmt::Display) -> DriverError {
    DriverError::new(ErrorKind::Protocol, error.to_string())
}

fn map_db_error(error: &DbError, message: &str) -> DriverError {
    let kind = match error {
        DbError::AuthenticationError => ErrorKind::Authentication,
        DbError::ReadTimeout { .. } | DbError::WriteTimeout { .. } => ErrorKind::Timeout,
        DbError::ProtocolError => ErrorKind::Protocol,
        DbError::ServerError | DbError::Unprepared { .. } => ErrorKind::Internal,
        _ => ErrorKind::Query,
    };
    DriverError::new(kind, message.to_owned()).with_detail(error.to_string())
}

fn classify_message(message: &str) -> ErrorKind {
    let lower = message.to_lowercase();
    if ["authentication", "credentials", "authenticator"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        ErrorKind::Authentication
    } else if lower.contains("timeout") || lower.contains("timed out") {
        ErrorKind::Timeout
    } else if [
        "connect", "broken", "refused", "resolve", "pool", "reset", "io error",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        ErrorKind::Connection
    } else if lower.contains("unsupported") || lower.contains("illegal") {
        ErrorKind::Configuration
    } else {
        ErrorKind::Internal
    }
}

fn syntax_position(message: &str, statement: &str) -> Option<usize> {
    let start = message.find("line ")? + "line ".len();
    let location = &message[start..];
    let (line, rest) = location.split_once(':')?;
    let line: usize = line.trim().parse().ok()?;
    let column: usize = rest
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;
    let preceding: usize = statement
        .split('\n')
        .take(line.checked_sub(1)?)
        .map(|text| text.chars().count() + 1)
        .sum();
    Some(preceding + column)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_are_classified_by_kind() {
        assert_eq!(
            classify_message("Authentication is required but no authenticator provided"),
            ErrorKind::Authentication
        );
        assert_eq!(
            classify_message("Control connection pool error: The pool is broken"),
            ErrorKind::Connection
        );
        assert_eq!(
            classify_message("Connect timeout elapsed"),
            ErrorKind::Timeout
        );
    }

    #[test]
    fn db_errors_keep_the_server_message() {
        let error = map_db_error(&DbError::SyntaxError, "line 1:7 no viable alternative");
        assert_eq!(error.kind, ErrorKind::Query);
        assert_eq!(error.message, "line 1:7 no viable alternative");
        assert_eq!(
            map_db_error(&DbError::AuthenticationError, "bad credentials").kind,
            ErrorKind::Authentication
        );
    }

    #[test]
    fn syntax_locations_become_character_offsets() {
        let statement = "SELECT *\nFRM users";
        assert_eq!(
            syntax_position("line 2:0 no viable alternative at input 'FRM'", statement),
            Some(9)
        );
        assert_eq!(
            syntax_position("line 1:7 mismatched input", "SELECT FROM"),
            Some(7)
        );
        assert_eq!(syntax_position("unconfigured table users", statement), None);
    }
}
