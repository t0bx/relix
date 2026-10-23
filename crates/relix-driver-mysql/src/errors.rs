/*
 * @author t0bx
 * @since 23.10.2026
 */

use mysql_async::{DriverError as MySqlDriverError, Error, IoError, ServerError};
use relix_driver_api::{DriverError, ErrorKind};

const ACCESS_DENIED_FOR_DATABASE: u16 = 1044;
const ACCESS_DENIED_FOR_USER: u16 = 1045;
const ACCESS_DENIED_NO_PASSWORD: u16 = 1698;
const UNKNOWN_DATABASE: u16 = 1049;
const SYNTAX_ERROR: u16 = 1064;
const LOCK_WAIT_TIMEOUT: u16 = 1205;
const QUERY_INTERRUPTED: u16 = 1317;
const CONNECTION_KILLED: u16 = 1927;
const MAX_EXECUTION_TIME_EXCEEDED: u16 = 3024;
const STATEMENT_TIMEOUT: u16 = 1969;

pub fn map_error(error: Error) -> DriverError {
    match error {
        Error::Server(server) => map_server_error(&server),
        Error::Io(IoError::Tls(tls)) => DriverError::new(ErrorKind::Tls, tls.to_string()),
        Error::Io(IoError::Io(io)) => map_io_error(&io),
        Error::Driver(driver) => map_driver_error(&driver),
        Error::Url(url) => DriverError::configuration(url.to_string()),
        Error::Other(other) => DriverError::internal(other.to_string()),
    }
}

/// Maps an error raised by a user statement and, for syntax errors, derives the 0-based
/// character offset from the `near '...' at line N` fragment of the server message.
pub fn map_statement_error(error: Error, statement: &str) -> DriverError {
    let position = match &error {
        Error::Server(server) if server.code == SYNTAX_ERROR => {
            syntax_error_position(statement, &server.message)
        }
        _ => None,
    };
    let mapped = map_error(error);
    match position {
        Some(position) => mapped.with_position(position),
        None => mapped,
    }
}

pub fn closed_connection() -> DriverError {
    DriverError::connection("connection is closed")
}

fn map_io_error(error: &std::io::Error) -> DriverError {
    let is_tls = error
        .get_ref()
        .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some());
    let kind = if is_tls {
        ErrorKind::Tls
    } else {
        ErrorKind::Connection
    };
    DriverError::new(kind, error.to_string())
}

fn map_server_error(server: &ServerError) -> DriverError {
    let kind = match server.code {
        ACCESS_DENIED_FOR_DATABASE | ACCESS_DENIED_FOR_USER | ACCESS_DENIED_NO_PASSWORD => {
            ErrorKind::Authentication
        }
        QUERY_INTERRUPTED => ErrorKind::Cancelled,
        CONNECTION_KILLED => ErrorKind::Connection,
        LOCK_WAIT_TIMEOUT | MAX_EXECUTION_TIME_EXCEEDED | STATEMENT_TIMEOUT => ErrorKind::Timeout,
        UNKNOWN_DATABASE => ErrorKind::Configuration,
        _ => ErrorKind::Query,
    };
    DriverError::new(kind, server.message.clone())
        .with_detail(format!("SQLSTATE {} (error {})", server.state, server.code))
}

fn map_driver_error(error: &MySqlDriverError) -> DriverError {
    let kind = match error {
        MySqlDriverError::ConnectionClosed | MySqlDriverError::PoolDisconnected => {
            ErrorKind::Connection
        }
        MySqlDriverError::NoClientSslFlagFromServer | MySqlDriverError::NoKeyFound => {
            ErrorKind::Tls
        }
        MySqlDriverError::UnknownAuthPlugin { .. }
        | MySqlDriverError::CleartextPluginDisabled
        | MySqlDriverError::MysqlOldPasswordDisabled
        | MySqlDriverError::InvalidParsecSalt => ErrorKind::Authentication,
        MySqlDriverError::Params(_)
        | MySqlDriverError::MixedParams
        | MySqlDriverError::StmtParamsMismatch { .. }
        | MySqlDriverError::StmtParamsNumberExceedsLimit { .. }
        | MySqlDriverError::NestedTransaction
        | MySqlDriverError::ReadOnlyTransNotSupported
        | MySqlDriverError::LocalInfile(_) => ErrorKind::Query,
        MySqlDriverError::FromValue { .. } | MySqlDriverError::FromRow { .. } => {
            ErrorKind::Internal
        }
        MySqlDriverError::NamedPipesDisabled => ErrorKind::Unsupported,
        _ => ErrorKind::Protocol,
    };
    DriverError::new(kind, error.to_string())
}

fn syntax_error_position(statement: &str, message: &str) -> Option<usize> {
    let (fragment, line) = mysql_near_fragment(message).or_else(|| tidb_near_fragment(message))?;
    let line_start = if line <= 1 {
        0
    } else {
        statement
            .match_indices('\n')
            .nth(line - 2)
            .map(|(index, _)| index + 1)?
    };
    let byte_offset = if fragment.is_empty() {
        statement.trim_end().len()
    } else {
        line_start + statement[line_start..].find(fragment)?
    };
    Some(statement[..byte_offset].chars().count())
}

/// MySQL and MariaDB end syntax errors with `near 'FROMM t' at line 1`.
fn mysql_near_fragment(message: &str) -> Option<(&str, usize)> {
    let start = message.find("near '")? + "near '".len();
    let rest = &message[start..];
    let end = rest.rfind("' at line ")?;
    let line = rest[end + "' at line ".len()..].trim().parse().ok()?;
    Some((&rest[..end], line))
}

/// TiDB ends syntax errors with `line 1 column 14 near "FROMM t" `.
fn tidb_near_fragment(message: &str) -> Option<(&str, usize)> {
    let near = message.rfind(" near \"")?;
    let fragment_start = near + " near \"".len();
    let fragment_end = message.rfind('"').filter(|end| *end >= fragment_start)?;
    let line_start = message[..near].rfind("line ")? + "line ".len();
    let line = message[line_start..near]
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some((&message[fragment_start..fragment_end], line))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_error(code: u16, message: &str) -> Error {
        Error::Server(ServerError {
            code,
            message: message.to_owned(),
            state: "28000".to_owned(),
        })
    }

    #[test]
    fn access_denied_maps_to_authentication() {
        for code in [1044, 1045, 1698] {
            let mapped = map_error(server_error(code, "Access denied for user 'relix'"));
            assert_eq!(mapped.kind, ErrorKind::Authentication);
        }
    }

    #[test]
    fn interrupted_query_maps_to_cancelled() {
        let mapped = map_error(server_error(1317, "Query execution was interrupted"));
        assert_eq!(mapped.kind, ErrorKind::Cancelled);
        assert_eq!(
            mapped.detail.as_deref(),
            Some("SQLSTATE 28000 (error 1317)")
        );
    }

    #[test]
    fn io_and_driver_errors_map_to_connection() {
        let io = Error::Io(IoError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "refused",
        )));
        assert_eq!(map_error(io).kind, ErrorKind::Connection);
        let handshake = Error::Io(IoError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            rustls::Error::NoCertificatesPresented,
        )));
        assert_eq!(map_error(handshake).kind, ErrorKind::Tls);
        let closed = Error::Driver(MySqlDriverError::ConnectionClosed);
        assert_eq!(map_error(closed).kind, ErrorKind::Connection);
        let no_tls = Error::Driver(MySqlDriverError::NoClientSslFlagFromServer);
        assert_eq!(map_error(no_tls).kind, ErrorKind::Tls);
    }

    #[test]
    fn other_server_errors_map_to_query() {
        let mapped = map_error(server_error(1146, "Table 'x.y' doesn't exist"));
        assert_eq!(mapped.kind, ErrorKind::Query);
    }

    #[test]
    fn syntax_error_position_points_at_fragment() {
        let message = "You have an error in your SQL syntax; check the manual that corresponds \
                       to your MySQL server version for the right syntax to use near 'FROMM t' at line 1";
        let mapped = map_statement_error(server_error(1064, message), "SELECT 1 FROMM t");
        assert_eq!(mapped.position, Some(9));
    }

    #[test]
    fn syntax_error_position_respects_line_numbers() {
        let statement = "SELECT x\nFROM t\nWHERE ÿ x";
        let message = "... the right syntax to use near 'x' at line 3";
        assert_eq!(syntax_error_position(statement, message), Some(24));
        let end = "... the right syntax to use near '' at line 1";
        assert_eq!(syntax_error_position("SELECT ", end), Some(6));
    }

    #[test]
    fn tidb_syntax_errors_are_located() {
        let message = "You have an error in your SQL syntax; check the manual that corresponds \
                       to your TiDB version for the right syntax to use line 1 column 14 near \"FROMM t\" ";
        assert_eq!(syntax_error_position("SELECT * FROMM t", message), Some(9));
        let second_line = "... to use line 2 column 10 near \"t\" ";
        assert_eq!(
            syntax_error_position("SELECT 1,\n2 FROMM t", second_line),
            Some(18)
        );
        let end = "... to use line 1 column 7 near \"\" ";
        assert_eq!(syntax_error_position("SELECT ", end), Some(6));
    }
}
