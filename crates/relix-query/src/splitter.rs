/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::lexer::{Token, TokenKind, tokenize};
use relix_core::Dialect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementSpan {
    pub start: usize,
    pub end: usize,
    pub text: String,
}

/// Splits a script into executable statements without changing their meaning.
///
/// Semicolons inside literals, comments and dollar quotes are ignored. Bodies of
/// `CREATE TRIGGER/FUNCTION/PROCEDURE` statements are kept whole by tracking
/// `BEGIN`/`CASE` … `END` nesting, and MySQL `DELIMITER` directives are honoured.
/// Spans exclude the delimiter and surrounding whitespace; empty statements are dropped.
pub fn split_statements(source: &str, dialect: Dialect) -> Vec<StatementSpan> {
    let tokens = tokenize(source, dialect);
    let mut splitter = Splitter::new(source, dialect);
    let mut index = 0;
    while index < tokens.len() {
        index = splitter.feed(&tokens, index);
    }
    splitter.flush(source.len());
    splitter.statements
}

/// Returns the statement containing `offset`; when the cursor sits between statements the
/// closest preceding statement wins, matching how editors pick the "current" statement.
pub fn statement_at(source: &str, dialect: Dialect, offset: usize) -> Option<StatementSpan> {
    let statements = split_statements(source, dialect);
    let containing = statements
        .iter()
        .find(|statement| offset >= statement.start && offset <= statement.end);
    containing
        .or_else(|| {
            statements
                .iter()
                .rev()
                .find(|statement| statement.end <= offset)
        })
        .or_else(|| statements.first())
        .cloned()
}

struct Splitter<'a> {
    source: &'a str,
    dialect: Dialect,
    delimiter: String,
    statement_start: usize,
    leading_words: Vec<String>,
    block_depth: usize,
    statements: Vec<StatementSpan>,
}

impl<'a> Splitter<'a> {
    fn new(source: &'a str, dialect: Dialect) -> Self {
        Self {
            source,
            dialect,
            delimiter: ";".to_owned(),
            statement_start: 0,
            leading_words: Vec::new(),
            block_depth: 0,
            statements: Vec::new(),
        }
    }

    fn feed(&mut self, tokens: &[Token<'_>], index: usize) -> usize {
        let token = tokens[index];
        if token.kind.is_trivia()
            || matches!(token.kind, TokenKind::String | TokenKind::QuotedIdentifier)
        {
            return index + 1;
        }
        if self.leading_words.is_empty() && self.is_delimiter_directive(&token) {
            return self.apply_delimiter_directive(tokens, index);
        }
        if self.block_depth == 0 && self.source[token.start..].starts_with(&self.delimiter) {
            self.flush(token.start);
            let delimiter_end = token.start + self.delimiter.len();
            self.statement_start = delimiter_end;
            return skip_until(tokens, index, delimiter_end);
        }
        if token.kind.is_word() {
            self.track_word(tokens, index);
        }
        index + 1
    }

    fn track_word(&mut self, tokens: &[Token<'_>], index: usize) {
        let word = tokens[index].text.to_ascii_uppercase();
        if self.leading_words.len() < 6 {
            self.leading_words.push(word.clone());
        }
        if !self.is_compound_definition() {
            return;
        }
        match word.as_str() {
            "BEGIN" | "CASE" => self.block_depth += 1,
            "END" => {
                let closes_control_flow = next_word(tokens, index).is_some_and(|next| {
                    ["IF", "LOOP", "WHILE", "REPEAT", "FOR"]
                        .iter()
                        .any(|keyword| next.eq_ignore_ascii_case(keyword))
                });
                if !closes_control_flow {
                    self.block_depth = self.block_depth.saturating_sub(1);
                }
            }
            _ => {}
        }
    }

    fn is_compound_definition(&self) -> bool {
        self.leading_words
            .first()
            .is_some_and(|word| word == "CREATE")
            && self.leading_words.iter().any(|word| {
                matches!(
                    word.as_str(),
                    "TRIGGER" | "FUNCTION" | "PROCEDURE" | "EVENT"
                )
            })
    }

    fn is_delimiter_directive(&self, token: &Token<'_>) -> bool {
        self.dialect == Dialect::MySql && token.is_keyword("DELIMITER")
    }

    fn apply_delimiter_directive(&mut self, tokens: &[Token<'_>], index: usize) -> usize {
        let token = tokens[index];
        let line_end = self.source[token.end..]
            .find('\n')
            .map_or(self.source.len(), |offset| token.end + offset);
        let delimiter = self.source[token.end..line_end].trim();
        if !delimiter.is_empty() {
            self.delimiter = delimiter.to_owned();
        }
        self.statement_start = line_end;
        skip_until(tokens, index, line_end)
    }

    fn flush(&mut self, end: usize) {
        let raw = &self.source[self.statement_start.min(end)..end];
        let leading = raw.len() - raw.trim_start().len();
        let text = raw.trim();
        if has_code(text, self.dialect) {
            let start = self.statement_start + leading;
            self.statements.push(StatementSpan {
                start,
                end: start + text.len(),
                text: text.to_owned(),
            });
        }
        self.leading_words.clear();
        self.block_depth = 0;
    }
}

fn skip_until(tokens: &[Token<'_>], mut index: usize, position: usize) -> usize {
    while index < tokens.len() && tokens[index].start < position {
        index += 1;
    }
    index
}

fn next_word<'a>(tokens: &[Token<'a>], index: usize) -> Option<&'a str> {
    tokens[index + 1..]
        .iter()
        .find(|token| !token.kind.is_trivia())
        .filter(|token| token.kind.is_word())
        .map(|token| token.text)
}

fn has_code(text: &str, dialect: Dialect) -> bool {
    tokenize(text, dialect)
        .iter()
        .any(|token| !token.kind.is_trivia())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn texts(source: &str, dialect: Dialect) -> Vec<String> {
        split_statements(source, dialect)
            .into_iter()
            .map(|statement| statement.text)
            .collect()
    }

    #[test]
    fn splits_on_semicolons_outside_literals() {
        assert_eq!(
            texts(
                "select ';' ; -- c;\nselect \"a;b\";\n\n  ;",
                Dialect::Postgres
            ),
            vec!["select ';'", "-- c;\nselect \"a;b\""]
        );
    }

    #[test]
    fn keeps_dollar_quoted_function_bodies() {
        let script = "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql;\nSELECT f();";
        assert_eq!(texts(script, Dialect::Postgres).len(), 2);
    }

    #[test]
    fn keeps_sqlite_trigger_bodies_together() {
        let script = "CREATE TRIGGER t AFTER INSERT ON a BEGIN\n  UPDATE b SET x = CASE WHEN 1 THEN 2 END;\n  INSERT INTO c VALUES (1);\nEND;\nSELECT 1;";
        let statements = texts(script, Dialect::Sqlite);
        assert_eq!(statements.len(), 2);
        assert!(statements[0].ends_with("END"));
    }

    #[test]
    fn transaction_begin_is_not_a_block() {
        assert_eq!(
            texts("BEGIN; UPDATE t SET a = 1; COMMIT;", Dialect::Postgres),
            vec!["BEGIN", "UPDATE t SET a = 1", "COMMIT"]
        );
    }

    #[test]
    fn honours_mysql_delimiter_directive() {
        let script = "DELIMITER //\nCREATE PROCEDURE p() BEGIN SELECT 1; IF 1 THEN SELECT 2; END IF; END//\nDELIMITER ;\nCALL p();";
        let statements = texts(script, Dialect::MySql);
        assert_eq!(statements.len(), 2);
        assert!(statements[0].starts_with("CREATE PROCEDURE"));
        assert!(statements[0].ends_with("END"));
        assert_eq!(statements[1], "CALL p()");
    }

    #[test]
    fn finds_statement_at_cursor() {
        let script = "select 1;\nselect 2;\n\n";
        assert_eq!(
            statement_at(script, Dialect::Generic, 3).unwrap().text,
            "select 1"
        );
        assert_eq!(
            statement_at(script, Dialect::Generic, 12).unwrap().text,
            "select 2"
        );
        assert_eq!(
            statement_at(script, Dialect::Generic, script.len())
                .unwrap()
                .text,
            "select 2"
        );
        assert_eq!(statement_at("", Dialect::Generic, 0), None);
    }

    #[test]
    fn spans_point_into_source() {
        let script = "  select 1 ;  select 2";
        for statement in split_statements(script, Dialect::Generic) {
            assert_eq!(&script[statement.start..statement.end], statement.text);
        }
    }
}
