/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Dialect;
use relix_query::lexer::{Token, TokenKind, significant, tokenize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TableReference {
    pub schema: Option<String>,
    pub name: String,
    pub alias: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionContext {
    Member { qualifier: String },
    TableName,
    DataType,
    Expression,
    StatementStart,
}

const TABLE_KEYWORDS: &[&str] = &["FROM", "JOIN", "INTO", "UPDATE", "TABLE", "DESCRIBE"];
const EXPRESSION_KEYWORDS: &[&str] = &[
    "SELECT",
    "WHERE",
    "AND",
    "OR",
    "ON",
    "BY",
    "HAVING",
    "SET",
    "NOT",
    "WHEN",
    "THEN",
    "ELSE",
    "DISTINCT",
    "RETURNING",
    "USING",
    "CASE",
    "IN",
    "LIKE",
    "ILIKE",
    "BETWEEN",
    "IS",
];

pub fn strip_quotes(text: &str) -> String {
    match text.chars().next() {
        Some(quote @ ('"' | '`')) if text.len() >= 2 && text.ends_with(quote) => {
            let inner = &text[1..text.len() - 1];
            inner.replace(&format!("{quote}{quote}"), &quote.to_string())
        }
        Some('[') if text.ends_with(']') => text[1..text.len() - 1].to_owned(),
        _ => text.to_owned(),
    }
}

/// Determines what kind of completion fits at the end of `before_cursor`, which must not
/// include the identifier prefix currently being typed.
pub fn completion_context(before_cursor: &str, dialect: Dialect) -> CompletionContext {
    let tokens = tokenize(before_cursor, dialect);
    let significant: Vec<Token<'_>> = significant(&tokens).collect();
    let Some(last) = significant.last() else {
        return CompletionContext::StatementStart;
    };
    if last.is_symbol(".") {
        let qualifier = significant
            .iter()
            .rev()
            .nth(1)
            .filter(|token| token.kind.is_word() || token.kind == TokenKind::QuotedIdentifier)
            .map(|token| strip_quotes(token.text));
        if let Some(qualifier) = qualifier {
            return CompletionContext::Member { qualifier };
        }
    }
    if last.is_symbol("::") || last.is_keyword("AS") && inside_cast(&significant) {
        return CompletionContext::DataType;
    }
    if TABLE_KEYWORDS
        .iter()
        .any(|keyword| last.is_keyword(keyword))
    {
        return CompletionContext::TableName;
    }
    if last.is_symbol(",") && in_from_clause(&significant) {
        return CompletionContext::TableName;
    }
    let expression = matches!(last.kind, TokenKind::Operator | TokenKind::Punctuation)
        || EXPRESSION_KEYWORDS
            .iter()
            .any(|keyword| last.is_keyword(keyword));
    if expression || (significant.len() == 1 && last.kind == TokenKind::Keyword) {
        CompletionContext::Expression
    } else {
        CompletionContext::StatementStart.refine(&significant)
    }
}

impl CompletionContext {
    fn refine(self, significant: &[Token<'_>]) -> Self {
        let in_statement = significant
            .iter()
            .any(|token| token.kind == TokenKind::Keyword);
        if in_statement { Self::Expression } else { self }
    }
}

fn inside_cast(tokens: &[Token<'_>]) -> bool {
    let mut depth = 0i32;
    for (index, token) in tokens.iter().enumerate().rev() {
        if token.is_symbol(")") {
            depth += 1;
        } else if token.is_symbol("(") {
            if depth == 0 {
                return index > 0 && tokens[index - 1].is_keyword("CAST");
            }
            depth -= 1;
        }
    }
    false
}

fn in_from_clause(tokens: &[Token<'_>]) -> bool {
    for token in tokens.iter().rev() {
        if token.is_keyword("FROM") || token.is_keyword("UPDATE") {
            return true;
        }
        if token.kind == TokenKind::Keyword && !token.is_keyword("AS") {
            return false;
        }
    }
    false
}

/// Collects tables referenced through `FROM`, `JOIN`, `UPDATE` and `INTO`, including their
/// aliases, anywhere in the statement (the FROM clause usually follows the cursor).
pub fn table_references(statement: &str, dialect: Dialect) -> Vec<TableReference> {
    let tokens = tokenize(statement, dialect);
    let words: Vec<Token<'_>> = significant(&tokens).collect();
    let mut references = Vec::new();
    let mut index = 0;
    while index < words.len() {
        let token = words[index];
        let introduces = ["FROM", "JOIN", "UPDATE", "INTO"]
            .iter()
            .any(|keyword| token.is_keyword(keyword));
        index += 1;
        if !introduces {
            continue;
        }
        while let Some((reference, next)) = parse_table_reference(&words, index) {
            references.push(reference);
            index = next;
            let continues_list = token.is_keyword("FROM")
                && words.get(index).is_some_and(|token| token.is_symbol(","));
            if !continues_list {
                break;
            }
            index += 1;
        }
    }
    references
}

fn is_name(token: &Token<'_>) -> bool {
    matches!(
        token.kind,
        TokenKind::Identifier | TokenKind::QuotedIdentifier
    )
}

fn parse_table_reference(words: &[Token<'_>], start: usize) -> Option<(TableReference, usize)> {
    let mut parts = Vec::new();
    let mut index = start;
    loop {
        let token = words.get(index)?;
        if !is_name(token) && !(token.kind == TokenKind::Keyword && !parts.is_empty()) {
            break;
        }
        parts.push(strip_quotes(token.text));
        index += 1;
        if words.get(index).is_some_and(|token| token.is_symbol(".")) {
            index += 1;
        } else {
            break;
        }
    }
    let name = parts.pop()?;
    let schema = parts.pop();
    if words.get(index).is_some_and(|token| token.is_keyword("AS")) {
        index += 1;
    }
    let alias = words
        .get(index)
        .filter(|token| is_name(token))
        .map(|token| strip_quotes(token.text));
    if alias.is_some() {
        index += 1;
    }
    Some((
        TableReference {
            schema,
            name,
            alias,
        },
        index,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn context(sql: &str) -> CompletionContext {
        completion_context(sql, Dialect::Postgres)
    }

    #[test]
    fn detects_contexts() {
        assert_eq!(context(""), CompletionContext::StatementStart);
        assert_eq!(context("SELECT * FROM "), CompletionContext::TableName);
        assert_eq!(context("SELECT * FROM a, "), CompletionContext::TableName);
        assert_eq!(
            context("SELECT u."),
            CompletionContext::Member {
                qualifier: "u".into()
            }
        );
        assert_eq!(
            context("SELECT \"Users\"."),
            CompletionContext::Member {
                qualifier: "Users".into()
            }
        );
        assert_eq!(context("SELECT "), CompletionContext::Expression);
        assert_eq!(context("SELECT a, "), CompletionContext::Expression);
        assert_eq!(
            context("SELECT * FROM t WHERE a = 1 AND "),
            CompletionContext::Expression
        );
        assert_eq!(context("SELECT x::"), CompletionContext::DataType);
        assert_eq!(context("SELECT CAST(x AS "), CompletionContext::DataType);
        assert_eq!(context("SELECT a AS "), CompletionContext::Expression);
    }

    #[test]
    fn collects_table_references_with_aliases() {
        let refs = table_references(
            "SELECT * FROM public.users u JOIN orders AS o ON o.user_id = u.id, items WHERE 1",
            Dialect::Postgres,
        );
        assert_eq!(
            refs,
            vec![
                TableReference {
                    schema: Some("public".into()),
                    name: "users".into(),
                    alias: Some("u".into()),
                },
                TableReference {
                    schema: None,
                    name: "orders".into(),
                    alias: Some("o".into()),
                },
            ]
        );
        let update = table_references("UPDATE `shop`.`orders` SET a = 1", Dialect::MySql);
        assert_eq!(update[0].schema.as_deref(), Some("shop"));
        assert_eq!(update[0].alias, None);
    }
}
