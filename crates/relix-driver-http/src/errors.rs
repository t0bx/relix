/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use std::error::Error as _;

const MAX_ERROR_BODY: usize = 2_000;

pub fn map_transport_error(error: &reqwest::Error) -> DriverError {
    if error.is_timeout() {
        return DriverError::new(ErrorKind::Timeout, "the server did not answer in time");
    }
    let chain = error_chain(error);
    if is_tls_message(&chain) {
        return DriverError::new(ErrorKind::Tls, "TLS handshake failed").with_detail(chain);
    }
    if error.is_connect() {
        return DriverError::connection("cannot connect to the server").with_detail(chain);
    }
    if error.is_decode() || error.is_body() {
        return DriverError::new(ErrorKind::Protocol, "unexpected response from the server")
            .with_detail(chain);
    }
    DriverError::new(ErrorKind::Io, "HTTP request failed").with_detail(chain)
}

/// Turns an HTTP error status into a driver error, keeping a bounded excerpt of the
/// server's explanation. Authentication statuses are reported as such so the UI asks
/// for credentials instead of showing a generic failure.
pub fn map_status_error(status: u16, body: &str) -> DriverError {
    let message = server_message(body);
    let kind = match status {
        401 | 403 => ErrorKind::Authentication,
        408 | 504 => ErrorKind::Timeout,
        409 | 412 => ErrorKind::ConcurrentModification,
        400..=499 => ErrorKind::Query,
        _ => ErrorKind::Internal,
    };
    let headline = match kind {
        ErrorKind::Authentication => "the server rejected the credentials".to_owned(),
        _ => format!("server responded with HTTP {status}"),
    };
    if message.is_empty() {
        DriverError::new(kind, headline)
    } else {
        DriverError::new(kind, message).with_detail(headline)
    }
}

pub fn is_tls_message(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["certificate", "tls", "ssl", "handshake", "invalid peer"]
        .iter()
        .any(|needle| lower.contains(needle))
}

fn error_chain(error: &reqwest::Error) -> String {
    let mut parts = Vec::new();
    let mut current = error.source();
    while let Some(source) = current {
        parts.push(source.to_string());
        current = source.source();
    }
    if parts.is_empty() {
        error.to_string()
    } else {
        parts.join(": ")
    }
}

fn server_message(body: &str) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(body).ok();
    let from_json = parsed.as_ref().and_then(|json| {
        ["message", "error", "reason", "detail"]
            .iter()
            .find_map(|field| match json.get(field) {
                Some(serde_json::Value::String(text)) => Some(text.clone()),
                Some(serde_json::Value::Object(nested)) => nested
                    .get("reason")
                    .or_else(|| nested.get("message"))
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                _ => None,
            })
    });
    let text = from_json.unwrap_or_else(|| body.trim().to_owned());
    text.chars().take(MAX_ERROR_BODY).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_map_to_error_kinds() {
        assert_eq!(map_status_error(401, "").kind, ErrorKind::Authentication);
        assert_eq!(map_status_error(400, "syntax").kind, ErrorKind::Query);
        assert_eq!(map_status_error(503, "").kind, ErrorKind::Internal);
    }

    #[test]
    fn json_error_messages_are_extracted() {
        let error = map_status_error(
            400,
            r#"{"error":{"type":"parsing_exception","reason":"unknown query [mach]"}}"#,
        );
        assert_eq!(error.message, "unknown query [mach]");
        let plain = map_status_error(
            500,
            r#"{"message":"Code: 62. DB::Exception: Syntax error"}"#,
        );
        assert!(plain.message.starts_with("Code: 62"));
        assert_eq!(map_status_error(400, "  raw text  ").message, "raw text");
    }
}
