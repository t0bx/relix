/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::errors::map_error;
use duckdb::types::{Value as DuckValue, ValueRef};
use duckdb::{Connection, Params, Row};
use relix_driver_api::DriverError;

pub(crate) fn query_all<T, P, F>(
    connection: &Connection,
    sql: &str,
    params: P,
    map: F,
) -> Result<Vec<T>, DriverError>
where
    P: Params,
    F: FnMut(&Row<'_>) -> duckdb::Result<T>,
{
    let mut statement = connection.prepare(sql).map_err(map_error)?;
    let rows = statement.query_map(params, map).map_err(map_error)?;
    rows.collect::<duckdb::Result<Vec<T>>>().map_err(map_error)
}

/// Reads a `VARCHAR[]` metadata column; DuckDB versions that expose the same column as
/// rendered text (`[a, 'b c']`) are parsed instead.
pub(crate) fn text_list(value: ValueRef<'_>) -> Vec<String> {
    match value.to_owned() {
        DuckValue::List(items) | DuckValue::Array(items) => items
            .into_iter()
            .filter_map(|item| match item {
                DuckValue::Text(text) => Some(text),
                _ => None,
            })
            .collect(),
        DuckValue::Text(text) => parse_rendered_list(&text),
        _ => Vec::new(),
    }
}

/// Splits DuckDB's textual list form (`[a, 'b, c', 'it\'s']`) into its elements.
fn parse_rendered_list(text: &str) -> Vec<String> {
    let inner = text
        .trim()
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(text);
    let mut items = Vec::new();
    let mut current = String::new();
    let mut chars = inner.chars().peekable();
    let mut depth = 0usize;
    let mut quoted = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\\' if quoted => current.extend(chars.next()),
            '\'' => quoted = !quoted,
            '[' | '(' | '{' if !quoted => {
                depth += 1;
                current.push(ch);
            }
            ']' | ')' | '}' if !quoted => {
                depth = depth.saturating_sub(1);
                current.push(ch);
            }
            ',' if !quoted && depth == 0 => {
                items.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() || !items.is_empty() {
        items.push(current.trim().to_owned());
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_lists_are_split_and_unquoted() {
        assert_eq!(parse_rendered_list("[pid, id]"), vec!["pid", "id"]);
        assert_eq!(
            parse_rendered_list("['((b + 1))', 'it\\'s, odd']"),
            vec!["((b + 1))", "it's, odd"]
        );
        assert!(parse_rendered_list("[]").is_empty());
    }
}
