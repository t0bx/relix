/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::splitter::StatementSpan;
use relix_core::Dialect;

const HTTP_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "HEAD", "PATCH"];

pub fn split_requests(source: &str, dialect: Dialect) -> Vec<StatementSpan> {
    match dialect {
        Dialect::SearchRequest => split_http_requests(source),
        _ => split_json_commands(source),
    }
}

/// Splits MongoDB console text into top-level JSON documents. Braces inside strings are
/// ignored; text outside documents (such as `use shop`) forms its own line statements,
/// and semicolons or blank lines between documents are separators only.
fn split_json_commands(source: &str) -> Vec<StatementSpan> {
    let mut statements = Vec::new();
    let mut depth = 0usize;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;
    let mut line_start = None;
    for (index, ch) in source.char_indices() {
        if depth > 0 {
            match (in_string, escaped, ch) {
                (true, true, _) => escaped = false,
                (true, false, '\\') => escaped = true,
                (true, false, '"') => in_string = false,
                (true, false, _) => {}
                (false, _, '"') => in_string = true,
                (false, _, '{' | '[') => depth += 1,
                (false, _, '}' | ']') => {
                    depth -= 1;
                    if depth == 0
                        && let Some(begin) = start.take()
                    {
                        push(&mut statements, source, begin, index + ch.len_utf8());
                    }
                }
                _ => {}
            }
            continue;
        }
        match ch {
            '{' => {
                flush_line(&mut statements, source, &mut line_start, index);
                depth = 1;
                start = Some(index);
            }
            '\n' | ';' => flush_line(&mut statements, source, &mut line_start, index),
            ch if ch.is_whitespace() => {}
            _ => {
                if line_start.is_none() {
                    line_start = Some(index);
                }
            }
        }
    }
    if let Some(begin) = start {
        push(&mut statements, source, begin, source.len());
    }
    flush_line(&mut statements, source, &mut line_start, source.len());
    statements
}

fn flush_line(
    statements: &mut Vec<StatementSpan>,
    source: &str,
    line_start: &mut Option<usize>,
    end: usize,
) {
    if let Some(begin) = line_start.take() {
        let text = source[begin..end].trim();
        if !text.starts_with("//") && !text.starts_with('#') {
            push(statements, source, begin, end);
        }
    }
}

/// Splits search consoles into Dev-Tools style requests: a request starts at a line
/// beginning with an HTTP method and owns the following body lines. A method word only
/// starts a new request outside of an unfinished JSON body.
fn split_http_requests(source: &str) -> Vec<StatementSpan> {
    let mut statements = Vec::new();
    let mut current: Option<usize> = None;
    let mut depth = 0i64;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let first_word = trimmed.split_whitespace().next().unwrap_or_default();
        let is_method =
            depth <= 0 && HTTP_METHODS.contains(&first_word.to_ascii_uppercase().as_str());
        if is_method {
            if let Some(begin) = current.take() {
                push(&mut statements, source, begin, offset);
            }
            current = Some(offset);
            depth = 0;
        }
        if current.is_some() && !is_method {
            depth += json_depth_change(line);
        }
        offset += line.len();
    }
    if let Some(begin) = current {
        push(&mut statements, source, begin, source.len());
    }
    statements
}

fn json_depth_change(line: &str) -> i64 {
    let mut change = 0;
    let mut in_string = false;
    let mut escaped = false;
    for ch in line.chars() {
        match (in_string, escaped, ch) {
            (true, true, _) => escaped = false,
            (true, false, '\\') => escaped = true,
            (true, false, '"') => in_string = false,
            (false, _, '"') => in_string = true,
            (false, _, '{' | '[') => change += 1,
            (false, _, '}' | ']') => change -= 1,
            _ => {}
        }
    }
    change
}

fn push(statements: &mut Vec<StatementSpan>, source: &str, begin: usize, end: usize) {
    let slice = &source[begin..end];
    let leading = slice.len() - slice.trim_start().len();
    let text = slice.trim();
    if text.is_empty() {
        return;
    }
    let start = begin + leading;
    statements.push(StatementSpan {
        start,
        end: start + text.len(),
        text: text.to_owned(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(source: &str, dialect: Dialect) -> Vec<String> {
        split_requests(source, dialect)
            .into_iter()
            .map(|span| span.text)
            .collect()
    }

    #[test]
    fn mongo_commands_split_at_top_level_documents() {
        let script = "use shop\n{\"find\": \"orders\", \"filter\": {\"note\": \"a } b\"}};\n\n{\"aggregate\": \"orders\",\n \"pipeline\": [{\"$match\": {}}], \"cursor\": {}}";
        assert_eq!(
            texts(script, Dialect::MongoCommand),
            vec![
                "use shop".to_owned(),
                "{\"find\": \"orders\", \"filter\": {\"note\": \"a } b\"}}".to_owned(),
                "{\"aggregate\": \"orders\",\n \"pipeline\": [{\"$match\": {}}], \"cursor\": {}}"
                    .to_owned(),
            ]
        );
    }

    #[test]
    fn search_requests_own_their_bodies() {
        let script = "# cluster\nGET /\n\nPOST /logs/_search\n{\n  \"query\": {\"match\": {\"message\": \"GET /x\"}}\n}\nDELETE /old-index\n";
        assert_eq!(
            texts(script, Dialect::SearchRequest),
            vec![
                "GET /".to_owned(),
                "POST /logs/_search\n{\n  \"query\": {\"match\": {\"message\": \"GET /x\"}}\n}"
                    .to_owned(),
                "DELETE /old-index".to_owned(),
            ]
        );
    }

    #[test]
    fn spans_point_into_the_source() {
        let script = "  {\"ping\": 1}  ";
        let spans = split_requests(script, Dialect::MongoCommand);
        assert_eq!(&script[spans[0].start..spans[0].end], "{\"ping\": 1}");
    }
}
