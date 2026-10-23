/*
 * @author t0bx
 * @since 23.10.2026
 */

use aws_sdk_dynamodb::error::{DisplayErrorContext, ProvideErrorMetadata, SdkError};
use relix_driver_api::{DriverError, ErrorKind};
use std::error::Error;
use std::fmt::Debug;

pub const CONDITIONAL_CHECK_FAILED: &str = "ConditionalCheckFailedException";
const AUTHENTICATION_CODES: [&str; 6] = [
    "UnrecognizedClientException",
    "InvalidSignatureException",
    "ExpiredTokenException",
    "MissingAuthenticationTokenException",
    "IncompleteSignatureException",
    "InvalidClientTokenId",
];
const CREDENTIAL_MARKERS: [&str; 2] = ["credential", "identity"];
const TLS_MARKERS: [&str; 3] = ["certificate", "tls", "handshake"];

pub fn map_sdk_error<E, R>(error: SdkError<E, R>) -> DriverError
where
    E: ProvideErrorMetadata + Error + Send + Sync + 'static,
    R: Debug,
{
    let text = DisplayErrorContext(&error).to_string();
    match &error {
        SdkError::ServiceError(service) => {
            let code = service.err().code().unwrap_or("UnknownError");
            let message = service.err().message().unwrap_or(&text).to_owned();
            DriverError::new(service_kind(code), message).with_detail(code.to_owned())
        }
        SdkError::TimeoutError(_) => DriverError::new(ErrorKind::Timeout, text),
        SdkError::DispatchFailure(failure) if failure.is_timeout() => {
            DriverError::new(ErrorKind::Timeout, text)
        }
        SdkError::DispatchFailure(_) | SdkError::ConstructionFailure(_) => {
            DriverError::new(transport_kind(&text), text)
        }
        SdkError::ResponseError(_) => DriverError::new(ErrorKind::Protocol, text),
        _ => DriverError::internal(text),
    }
}

pub fn is_conditional_failure<E, R>(error: &SdkError<E, R>) -> bool
where
    E: ProvideErrorMetadata,
{
    error.code() == Some(CONDITIONAL_CHECK_FAILED)
}

pub fn connection_closed() -> DriverError {
    DriverError::connection("the DynamoDB connection is closed")
}

fn service_kind(code: &str) -> ErrorKind {
    if AUTHENTICATION_CODES.contains(&code) {
        ErrorKind::Authentication
    } else if code == CONDITIONAL_CHECK_FAILED {
        ErrorKind::ConcurrentModification
    } else {
        ErrorKind::Query
    }
}

/// Failures before a response arrives carry their cause only as text: missing or
/// unusable credentials are reported as authentication problems so the UI asks for the
/// secret key, certificate problems as TLS errors, everything else as lost connectivity.
fn transport_kind(text: &str) -> ErrorKind {
    let lower = text.to_lowercase();
    if CREDENTIAL_MARKERS
        .iter()
        .any(|marker| lower.contains(marker))
    {
        ErrorKind::Authentication
    } else if TLS_MARKERS.iter().any(|marker| lower.contains(marker)) {
        ErrorKind::Tls
    } else {
        ErrorKind::Connection
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_codes_map_to_error_kinds() {
        assert_eq!(
            service_kind("UnrecognizedClientException"),
            ErrorKind::Authentication
        );
        assert_eq!(
            service_kind(CONDITIONAL_CHECK_FAILED),
            ErrorKind::ConcurrentModification
        );
        assert_eq!(service_kind("ValidationException"), ErrorKind::Query);
        assert_eq!(service_kind("AccessDeniedException"), ErrorKind::Query);
    }

    #[test]
    fn transport_failures_are_classified_by_their_cause() {
        assert_eq!(
            transport_kind("failed to load credentials from the chain"),
            ErrorKind::Authentication
        );
        assert_eq!(transport_kind("invalid peer certificate"), ErrorKind::Tls);
        assert_eq!(transport_kind("connection refused"), ErrorKind::Connection);
    }
}
