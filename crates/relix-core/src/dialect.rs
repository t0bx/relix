/*
 * @author t0bx
 * @since 23.10.2026
 */

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DataModel {
    Relational,
    KeyValue,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    Postgres,
    MySql,
    Sqlite,
    Generic,
}

impl Dialect {
    pub fn name(self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::MySql => "MySQL",
            Self::Sqlite => "SQLite",
            Self::Generic => "SQL",
        }
    }

    pub fn quote_identifier(self, identifier: &str) -> String {
        let quote = match self {
            Self::MySql => '`',
            _ => '"',
        };
        let mut quoted = String::with_capacity(identifier.len() + 2);
        quoted.push(quote);
        for ch in identifier.chars() {
            if ch == quote {
                quoted.push(quote);
            }
            quoted.push(ch);
        }
        quoted.push(quote);
        quoted
    }

    /// Quotes a string literal. MySQL additionally treats backslashes as escape
    /// characters unless `NO_BACKSLASH_ESCAPES` is active, so they are doubled as well.
    pub fn quote_literal(self, literal: &str) -> String {
        let mut quoted = String::with_capacity(literal.len() + 2);
        quoted.push('\'');
        for ch in literal.chars() {
            match ch {
                '\'' => quoted.push_str("''"),
                '\\' if self == Self::MySql => quoted.push_str("\\\\"),
                _ => quoted.push(ch),
            }
        }
        quoted.push('\'');
        quoted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_identifiers_per_dialect() {
        assert_eq!(Dialect::Postgres.quote_identifier("a\"b"), "\"a\"\"b\"");
        assert_eq!(Dialect::MySql.quote_identifier("a`b"), "`a``b`");
        assert_eq!(Dialect::Sqlite.quote_identifier("users"), "\"users\"");
    }

    #[test]
    fn quotes_literals_per_dialect() {
        assert_eq!(Dialect::Postgres.quote_literal("it's \\n"), "'it''s \\n'");
        assert_eq!(Dialect::MySql.quote_literal("it's \\n"), "'it''s \\\\n'");
    }
}
