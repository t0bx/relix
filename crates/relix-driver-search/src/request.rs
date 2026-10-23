/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::request_body::{RequestBody, parse_body};
use relix_driver_api::DriverError;
use relix_driver_http::Method;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleRequest {
    pub method: Method,
    pub path: String,
    pub body: Option<RequestBody>,
}

/// Parses one Dev Tools style request: blank lines and lines starting with `#` or `//`
/// before the request line are skipped, the request line is `METHOD /path[?query]`, and
/// everything after it is the body — a single JSON value, or newline-delimited JSON for
/// `_bulk` and `_msearch` endpoints.
pub fn parse_request(statement: &str) -> Result<ConsoleRequest, DriverError> {
    let mut offset = 0;
    let mut request_line = None;
    for line in statement.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        request_line = Some((start + (line.len() - line.trim_start().len()), trimmed));
        break;
    }
    let Some((line_start, line)) = request_line else {
        return Err(DriverError::query(
            "empty request: write METHOD /path, e.g. GET /_cluster/health",
        ));
    };
    let position = char_position(statement, line_start);
    let mut parts = line.split_whitespace();
    let method = parse_method(parts.next().unwrap_or_default())
        .map_err(|error| error.with_position(position))?;
    let path = parts.next().ok_or_else(|| {
        DriverError::query("missing path after the method, e.g. GET /_search")
            .with_position(position)
    })?;
    if parts.next().is_some() {
        return Err(DriverError::query(
            "the request line must be METHOD /path; put the body on the next line",
        )
        .with_position(position));
    }
    let path = normalize_path(path).map_err(|error| error.with_position(position))?;
    let body = parse_body(statement, offset, is_ndjson_endpoint(&path))?;
    if body.is_some() && method == Method::HEAD {
        return Err(DriverError::query("HEAD requests cannot have a body").with_position(position));
    }
    Ok(ConsoleRequest { method, path, body })
}

pub fn char_position(text: &str, byte_offset: usize) -> usize {
    let mut boundary = byte_offset.min(text.len());
    while !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    text[..boundary].chars().count()
}

fn parse_method(word: &str) -> Result<Method, DriverError> {
    match word.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "DELETE" => Ok(Method::DELETE),
        "HEAD" => Ok(Method::HEAD),
        _ => Err(DriverError::query(format!(
            "unknown method \"{word}\": use GET, POST, PUT, DELETE or HEAD"
        ))),
    }
}

/// Keeps the request on the configured server: absolute URLs are refused, and `:` in
/// the path (cross-cluster names such as `remote:logs`) is percent-encoded so it can
/// never be read as a URL scheme.
fn normalize_path(path: &str) -> Result<String, DriverError> {
    if path.contains("://") {
        return Err(DriverError::query(
            "use a path on the connected server, e.g. GET /_cluster/health",
        ));
    }
    let (route, query) = match path.split_once('?') {
        Some((route, query)) => (route, Some(query)),
        None => (path, None),
    };
    let mut normalized = String::with_capacity(path.len() + 4);
    if !route.starts_with('/') {
        normalized.push('/');
    }
    normalized.push_str(&route.replace(':', "%3A"));
    if let Some(query) = query {
        normalized.push('?');
        normalized.push_str(query);
    }
    Ok(normalized.replace('#', "%23"))
}

fn is_ndjson_endpoint(path: &str) -> bool {
    let route = path.split('?').next().unwrap_or_default();
    route
        .split('/')
        .any(|segment| segment == "_bulk" || segment.starts_with("_msearch"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::ErrorKind;

    #[test]
    fn parses_request_lines_and_skips_leading_comments() {
        let request = parse_request("# health\n\nget _cluster/health?level=indices").unwrap();
        assert_eq!(request.method, Method::GET);
        assert_eq!(request.path, "/_cluster/health?level=indices");
        assert_eq!(request.body, None);
    }

    #[test]
    fn json_bodies_follow_the_request_line() {
        let request =
            parse_request("POST /orders/_search\n{\n  \"query\": {\"match_all\": {}}\n}\n")
                .unwrap();
        let body = request.body.unwrap();
        assert_eq!(body.content_type, "application/json");
        assert!(body.text.starts_with('{') && body.text.ends_with('}'));
    }

    #[test]
    fn bulk_bodies_are_sent_as_ndjson() {
        let request =
            parse_request("POST _bulk\n{\"index\": {\"_index\": \"a\"}}\n{\n  \"x\": \"a b\"\n}\n")
                .unwrap();
        let body = request.body.unwrap();
        assert_eq!(body.content_type, "application/x-ndjson");
        assert_eq!(
            body.text,
            "{\"index\":{\"_index\":\"a\"}}\n{\"x\":\"a b\"}\n"
        );
        assert!(is_ndjson_endpoint("/logs/_msearch/template"));
        assert!(!is_ndjson_endpoint("/_bulky/_search"));
    }

    #[test]
    fn invalid_requests_point_at_the_problem() {
        let error = parse_request("\n  FETCH /x").unwrap_err();
        assert_eq!(error.kind, ErrorKind::Query);
        assert_eq!(error.position, Some(3));
        assert!(parse_request("GET").is_err());
        assert!(parse_request("GET /a /b").is_err());
        assert!(parse_request("   \n# only a comment").is_err());
        assert!(parse_request("HEAD /a\n{}").is_err());
    }

    #[test]
    fn paths_cannot_leave_the_server() {
        assert!(parse_request("GET http://evil.example/_search").is_err());
        let remote = parse_request("GET remote:logs/_search?q=a:b").unwrap();
        assert_eq!(remote.path, "/remote%3Alogs/_search?q=a:b");
        assert_eq!(normalize_path("/a#b").unwrap(), "/a%23b");
    }

    #[test]
    fn positions_count_characters() {
        assert_eq!(char_position("äb", 2), 1);
        assert_eq!(char_position("äb", 1), 0);
        assert_eq!(char_position("ab", 99), 2);
    }
}
