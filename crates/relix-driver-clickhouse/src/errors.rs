/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};

pub(crate) const READONLY: u32 = 164;
pub(crate) const SESSION_IS_LOCKED: u32 = 373;
const SYNTAX_ERROR: u32 = 62;
const TIMEOUT_EXCEEDED: u32 = 159;
const QUERY_WAS_CANCELLED: u32 = 394;
const AUTHENTICATION_CODES: [u32; 5] = [192, 193, 194, 195, 516];
const EXCEPTION_MARKER: &str = "Code: ";
const MAX_MESSAGE_CHARS: usize = 2_000;

/// A server-side exception: from an error response, or written into a successful
/// response when the query failed after streaming had started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerException {
    pub(crate) code: Option<u32>,
    pub(crate) message: String,
}

impl ServerException {
    /// Extracts `Code: N. DB::Exception: …` from a response body, which ClickHouse
    /// writes either as plain text or wrapped in a row of the requested JSON format.
    pub(crate) fn from_body(code_header: Option<u32>, body: &str) -> Self {
        let text = body
            .lines()
            .find_map(exception_line)
            .unwrap_or_else(|| body.trim().to_owned());
        Self {
            code: code_header.or_else(|| exception_code(&text)),
            message: text,
        }
    }

    pub(crate) fn from_row_text(text: &str) -> Option<Self> {
        exception_code(text)?;
        text.contains("DB::Exception").then(|| Self {
            code: exception_code(text),
            message: text.to_owned(),
        })
    }

    pub(crate) fn into_error(self, status: Option<u16>, sql: Option<&str>) -> DriverError {
        let kind = self.kind(status);
        let position = match (self.code, sql) {
            (Some(SYNTAX_ERROR), Some(sql)) => syntax_position(&self.message, sql),
            _ => None,
        };
        let error = DriverError::new(kind, readable_message(&self.message));
        match position {
            Some(position) => error.with_position(position),
            None => error,
        }
    }

    fn kind(&self, status: Option<u16>) -> ErrorKind {
        match self.code {
            Some(code) if AUTHENTICATION_CODES.contains(&code) => ErrorKind::Authentication,
            Some(QUERY_WAS_CANCELLED) => ErrorKind::Cancelled,
            Some(TIMEOUT_EXCEEDED) => ErrorKind::Timeout,
            Some(_) => ErrorKind::Query,
            None => match status {
                Some(401 | 403) => ErrorKind::Authentication,
                Some(400..=499) => ErrorKind::Query,
                _ => ErrorKind::Internal,
            },
        }
    }
}

fn exception_line(line: &str) -> Option<String> {
    let start = line.find(EXCEPTION_MARKER)?;
    if let Ok(row) = serde_json::from_str::<Vec<String>>(line.trim())
        && let [text] = row.as_slice()
    {
        return Some(text.clone());
    }
    Some(line[start..].trim().to_owned())
}

fn exception_code(text: &str) -> Option<u32> {
    let rest = text.trim_start().strip_prefix(EXCEPTION_MARKER)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// Drops the `Code: N. DB::Exception: ` prefix and the trailing server version, keeping
/// the message and the error name (`(SYNTAX_ERROR)`).
fn readable_message(text: &str) -> String {
    let without_prefix = text
        .split_once("DB::Exception: ")
        .map_or(text, |(_, rest)| rest);
    let without_version = without_prefix
        .rfind(" (version ")
        .map_or(without_prefix, |index| &without_prefix[..index]);
    without_version
        .trim()
        .chars()
        .take(MAX_MESSAGE_CHARS)
        .collect()
}

/// ClickHouse reports `failed at position N` as a 1-based byte offset.
fn syntax_position(message: &str, sql: &str) -> Option<usize> {
    let rest = message.split_once("failed at position ")?.1;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let byte = digits.parse::<usize>().ok()?.checked_sub(1)?;
    Some(
        sql.char_indices()
            .take_while(|(index, _)| *index < byte)
            .count(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syntax_errors_point_at_the_character() {
        let body = "Code: 62. DB::Exception: Syntax error: failed at position 17 (t): t WHERE. \
                    Expected one of: token. (SYNTAX_ERROR) (version 25.8.33.6 (official build))";
        let error = ServerException::from_body(Some(62), body)
            .into_error(Some(400), Some("SELECT 'ä' FRM t WHERE"));
        assert_eq!(error.kind, ErrorKind::Query);
        assert_eq!(error.position, Some(15));
        assert!(
            error
                .message
                .starts_with("Syntax error: failed at position 17")
        );
        assert!(error.message.ends_with("(SYNTAX_ERROR)"));
    }

    #[test]
    fn authentication_and_cancellation_codes_are_recognised() {
        let denied = ServerException::from_body(
            None,
            "Code: 516. DB::Exception: relix: Authentication failed. (AUTHENTICATION_FAILED)",
        );
        assert_eq!(denied.code, Some(516));
        assert_eq!(
            denied.into_error(Some(403), None).kind,
            ErrorKind::Authentication
        );
        let wrapped = "[\"count()\"]\n[\"UInt64\"]\n[\"Code: 394. DB::Exception: Query was cancelled. \
                       (QUERY_WAS_CANCELLED) (version 25.8)\"]";
        let cancelled = ServerException::from_body(Some(394), wrapped);
        assert_eq!(
            cancelled.message,
            "Code: 394. DB::Exception: Query was cancelled. (QUERY_WAS_CANCELLED) (version 25.8)"
        );
        assert_eq!(
            cancelled.into_error(Some(500), None).kind,
            ErrorKind::Cancelled
        );
    }

    #[test]
    fn plain_failures_fall_back_to_the_status() {
        let error = ServerException::from_body(None, "Bad Gateway").into_error(Some(502), None);
        assert_eq!(error.kind, ErrorKind::Internal);
        assert_eq!(error.message, "Bad Gateway");
        assert!(ServerException::from_row_text("Code: 12 apples").is_none());
    }
}
