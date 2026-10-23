/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Dialect;
use relix_query::top_level_words;

const CREATE_MODIFIERS: [&str; 4] = ["OR", "REPLACE", "EDITIONABLE", "NONEDITIONABLE"];
const PLSQL_UNITS: [&str; 6] = [
    "PROCEDURE",
    "FUNCTION",
    "PACKAGE",
    "TRIGGER",
    "TYPE",
    "LIBRARY",
];

/// Prepares editor text for OCI: a trailing SQL*Plus `/` line is dropped, and SQL
/// statements lose their terminating `;` (Oracle rejects it), while PL/SQL blocks and
/// stored program units keep it because `END;` belongs to their syntax.
pub fn executable_text(statement: &str) -> &str {
    let mut text = statement.trim_end();
    if let Some(body) = text.strip_suffix('/')
        && (body.ends_with('\n') || body.trim().is_empty())
    {
        text = body.trim_end();
    }
    if is_plsql(text) {
        return text;
    }
    text.trim_end_matches(';').trim_end()
}

pub fn first_keyword(statement: &str) -> Option<String> {
    top_level_words(statement, Dialect::Oracle)
        .first()
        .map(str::to_owned)
}

fn is_plsql(statement: &str) -> bool {
    let scan = top_level_words(statement, Dialect::Oracle);
    let mut words = scan.words.iter().map(String::as_str);
    match words.next() {
        Some("BEGIN" | "DECLARE") => true,
        Some("CREATE") => words
            .find(|word| !CREATE_MODIFIERS.contains(word))
            .is_some_and(|word| PLSQL_UNITS.contains(&word)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_statements_lose_their_terminator() {
        assert_eq!(
            executable_text("SELECT 1 FROM dual;  "),
            "SELECT 1 FROM dual"
        );
        assert_eq!(executable_text("DELETE FROM t;\n/\n"), "DELETE FROM t");
        assert_eq!(
            executable_text("SELECT 4/2 FROM dual"),
            "SELECT 4/2 FROM dual"
        );
    }

    #[test]
    fn plsql_keeps_its_end() {
        assert_eq!(executable_text("BEGIN NULL; END;"), "BEGIN NULL; END;");
        assert_eq!(
            executable_text("create or replace editionable procedure p as begin null; end;\n/"),
            "create or replace editionable procedure p as begin null; end;"
        );
        assert_eq!(
            executable_text("CREATE OR REPLACE PACKAGE BODY pkg AS END pkg;"),
            "CREATE OR REPLACE PACKAGE BODY pkg AS END pkg;"
        );
        assert_eq!(
            executable_text("CREATE TABLE t (a INT);"),
            "CREATE TABLE t (a INT)"
        );
    }

    #[test]
    fn first_keyword_skips_comments() {
        assert_eq!(
            first_keyword("-- x\n update t set a = 1").as_deref(),
            Some("UPDATE")
        );
        assert_eq!(first_keyword("  "), None);
    }
}
