/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Dialect;
use relix_query::lexer::{Token, TokenKind, significant, tokenize};
use serde::Serialize;

/// Database, schema, warehouse and role sent with every SQL API request. The API runs
/// each request in its own session, so `USE …` statements are remembered here and
/// applied to the statements that follow.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub(crate) struct StatementContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub database: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warehouse: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

impl StatementContext {
    /// Returns the context after a successful `USE ROLE | WAREHOUSE | DATABASE | SCHEMA`
    /// statement, or `None` when the statement does not change it.
    pub(crate) fn after_use(&self, statement: &str) -> Option<Self> {
        let tokens = tokenize(statement, Dialect::Snowflake);
        let words: Vec<Token<'_>> = significant(&tokens)
            .filter(|token| !token.is_symbol(";"))
            .collect();
        let (first, rest) = words.split_first()?;
        if !first.is_keyword("USE") {
            return None;
        }
        let mut updated = self.clone();
        match rest {
            [kind, name] if kind.is_keyword("ROLE") => updated.role = Some(identifier(name)?),
            [kind, name] if kind.is_keyword("WAREHOUSE") => {
                updated.warehouse = Some(identifier(name)?)
            }
            [kind, name] if kind.is_keyword("DATABASE") => updated.use_database(name)?,
            [name] => updated.use_database(name)?,
            [kind, name] if kind.is_keyword("SCHEMA") => updated.schema = Some(identifier(name)?),
            [kind, database, dot, name] if kind.is_keyword("SCHEMA") && dot.is_symbol(".") => {
                updated.database = Some(identifier(database)?);
                updated.schema = Some(identifier(name)?);
            }
            _ => return None,
        }
        Some(updated)
    }

    fn use_database(&mut self, name: &Token<'_>) -> Option<()> {
        self.database = Some(identifier(name)?);
        self.schema = None;
        Some(())
    }
}

/// Resolves a configured object name the way Snowflake resolves identifiers: unquoted
/// names are case-insensitive and stored upper case, double-quoted names are exact.
pub(crate) fn object_name(raw: &str) -> String {
    let trimmed = raw.trim();
    match trimmed
        .strip_prefix('"')
        .and_then(|inner| inner.strip_suffix('"'))
    {
        Some(inner) => inner.replace("\"\"", "\""),
        None => trimmed.to_uppercase(),
    }
}

fn identifier(token: &Token<'_>) -> Option<String> {
    match token.kind {
        TokenKind::QuotedIdentifier | TokenKind::Identifier | TokenKind::Keyword => {
            Some(object_name(token.text))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn context() -> StatementContext {
        StatementContext {
            database: Some("SALES".into()),
            schema: Some("PUBLIC".into()),
            warehouse: Some("WH".into()),
            role: None,
        }
    }

    #[test]
    fn use_statements_update_the_context() {
        let next = context().after_use("use warehouse reporting_wh;").unwrap();
        assert_eq!(next.warehouse.as_deref(), Some("REPORTING_WH"));
        let next = context().after_use("USE ROLE \"Analyst\"").unwrap();
        assert_eq!(next.role.as_deref(), Some("Analyst"));
        let next = context().after_use("USE DATABASE finance").unwrap();
        assert_eq!(next.database.as_deref(), Some("FINANCE"));
        assert_eq!(next.schema, None);
        let next = context().after_use("USE finance").unwrap();
        assert_eq!(next.database.as_deref(), Some("FINANCE"));
        let next = context().after_use("USE SCHEMA hr.\"Staff\"").unwrap();
        assert_eq!(next.database.as_deref(), Some("HR"));
        assert_eq!(next.schema.as_deref(), Some("Staff"));
    }

    #[test]
    fn other_statements_keep_the_context() {
        assert_eq!(context().after_use("SELECT 1"), None);
        assert_eq!(context().after_use("USE SECONDARY ROLES ALL"), None);
    }

    #[test]
    fn configured_names_follow_identifier_rules() {
        assert_eq!(object_name(" compute_wh "), "COMPUTE_WH");
        assert_eq!(object_name("\"My \"\"Db\"\"\""), "My \"Db\"");
    }

    #[test]
    fn empty_fields_are_not_serialized() {
        let json = serde_json::to_value(StatementContext::default()).unwrap();
        assert_eq!(json, serde_json::json!({}));
    }
}
