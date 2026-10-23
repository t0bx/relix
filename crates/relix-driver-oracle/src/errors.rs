/*
 * @author t0bx
 * @since 23.10.2026
 */

use oracle::Error;
use relix_driver_api::{DriverError, ErrorKind};

const CLIENT_LIBRARY_NOT_FOUND: i32 = 1047;
const NOT_CONNECTED: i32 = 1010;
const CONNECTION_CLOSED: i32 = 1080;

const AUTHENTICATION_ERRORS: [i32; 6] = [1005, 1017, 1045, 28000, 28001, 28009];
const CONFIGURATION_ERRORS: [i32; 3] = [12154, 12514, 12505];
const CONNECTION_ERRORS: [i32; 12] = [
    3113, 3114, 3135, 12170, 12500, 12537, 12541, 12543, 12545, 12547, 12560, 28547,
];
const USER_CANCELLED: i32 = 1013;
const TIMEOUT_ERRORS: [i32; 3] = [30006, 12535, 3156];

pub const INSTANT_CLIENT_HINT: &str = "the Oracle driver uses ODPI-C and needs the 64-bit \
     Oracle Instant Client (Basic or Basic Light) at runtime: download it from \
     https://www.oracle.com/database/technologies/instant-client.html and either add it to \
     the library search path (LD_LIBRARY_PATH on Linux, PATH on Windows) or set the \
     connection property `client_lib_dir` to its directory (required on macOS)";

pub fn map_error(error: Error) -> DriverError {
    if error.dpi_code() == Some(CLIENT_LIBRARY_NOT_FOUND) {
        return DriverError::configuration(format!(
            "Oracle Instant Client not found: {INSTANT_CLIENT_HINT}"
        ))
        .with_detail(error.to_string());
    }
    let kind = error_kind(error.oci_code(), error.dpi_code());
    let mapped = DriverError::new(kind, error.to_string());
    match error.oci_code() {
        Some(code) => mapped.with_detail(format!("ORA-{code:05}")),
        None => mapped,
    }
}

/// Maps an error raised by a user statement; parse errors carry the byte offset of the
/// offending token, which becomes the 0-based character position in the statement.
pub fn map_statement_error(error: Error, statement: &str) -> DriverError {
    let offset = error
        .db_error()
        .map(|db_error| db_error.offset())
        .filter(|offset| *offset > 0)
        .and_then(|offset| usize::try_from(offset).ok());
    let mapped = map_error(error);
    match offset.and_then(|offset| character_position(statement, offset)) {
        Some(position) if mapped.kind == ErrorKind::Query => mapped.with_position(position),
        _ => mapped,
    }
}

pub fn connection_closed() -> DriverError {
    DriverError::connection("connection is closed")
}

fn error_kind(oci_code: Option<i32>, dpi_code: Option<i32>) -> ErrorKind {
    if let Some(code) = dpi_code {
        return match code {
            NOT_CONNECTED | CONNECTION_CLOSED => ErrorKind::Connection,
            _ => ErrorKind::Internal,
        };
    }
    match oci_code {
        Some(code) if AUTHENTICATION_ERRORS.contains(&code) => ErrorKind::Authentication,
        Some(code) if CONFIGURATION_ERRORS.contains(&code) => ErrorKind::Configuration,
        Some(code) if CONNECTION_ERRORS.contains(&code) => ErrorKind::Connection,
        Some(code) if TIMEOUT_ERRORS.contains(&code) => ErrorKind::Timeout,
        Some(USER_CANCELLED) => ErrorKind::Cancelled,
        Some(_) => ErrorKind::Query,
        None => ErrorKind::Internal,
    }
}

fn character_position(statement: &str, byte_offset: usize) -> Option<usize> {
    statement
        .get(..byte_offset)
        .map(|prefix| prefix.chars().count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_codes_map_to_error_kinds() {
        assert_eq!(error_kind(Some(1017), None), ErrorKind::Authentication);
        assert_eq!(error_kind(Some(12514), None), ErrorKind::Configuration);
        assert_eq!(error_kind(Some(12541), None), ErrorKind::Connection);
        assert_eq!(error_kind(Some(1013), None), ErrorKind::Cancelled);
        assert_eq!(error_kind(Some(942), None), ErrorKind::Query);
        assert_eq!(error_kind(None, Some(1080)), ErrorKind::Connection);
        assert_eq!(error_kind(None, None), ErrorKind::Internal);
    }

    #[test]
    fn byte_offsets_become_character_positions() {
        assert_eq!(character_position("SELECT 'ä' FROMM", 12), Some(11));
        assert_eq!(character_position("SELECT", 2), Some(2));
        assert_eq!(character_position("ä", 1), None);
    }
}
