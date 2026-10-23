/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::protocol::{ErrorLocation, QueryError};
use relix_driver_api::{DriverError, ErrorKind};

pub(crate) fn map_query_error(error: QueryError, sql: &str) -> DriverError {
    let kind = match (error.error_name.as_deref(), error.error_type.as_deref()) {
        (Some("USER_CANCELED"), _) => ErrorKind::Cancelled,
        (Some("EXCEEDED_TIME_LIMIT" | "ABANDONED_QUERY"), _) => ErrorKind::Timeout,
        (_, Some("INTERNAL_ERROR")) => ErrorKind::Internal,
        _ => ErrorKind::Query,
    };
    let mapped = DriverError::new(kind, error.message);
    let mapped = match error.error_name {
        Some(name) => mapped.with_detail(name),
        None => mapped,
    };
    match error
        .error_location
        .and_then(|location| position(location, sql))
    {
        Some(position) => mapped.with_position(position),
        None => mapped,
    }
}

/// Converts Trino's 1-based line and column into a 0-based character offset.
fn position(location: ErrorLocation, sql: &str) -> Option<usize> {
    let line_index = location.line_number.checked_sub(1)?;
    let column = location.column_number.checked_sub(1)?;
    let preceding: usize = sql
        .split_inclusive('\n')
        .take(line_index)
        .map(|line| line.chars().count())
        .sum();
    let line = sql.split_inclusive('\n').nth(line_index)?;
    (column <= line.chars().count()).then_some(preceding + column)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error(name: &str, kind: &str, line: usize, column: usize) -> QueryError {
        QueryError {
            message: "line 2:3: mismatched input".into(),
            error_name: Some(name.into()),
            error_type: Some(kind.into()),
            error_location: Some(ErrorLocation {
                line_number: line,
                column_number: column,
            }),
        }
    }

    #[test]
    fn locations_become_character_positions() {
        let mapped = map_query_error(
            error("SYNTAX_ERROR", "USER_ERROR", 2, 3),
            "SELECT 'ä'\nFRM x",
        );
        assert_eq!(mapped.kind, ErrorKind::Query);
        assert_eq!(mapped.position, Some(13));
        assert_eq!(mapped.detail.as_deref(), Some("SYNTAX_ERROR"));
    }

    #[test]
    fn cancellation_and_internal_failures_are_classified() {
        let cancelled = map_query_error(error("USER_CANCELED", "USER_ERROR", 0, 0), "x");
        assert_eq!(cancelled.kind, ErrorKind::Cancelled);
        assert_eq!(cancelled.position, None);
        let internal =
            map_query_error(error("GENERIC_INTERNAL_ERROR", "INTERNAL_ERROR", 1, 1), "x");
        assert_eq!(internal.kind, ErrorKind::Internal);
    }
}
