/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use tiberius::error::{Error, IoErrorKind, TokenError};

const LOGIN_FAILED: u32 = 18456;
const LOGIN_FROM_UNTRUSTED_DOMAIN: u32 = 18452;
const ACCOUNT_LOCKED: u32 = 18486;
const PASSWORD_EXPIRED: u32 = 18487;
const PASSWORD_MUST_CHANGE: u32 = 18488;
const CANNOT_OPEN_DATABASE: u32 = 4060;
const LOCK_REQUEST_TIMEOUT: u32 = 1222;
const BATCH_ABORTED_BY_CLIENT: u32 = 3980;
const SESSION_KILLED: u32 = 596;
const FATAL_SEVERITY: u8 = 20;
const TLS_HINTS: [&str; 4] = ["certificate", "tls", "handshake", "ssl"];

pub fn map_error(error: Error) -> DriverError {
    match error {
        Error::Server(token) => map_server_error(&token),
        Error::Io { kind, message } => map_io_error(kind, message),
        Error::Tls(message) => DriverError::new(ErrorKind::Tls, message),
        Error::Protocol(message) | Error::Encoding(message) => {
            DriverError::new(ErrorKind::Protocol, message.into_owned())
        }
        Error::Routing { host, port } => DriverError::connection(format!(
            "the server redirected the connection to {host}:{port}"
        )),
        other => DriverError::internal(other.to_string()),
    }
}

/// Maps an error raised by a user statement and points `position` at the reported line
/// or, when the message quotes the offending token (`Incorrect syntax near 'x'`), at
/// that token within the line.
pub fn map_statement_error(error: Error, statement: &str) -> DriverError {
    let position = match &error {
        Error::Server(token) => error_position(statement, token.line(), token.message()),
        _ => None,
    };
    let mapped = map_error(error);
    match position {
        Some(position) => mapped.with_position(position),
        None => mapped,
    }
}

pub fn connection_closed() -> DriverError {
    DriverError::connection("connection is closed")
}

pub fn connection_lost() -> DriverError {
    DriverError::connection("the session connection was lost")
}

pub fn statement_cancelled() -> DriverError {
    DriverError::new(ErrorKind::Cancelled, "statement cancelled").with_detail(
        "SQL Server sessions are cancelled with KILL: the session was ended, any open \
         transaction was rolled back and the next statement runs in a new session",
    )
}

fn map_io_error(kind: IoErrorKind, message: String) -> DriverError {
    let lowered = message.to_ascii_lowercase();
    let is_tls =
        kind == IoErrorKind::InvalidData && TLS_HINTS.iter().any(|hint| lowered.contains(hint));
    let kind = if is_tls {
        ErrorKind::Tls
    } else {
        ErrorKind::Connection
    };
    DriverError::new(kind, message)
}

fn map_server_error(token: &TokenError) -> DriverError {
    let kind = match token.code() {
        LOGIN_FAILED
        | LOGIN_FROM_UNTRUSTED_DOMAIN
        | ACCOUNT_LOCKED
        | PASSWORD_EXPIRED
        | PASSWORD_MUST_CHANGE => ErrorKind::Authentication,
        CANNOT_OPEN_DATABASE => ErrorKind::Configuration,
        LOCK_REQUEST_TIMEOUT => ErrorKind::Timeout,
        BATCH_ABORTED_BY_CLIENT => ErrorKind::Cancelled,
        SESSION_KILLED => ErrorKind::Connection,
        _ if token.class() >= FATAL_SEVERITY => ErrorKind::Connection,
        _ => ErrorKind::Query,
    };
    DriverError::new(kind, token.message().to_owned()).with_detail(server_detail(token))
}

fn server_detail(token: &TokenError) -> String {
    let mut detail = format!(
        "error {}, severity {}, state {}",
        token.code(),
        token.class(),
        token.state()
    );
    if !token.procedure().is_empty() {
        detail.push_str(&format!(", procedure {}", token.procedure()));
    }
    if token.line() > 0 {
        detail.push_str(&format!(", line {}", token.line()));
    }
    detail
}

fn error_position(statement: &str, line: u32, message: &str) -> Option<usize> {
    let line_index = usize::try_from(line).ok()?.checked_sub(1)?;
    let line_start = if line_index == 0 {
        0
    } else {
        statement
            .match_indices('\n')
            .nth(line_index - 1)
            .map(|(index, _)| index + 1)?
    };
    let line_text = statement[line_start..]
        .split('\n')
        .next()
        .unwrap_or_default();
    let within_line = quoted_fragment(message)
        .and_then(|fragment| line_text.find(fragment))
        .unwrap_or_else(|| line_text.len() - line_text.trim_start().len());
    Some(statement[..line_start + within_line].chars().count())
}

fn quoted_fragment(message: &str) -> Option<&str> {
    let start = message.find("near '")? + "near '".len();
    let rest = &message[start..];
    let fragment = &rest[..rest.rfind('\'')?];
    (!fragment.is_empty()).then_some(fragment)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_failures_map_to_connection_or_tls() {
        let refused = Error::Io {
            kind: IoErrorKind::ConnectionRefused,
            message: "refused".into(),
        };
        assert_eq!(map_error(refused).kind, ErrorKind::Connection);
        let certificate = Error::Io {
            kind: IoErrorKind::InvalidData,
            message: "invalid peer certificate: UnknownIssuer".into(),
        };
        assert_eq!(map_error(certificate).kind, ErrorKind::Tls);
        assert_eq!(map_error(Error::Tls("bad".into())).kind, ErrorKind::Tls);
    }

    #[test]
    fn positions_follow_line_and_quoted_token() {
        let statement = "SELECT 1\nFROMM t";
        assert_eq!(
            error_position(statement, 2, "Incorrect syntax near 'FROMM'."),
            Some(9)
        );
        assert_eq!(
            error_position("SELECT * FROMM t", 1, "Incorrect syntax near 't'."),
            Some(15)
        );
        assert_eq!(
            error_position("  SELECT x", 1, "Invalid column name 'x'."),
            Some(2)
        );
        assert_eq!(error_position("SELECT 1", 3, "boom"), None);
        assert_eq!(error_position("SELECT 1", 0, "boom"), None);
    }

    #[test]
    fn quoted_fragments_are_extracted() {
        assert_eq!(
            quoted_fragment("Incorrect syntax near 'it's'."),
            Some("it's")
        );
        assert_eq!(quoted_fragment("Incorrect syntax near ''."), None);
        assert_eq!(quoted_fragment("no quote"), None);
    }
}
