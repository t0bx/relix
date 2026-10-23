/*
 * @author t0bx
 * @since 23.10.2026
 */

use mongodb::error::{Error, ErrorKind as MongoErrorKind, WriteFailure};
use relix_driver_api::{DriverError, ErrorKind};

const AUTHENTICATION_FAILED: i32 = 18;
const INTERRUPTED: i32 = 11601;
const MAX_TIME_EXPIRED: i32 = 50;
const DUPLICATE_KEY: i32 = 11000;
const TLS_MARKERS: [&str; 4] = ["certificate", "tls", "handshake", "invalid peer"];

pub fn map_error(error: Error) -> DriverError {
    let text = error.to_string();
    match *error.kind {
        MongoErrorKind::Authentication { message, .. } => {
            DriverError::new(ErrorKind::Authentication, message)
        }
        MongoErrorKind::Command(command) => {
            DriverError::new(command_kind(command.code), command.message)
                .with_detail(format!("{} ({})", command.code_name, command.code))
        }
        MongoErrorKind::Write(WriteFailure::WriteError(write)) => {
            let kind = if write.code == DUPLICATE_KEY {
                ErrorKind::Query
            } else {
                command_kind(write.code)
            };
            let mapped = DriverError::new(kind, write.message);
            match write.code_name {
                Some(name) => mapped.with_detail(format!("{name} ({})", write.code)),
                None => mapped.with_detail(format!("code {}", write.code)),
            }
        }
        MongoErrorKind::Write(WriteFailure::WriteConcernError(concern)) => {
            DriverError::query(concern.message).with_detail(format!(
                "write concern {} ({})",
                concern.code_name, concern.code
            ))
        }
        MongoErrorKind::InvalidArgument { message, .. } => DriverError::query(message),
        MongoErrorKind::InvalidTlsConfig { message, .. } => {
            DriverError::new(ErrorKind::Tls, message)
        }
        MongoErrorKind::ServerSelection { message, .. } => classify_transport(message),
        MongoErrorKind::Io(_) | MongoErrorKind::ConnectionPoolCleared { .. } => {
            classify_transport(text)
        }
        MongoErrorKind::DnsResolve { message, .. } => DriverError::connection(message),
        MongoErrorKind::Shutdown => DriverError::connection("the connection was closed"),
        MongoErrorKind::InvalidResponse { message, .. } => {
            DriverError::new(ErrorKind::Protocol, message)
        }
        MongoErrorKind::IncompatibleServer { message, .. } => DriverError::unsupported(message),
        _ => DriverError::query(text),
    }
}

pub fn connection_closed() -> DriverError {
    DriverError::connection("the MongoDB connection is closed")
}

fn command_kind(code: i32) -> ErrorKind {
    match code {
        AUTHENTICATION_FAILED => ErrorKind::Authentication,
        INTERRUPTED => ErrorKind::Cancelled,
        MAX_TIME_EXPIRED => ErrorKind::Timeout,
        _ => ErrorKind::Query,
    }
}

/// Server selection and I/O failures carry the underlying cause only as text; TLS
/// problems are recognised by their wording so they are reported as [`ErrorKind::Tls`]
/// and authentication failures during the handshake as [`ErrorKind::Authentication`].
fn classify_transport(message: String) -> DriverError {
    let lower = message.to_lowercase();
    let kind = if lower.contains("authentication failed") {
        ErrorKind::Authentication
    } else if TLS_MARKERS.iter().any(|marker| lower.contains(marker)) {
        ErrorKind::Tls
    } else {
        ErrorKind::Connection
    };
    DriverError::new(kind, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_codes_map_to_error_kinds() {
        assert_eq!(command_kind(18), ErrorKind::Authentication);
        assert_eq!(command_kind(11601), ErrorKind::Cancelled);
        assert_eq!(command_kind(50), ErrorKind::Timeout);
        assert_eq!(command_kind(26), ErrorKind::Query);
    }

    #[test]
    fn transport_failures_are_classified_by_their_cause() {
        let tls = classify_transport("invalid peer certificate: UnknownIssuer".into());
        assert_eq!(tls.kind, ErrorKind::Tls);
        let refused = classify_transport("Connection refused (os error 61)".into());
        assert_eq!(refused.kind, ErrorKind::Connection);
    }
}
