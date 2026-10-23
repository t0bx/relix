/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors_position::error_position;
use relix_driver_api::{DriverError, ErrorKind};

const CONTEXT_MARKER: &str = "\nLINE ";

pub(crate) fn map_error(error: duckdb::Error) -> DriverError {
    match error {
        duckdb::Error::DuckDBFailure(_, Some(message)) => map_engine_message(&message),
        duckdb::Error::DuckDBFailure(failure, None) => DriverError::query(failure.to_string()),
        duckdb::Error::MultipleStatement => {
            DriverError::query("only one statement can be executed at a time")
        }
        other => DriverError::query(other.to_string()),
    }
}

/// Maps an error raised while running `sql` and points at the failing character when
/// DuckDB reported its source context.
pub(crate) fn map_statement_error(error: duckdb::Error, sql: &str) -> DriverError {
    let position = match &error {
        duckdb::Error::DuckDBFailure(_, Some(message)) => error_position(message, sql),
        _ => None,
    };
    let mapped = map_error(error);
    match position {
        Some(position) => mapped.with_position(position),
        None => mapped,
    }
}

pub(crate) fn map_open_error(error: duckdb::Error) -> DriverError {
    let mapped = map_error(error);
    match mapped.kind {
        ErrorKind::Query => DriverError {
            kind: ErrorKind::Connection,
            ..mapped
        },
        _ => mapped,
    }
}

fn map_engine_message(message: &str) -> DriverError {
    let headline = message
        .split_once(CONTEXT_MARKER)
        .map_or(message, |(headline, _)| headline)
        .trim();
    DriverError::new(kind_for_message(headline), headline)
}

fn kind_for_message(message: &str) -> ErrorKind {
    let category = message
        .split_once(" Error:")
        .map_or("", |(category, _)| category);
    match category {
        "INTERRUPT" => ErrorKind::Cancelled,
        "TransactionContext" | "Transaction" if message.contains("Conflict") => {
            ErrorKind::ConcurrentModification
        }
        "FATAL" => ErrorKind::Connection,
        "INTERNAL" | "Out of Memory" => ErrorKind::Internal,
        _ => ErrorKind::Query,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_categories_map_to_error_kinds() {
        assert_eq!(
            kind_for_message("INTERRUPT Error: Interrupted!"),
            ErrorKind::Cancelled
        );
        assert_eq!(
            kind_for_message(
                "TransactionContext Error: Failed to commit: Conflict on tuple deletion!"
            ),
            ErrorKind::ConcurrentModification
        );
        assert_eq!(
            kind_for_message("Catalog Error: Table with name x does not exist!"),
            ErrorKind::Query
        );
        assert_eq!(
            kind_for_message("FATAL Error: database has been invalidated"),
            ErrorKind::Connection
        );
    }

    #[test]
    fn source_context_is_removed_from_the_message() {
        let error = map_engine_message(
            "Parser Error: syntax error at or near \"FRM\"\n\nLINE 1: SELECT 1 FRM t\n                 ^",
        );
        assert_eq!(
            error.message,
            "Parser Error: syntax error at or near \"FRM\""
        );
        assert_eq!(error.kind, ErrorKind::Query);
    }
}
