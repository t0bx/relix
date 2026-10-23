/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use std::error::Error as StdError;
use tokio_postgres::error::{DbError, ErrorPosition};

const CARET_DIAGRAM_HEADER: &str = "source SQL:\n";

pub(crate) fn map_error(error: tokio_postgres::Error) -> DriverError {
    match error.as_db_error() {
        Some(db_error) => map_db_error(db_error),
        None => map_client_error(&error),
    }
}

pub(crate) fn map_statement_error(error: tokio_postgres::Error) -> DriverError {
    let position = error
        .as_db_error()
        .and_then(|db_error| match db_error.position() {
            Some(ErrorPosition::Original(position)) => zero_based_position(*position),
            Some(ErrorPosition::Internal { .. }) => None,
            None => db_error.detail().and_then(caret_position),
        });
    let mapped = map_error(error);
    match position {
        Some(position) => mapped.with_position(position),
        None => mapped,
    }
}

pub(crate) fn is_tls_failure(error: &tokio_postgres::Error) -> bool {
    error.as_db_error().is_none() && classify_client_error(&error.to_string()) == ErrorKind::Tls
}

pub(crate) fn connection_closed() -> DriverError {
    DriverError::connection("connection is closed")
}

fn map_db_error(error: &DbError) -> DriverError {
    let mapped = DriverError::new(kind_for_sqlstate(error.code().code()), error.message());
    match server_detail(error.detail(), error.hint()) {
        Some(detail) => mapped.with_detail(detail),
        None => mapped,
    }
}

fn map_client_error(error: &tokio_postgres::Error) -> DriverError {
    let summary = error.to_string();
    let mut kind = classify_client_error(&summary);
    if kind == ErrorKind::Connection && caused_by_timeout(error) {
        kind = ErrorKind::Timeout;
    }
    let causes = source_chain(error);
    let message = if causes.is_empty() {
        summary
    } else {
        format!("{summary}: {}", causes.join(": "))
    };
    DriverError::new(kind, message)
}

fn kind_for_sqlstate(code: &str) -> ErrorKind {
    match code {
        "28P01" | "28000" => ErrorKind::Authentication,
        "57014" => ErrorKind::Cancelled,
        "40001" => ErrorKind::ConcurrentModification,
        "57P01" | "57P02" | "57P03" | "53300" => ErrorKind::Connection,
        code if code.starts_with("08") => ErrorKind::Connection,
        _ => ErrorKind::Query,
    }
}

fn classify_client_error(summary: &str) -> ErrorKind {
    match summary {
        "error performing TLS handshake" => ErrorKind::Tls,
        "timeout waiting for server" => ErrorKind::Timeout,
        "error connecting to server"
        | "connection closed"
        | "error communicating with the server" => ErrorKind::Connection,
        "authentication error" => ErrorKind::Authentication,
        "invalid configuration" | "invalid connection string" => ErrorKind::Configuration,
        "unexpected message from server" | "error parsing response from server" => {
            ErrorKind::Protocol
        }
        _ => ErrorKind::Internal,
    }
}

fn server_detail(detail: Option<&str>, hint: Option<&str>) -> Option<String> {
    match (detail, hint) {
        (Some(detail), Some(hint)) => Some(format!("{detail}\nHint: {hint}")),
        (Some(detail), None) => Some(detail.to_owned()),
        (None, Some(hint)) => Some(format!("Hint: {hint}")),
        (None, None) => None,
    }
}

fn zero_based_position(position: u32) -> Option<usize> {
    usize::try_from(position).ok()?.checked_sub(1)
}

/// Reads the error position from a caret diagram in the detail, which is how
/// CockroachDB reports syntax errors instead of the protocol's position field:
/// `source SQL:` followed by the statement up to the failing line and a `^` line.
fn caret_position(detail: &str) -> Option<usize> {
    let (_, diagram) = detail.split_once(CARET_DIAGRAM_HEADER)?;
    let lines: Vec<&str> = diagram.lines().collect();
    let caret_line = lines.iter().position(|line| line.trim_start() == "^")?;
    let column = lines[caret_line]
        .chars()
        .take_while(|ch| *ch != '^')
        .count();
    let preceding: usize = lines[..caret_line.checked_sub(1)?]
        .iter()
        .map(|line| line.chars().count() + 1)
        .sum();
    Some(preceding + column)
}

fn source_chain(error: &tokio_postgres::Error) -> Vec<String> {
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(cause) = source {
        causes.push(cause.to_string());
        source = cause.source();
    }
    causes
}

fn caused_by_timeout(error: &tokio_postgres::Error) -> bool {
    let mut source = error.source();
    while let Some(cause) = source {
        if cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
        {
            return true;
        }
        source = cause.source();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlstates_map_to_error_kinds() {
        assert_eq!(kind_for_sqlstate("28P01"), ErrorKind::Authentication);
        assert_eq!(kind_for_sqlstate("28000"), ErrorKind::Authentication);
        assert_eq!(kind_for_sqlstate("57014"), ErrorKind::Cancelled);
        assert_eq!(kind_for_sqlstate("08006"), ErrorKind::Connection);
        assert_eq!(kind_for_sqlstate("42601"), ErrorKind::Query);
    }

    #[test]
    fn client_errors_are_classified_by_kind() {
        assert_eq!(
            classify_client_error("error performing TLS handshake"),
            ErrorKind::Tls
        );
        assert_eq!(
            classify_client_error("error connecting to server"),
            ErrorKind::Connection
        );
        assert_eq!(
            classify_client_error("timeout waiting for server"),
            ErrorKind::Timeout
        );
        assert_eq!(
            classify_client_error("authentication error"),
            ErrorKind::Authentication
        );
    }

    #[test]
    fn detail_combines_detail_and_hint() {
        assert_eq!(
            server_detail(Some("Key exists."), Some("Use another key.")),
            Some("Key exists.\nHint: Use another key.".to_owned())
        );
        assert_eq!(server_detail(None, None), None);
    }

    #[test]
    fn positions_become_zero_based() {
        assert_eq!(zero_based_position(1), Some(0));
        assert_eq!(zero_based_position(15), Some(14));
        assert_eq!(zero_based_position(0), None);
    }

    #[test]
    fn caret_diagrams_give_a_zero_based_position() {
        assert_eq!(
            caret_position("source SQL:\nSELECT * FORM pg_class\n              ^"),
            Some(14)
        );
        assert_eq!(
            caret_position("source SQL:\nSELECT 1,\n2 FORM pg_class\n       ^"),
            Some(17)
        );
        assert_eq!(caret_position("Key (id)=(1) already exists."), None);
        assert_eq!(caret_position("source SQL:\n^"), None);
    }
}
