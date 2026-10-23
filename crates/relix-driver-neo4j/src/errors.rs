/*
 * @author t0bx
 * @since 24.10.2026
 */

use neo4rs::Error;
use relix_driver_api::{DriverError, ErrorKind};

pub(crate) fn connection_closed() -> DriverError {
    DriverError::connection("connection is closed")
}

pub(crate) fn map_error(error: Error) -> DriverError {
    match error {
        Error::Neo4j(server) => {
            let kind = kind_for_code(server.code());
            DriverError::new(kind, server.message().to_owned())
                .with_detail(server.code().to_owned())
        }
        Error::AuthenticationError(message) => DriverError::new(ErrorKind::Authentication, message),
        Error::IOError { detail } => DriverError::new(ErrorKind::Connection, detail.to_string()),
        Error::ConnectionError => DriverError::connection("cannot connect to the Neo4j server"),
        Error::ConnectionTimedOut => DriverError::new(ErrorKind::Timeout, "connection timed out"),
        Error::UnsupportedVersion(..) | Error::ProtocolMismatch(_) => {
            DriverError::new(ErrorKind::Protocol, error.to_string())
                .with_detail("the server does not speak Bolt 4.x on this port")
        }
        Error::UrlParseError(_) | Error::InvalidDnsName(_) | Error::InvalidConfig => {
            DriverError::configuration(error.to_string())
        }
        Error::DeserializationError(_) | Error::ConversionError | Error::UnknownType(_) => {
            DriverError::new(ErrorKind::Protocol, error.to_string())
        }
        other => DriverError::new(ErrorKind::Internal, other.to_string()),
    }
}

/// Maps an error of a statement typed by the user and moves the `(offset: N)` reported
/// for Cypher syntax errors into the error position so the editor can underline it.
pub(crate) fn map_statement_error(error: Error) -> DriverError {
    let mapped = map_error(error);
    match syntax_offset(&mapped.message) {
        Some(offset) if mapped.kind == ErrorKind::Query => mapped.with_position(offset),
        _ => mapped,
    }
}

fn kind_for_code(code: &str) -> ErrorKind {
    match code {
        "Neo.ClientError.Security.Unauthorized"
        | "Neo.ClientError.Security.AuthenticationRateLimit"
        | "Neo.ClientError.Security.CredentialsExpired" => ErrorKind::Authentication,
        "Neo.ClientError.Transaction.Terminated"
        | "Neo.TransientError.Transaction.Terminated"
        | "Neo.ClientError.Transaction.LockClientStopped"
        | "Neo.TransientError.Transaction.LockClientStopped" => ErrorKind::Cancelled,
        code if code.contains("TransactionTimedOut") => ErrorKind::Timeout,
        code if code.starts_with("Neo.DatabaseError") => ErrorKind::Internal,
        _ => ErrorKind::Query,
    }
}

fn syntax_offset(message: &str) -> Option<usize> {
    let start = message.find("(offset: ")? + "(offset: ".len();
    message[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_codes_map_to_error_kinds() {
        assert_eq!(
            kind_for_code("Neo.ClientError.Security.Unauthorized"),
            ErrorKind::Authentication
        );
        assert_eq!(
            kind_for_code("Neo.ClientError.Statement.SyntaxError"),
            ErrorKind::Query
        );
        assert_eq!(
            kind_for_code("Neo.ClientError.Transaction.Terminated"),
            ErrorKind::Cancelled
        );
        assert_eq!(
            kind_for_code("Neo.ClientError.Transaction.TransactionTimedOutClientConfiguration"),
            ErrorKind::Timeout
        );
        assert_eq!(
            kind_for_code("Neo.DatabaseError.General.UnknownError"),
            ErrorKind::Internal
        );
    }

    #[test]
    fn syntax_offsets_are_extracted() {
        let message = "Invalid input 'RETRN': expected ... (line 1, column 11 (offset: 10))";
        assert_eq!(syntax_offset(message), Some(10));
        assert_eq!(syntax_offset("Unknown function"), None);
    }
}
