/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::lexer::{significant, tokenize};
use relix_core::Dialect;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TopLevelWords {
    pub words: Vec<String>,
    pub has_multiple_statements: bool,
}

impl TopLevelWords {
    pub fn first(&self) -> Option<&str> {
        self.words.first().map(String::as_str)
    }
}

/// Collects the upper-cased words of the first statement in `sql` that sit outside any
/// parentheses, in source order. Comments, literals, quoted identifiers and parameters never
/// yield words. Leading empty statements are skipped, and any code after the first
/// statement's top-level `;` marks the text as containing several statements.
pub fn top_level_words(sql: &str, dialect: Dialect) -> TopLevelWords {
    let tokens = tokenize(sql, dialect);
    let mut result = TopLevelWords::default();
    let mut depth = 0usize;
    let mut statement_started = false;
    let mut statement_ended = false;
    for token in significant(&tokens) {
        let is_delimiter = token.is_symbol(";");
        if statement_ended {
            if !is_delimiter {
                result.has_multiple_statements = true;
                break;
            }
            continue;
        }
        if is_delimiter && depth == 0 {
            statement_ended = statement_started;
            continue;
        }
        statement_started = true;
        if token.is_symbol("(") {
            depth += 1;
        } else if token.is_symbol(")") {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && token.kind.is_word() {
            result.words.push(token.text.to_uppercase());
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(sql: &str, dialect: Dialect) -> Vec<String> {
        top_level_words(sql, dialect).words
    }

    #[test]
    fn collects_top_level_words_only() {
        assert_eq!(
            words("select a from (select b from t) x", Dialect::Postgres),
            vec!["SELECT", "A", "FROM", "X"]
        );
    }

    #[test]
    fn skips_comments_strings_and_quoted_identifiers() {
        let sql = "-- delete\n/* update /* nested */ */ insert into \"drop\" values ('select', E'it\\'s')";
        assert_eq!(
            words(sql, Dialect::Postgres),
            vec!["INSERT", "INTO", "VALUES"]
        );
        assert_eq!(
            words(
                "# note\n/* c */ update `select` set a = \"x\"",
                Dialect::MySql
            ),
            vec!["UPDATE", "SET", "A"]
        );
        assert_eq!(
            words("drop table [select]", Dialect::Sqlite),
            vec!["DROP", "TABLE"]
        );
    }

    #[test]
    fn skips_dollar_quoted_bodies_and_parameters() {
        let sql = "create function f() returns int as $body$ select 1; $body$ language sql";
        let scan = top_level_words(sql, Dialect::Postgres);
        assert_eq!(
            scan.words,
            vec![
                "CREATE", "FUNCTION", "F", "RETURNS", "INT", "AS", "LANGUAGE", "SQL"
            ]
        );
        assert!(!scan.has_multiple_statements);
        assert_eq!(words("select $1, $2", Dialect::Postgres), vec!["SELECT"]);
    }

    #[test]
    fn detects_multiple_statements() {
        let multiple = |sql: &str| top_level_words(sql, Dialect::Postgres).has_multiple_statements;
        assert!(!multiple("select 1;  -- trailing\n"));
        assert!(!multiple("select ';'"));
        assert!(!multiple("select 1;;"));
        assert!(multiple("select 1; select 2"));
        assert_eq!(
            words("select 1; delete from t", Dialect::Postgres),
            vec!["SELECT"]
        );
    }

    #[test]
    fn leading_empty_statements_are_skipped() {
        let scan = top_level_words(" ; /* here */ ; delete from t", Dialect::Sqlite);
        assert_eq!(scan.first(), Some("DELETE"));
        assert!(!scan.has_multiple_statements);
        assert_eq!(
            top_level_words(" -- nothing\n ;", Dialect::Sqlite).first(),
            None
        );
    }
}
