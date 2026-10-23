/*
 * @author t0bx
 * @since 23.10.2026
 */

use redis::{ParsingError, RedisError};
use relix_driver_api::{DriverError, ErrorKind};
use std::fmt::Display;

const AUTHENTICATION_CODES: [&str; 2] = ["NOAUTH", "WRONGPASS"];
const TLS_MARKERS: [&str; 5] = [
    "certificate",
    "tls",
    "handshake",
    "invalid peer",
    "corrupt message",
];

pub fn map_redis_error(error: RedisError) -> DriverError {
    let kind = classify(&error);
    let mut mapped = DriverError::new(kind, error.to_string());
    if let Some(detail) = error.detail().filter(|detail| !detail.is_empty()) {
        mapped = mapped.with_detail(detail.to_owned());
    }
    mapped
}

/// Maps a failure that happened while establishing a connection. Redis reports TLS
/// failures as plain I/O errors, so when TLS is in use the message is inspected for
/// handshake or certificate problems to report them as [`ErrorKind::Tls`].
pub fn map_connect_error(error: RedisError, tls_enabled: bool) -> DriverError {
    let mut mapped = map_redis_error(error);
    if tls_enabled && is_transport_kind(mapped.kind) && mentions_tls(&mapped.message) {
        mapped.kind = ErrorKind::Tls;
    }
    mapped
}

pub fn map_parsing_error(error: ParsingError) -> DriverError {
    protocol_error(error)
}

pub fn protocol_error(reason: impl Display) -> DriverError {
    DriverError::new(
        ErrorKind::Protocol,
        format!("unexpected reply from Redis: {reason}"),
    )
}

fn classify(error: &RedisError) -> ErrorKind {
    if error.kind() == redis::ErrorKind::AuthenticationFailed
        || error
            .code()
            .is_some_and(|code| AUTHENTICATION_CODES.contains(&code))
    {
        return ErrorKind::Authentication;
    }
    if error.is_timeout() {
        return ErrorKind::Timeout;
    }
    if error.is_connection_refusal() || error.is_connection_dropped() {
        return ErrorKind::Connection;
    }
    match error.kind() {
        redis::ErrorKind::Io => ErrorKind::Connection,
        redis::ErrorKind::InvalidClientConfig => ErrorKind::Configuration,
        redis::ErrorKind::Parse
        | redis::ErrorKind::UnexpectedReturnType
        | redis::ErrorKind::RESP3NotSupported => ErrorKind::Protocol,
        redis::ErrorKind::Server(_) | redis::ErrorKind::Extension => ErrorKind::Query,
        _ => ErrorKind::Internal,
    }
}

fn is_transport_kind(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::Connection | ErrorKind::Io | ErrorKind::Internal
    )
}

fn mentions_tls(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    TLS_MARKERS.iter().any(|marker| lowered.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server_error(code: &str, detail: &str) -> RedisError {
        redis::make_extension_error(code.to_owned(), Some(detail.to_owned()))
    }

    #[test]
    fn authentication_codes_map_to_authentication() {
        let wrong_pass = server_error("WRONGPASS", "invalid username-password pair");
        assert_eq!(map_redis_error(wrong_pass).kind, ErrorKind::Authentication);
        let no_auth = server_error("NOAUTH", "Authentication required.");
        assert_eq!(map_redis_error(no_auth).kind, ErrorKind::Authentication);
    }

    #[test]
    fn other_server_errors_map_to_query() {
        let error = server_error("WRONGTYPE", "Operation against a key");
        let mapped = map_redis_error(error);
        assert_eq!(mapped.kind, ErrorKind::Query);
        assert_eq!(mapped.detail.as_deref(), Some("Operation against a key"));
    }

    #[test]
    fn io_failures_with_tls_markers_become_tls_errors_only_when_tls_is_used() {
        let io = || {
            RedisError::from(std::io::Error::other(
                "invalid peer certificate: UnknownIssuer",
            ))
        };
        assert_eq!(map_connect_error(io(), true).kind, ErrorKind::Tls);
        assert_eq!(map_connect_error(io(), false).kind, ErrorKind::Connection);
    }
}
