/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use rusqlite::ffi::ErrorCode;

pub(crate) fn map_error(error: rusqlite::Error) -> DriverError {
    match error {
        rusqlite::Error::SqliteFailure(failure, message) => DriverError::new(
            kind_for_code(failure.code),
            message.unwrap_or_else(|| failure.to_string()),
        ),
        rusqlite::Error::SqlInputError {
            error,
            msg,
            sql,
            offset,
        } => {
            let mapped = DriverError::new(kind_for_code(error.code), msg);
            match usize::try_from(offset) {
                Ok(offset) => mapped.with_position(char_offset(&sql, offset)),
                Err(_) => mapped,
            }
        }
        rusqlite::Error::MultipleStatement => {
            DriverError::query("only one statement can be executed at a time")
        }
        other => DriverError::query(other.to_string()),
    }
}

fn kind_for_code(code: ErrorCode) -> ErrorKind {
    match code {
        ErrorCode::OperationInterrupted => ErrorKind::Cancelled,
        ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => ErrorKind::Timeout,
        ErrorCode::CannotOpen | ErrorCode::NotADatabase | ErrorCode::DatabaseCorrupt => {
            ErrorKind::Connection
        }
        ErrorCode::SystemIoFailure | ErrorCode::DiskFull => ErrorKind::Io,
        ErrorCode::InternalMalfunction | ErrorCode::OutOfMemory | ErrorCode::ApiMisuse => {
            ErrorKind::Internal
        }
        _ => ErrorKind::Query,
    }
}

fn char_offset(sql: &str, byte_offset: usize) -> usize {
    sql.char_indices()
        .take_while(|(index, _)| *index < byte_offset)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_offsets_become_character_offsets() {
        assert_eq!(char_offset("SELECT 'ä' FRM", 12), 11);
        assert_eq!(char_offset("abc", 10), 3);
    }

    #[test]
    fn syntax_errors_carry_their_position() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        let error = map_error(connection.prepare("SELECT 'ü' FRM t").unwrap_err());
        assert_eq!(error.kind, ErrorKind::Query);
        assert_eq!(error.position, Some(15));
    }

    #[test]
    fn multiple_statements_are_rejected_as_query_errors() {
        let error = map_error(rusqlite::Error::MultipleStatement);
        assert_eq!(error.kind, ErrorKind::Query);
    }
}
