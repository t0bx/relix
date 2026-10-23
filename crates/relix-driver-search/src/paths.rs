/*
 * @author t0bx
 * @since 23.10.2026
 */

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use relix_driver_api::DriverError;

const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// Percent-encodes one path segment (an index name or a document id). The dot segments
/// `.` and `..` are rejected because URL normalisation would turn them into a different
/// path, for example `DELETE /index/_doc/..` into `DELETE /index`.
pub fn segment(name: &str) -> Result<String, DriverError> {
    if name.is_empty() || name == "." || name == ".." {
        return Err(DriverError::query(format!(
            "\"{name}\" cannot be addressed in a request path"
        )));
    }
    Ok(utf8_percent_encode(name, SEGMENT).to_string())
}

pub fn concrete_name(name: &str) -> Result<String, DriverError> {
    if name.contains(['*', ',']) || name.starts_with(['_', '-', '+']) {
        return Err(DriverError::query(format!(
            "\"{name}\" is not the name of a single index"
        )));
    }
    segment(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_are_percent_encoded() {
        assert_eq!(segment("logs-2026.10").unwrap(), "logs-2026.10");
        assert_eq!(segment("a/b c?d#e").unwrap(), "a%2Fb%20c%3Fd%23e");
        assert_eq!(segment("remote:logs").unwrap(), "remote%3Alogs");
        assert_eq!(segment("%2e%2e").unwrap(), "%252e%252e");
    }

    #[test]
    fn dot_segments_are_rejected() {
        for name in ["", ".", ".."] {
            assert!(segment(name).is_err());
        }
        assert_eq!(segment("...").unwrap(), "...");
    }

    #[test]
    fn concrete_names_exclude_patterns() {
        for name in ["logs-*", "a,b", "_all", "-logs"] {
            assert!(concrete_name(name).is_err(), "{name}");
        }
        assert_eq!(concrete_name("logs").unwrap(), "logs");
    }
}
