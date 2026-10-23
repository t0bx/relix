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
    Document,
    WideColumn,
    Graph,
    TimeSeries,
    Search,
}

impl DataModel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Relational => "relational",
            Self::KeyValue => "key-value",
            Self::Document => "document",
            Self::WideColumn => "wide-column",
            Self::Graph => "graph",
            Self::TimeSeries => "time-series",
            Self::Search => "search",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    Postgres,
    MySql,
    Sqlite,
    Generic,
    MsSql,
    Oracle,
    ClickHouse,
    DuckDb,
    Trino,
    Snowflake,
    BigQuery,
    InfluxSql,
    Cql,
    Cypher,
    PartiQl,
    MongoCommand,
    SearchRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringEscape {
    Doubling,
    DoublingAndBackslash,
    Backslash,
}

impl Dialect {
    pub fn name(self) -> &'static str {
        match self {
            Self::Postgres => "PostgreSQL",
            Self::MySql => "MySQL",
            Self::Sqlite => "SQLite",
            Self::Generic => "SQL",
            Self::MsSql => "T-SQL",
            Self::Oracle => "Oracle SQL",
            Self::ClickHouse => "ClickHouse SQL",
            Self::DuckDb => "DuckDB SQL",
            Self::Trino => "Trino SQL",
            Self::Snowflake => "Snowflake SQL",
            Self::BigQuery => "GoogleSQL",
            Self::InfluxSql => "InfluxDB SQL",
            Self::Cql => "CQL",
            Self::Cypher => "Cypher",
            Self::PartiQl => "PartiQL",
            Self::MongoCommand => "MongoDB commands",
            Self::SearchRequest => "Query DSL requests",
        }
    }

    pub fn is_sql_like(self) -> bool {
        !matches!(self, Self::MongoCommand | Self::SearchRequest)
    }

    pub fn identifier_quotes(self) -> (char, char) {
        match self {
            Self::MySql | Self::ClickHouse | Self::BigQuery | Self::Cypher => ('`', '`'),
            Self::MsSql => ('[', ']'),
            _ => ('"', '"'),
        }
    }

    pub fn string_escape(self) -> StringEscape {
        match self {
            Self::MySql | Self::ClickHouse => StringEscape::DoublingAndBackslash,
            Self::BigQuery | Self::Cypher => StringEscape::Backslash,
            _ => StringEscape::Doubling,
        }
    }

    pub fn has_hash_comments(self) -> bool {
        matches!(self, Self::MySql | Self::ClickHouse | Self::BigQuery)
    }

    pub fn has_slash_comments(self) -> bool {
        matches!(self, Self::Cql | Self::Cypher)
    }

    pub fn has_dollar_quotes(self) -> bool {
        matches!(self, Self::Postgres | Self::DuckDb | Self::Snowflake)
    }

    pub fn has_bracket_identifiers(self) -> bool {
        matches!(self, Self::Sqlite | Self::MsSql)
    }

    pub fn has_nested_comments(self) -> bool {
        matches!(self, Self::Postgres | Self::DuckDb)
    }

    pub fn quote_identifier(self, identifier: &str) -> String {
        let (open, close) = self.identifier_quotes();
        let mut quoted = String::with_capacity(identifier.len() + 2);
        quoted.push(open);
        for ch in identifier.chars() {
            if ch == close {
                quoted.push(close);
            }
            quoted.push(ch);
        }
        quoted.push(close);
        quoted
    }

    /// Quotes a string literal. Dialects that treat backslashes as escape characters get
    /// them escaped as well; dialects without `''` doubling escape the quote with `\'`.
    pub fn quote_literal(self, literal: &str) -> String {
        let escape = self.string_escape();
        let mut quoted = String::with_capacity(literal.len() + 2);
        quoted.push('\'');
        for ch in literal.chars() {
            match (ch, escape) {
                ('\'', StringEscape::Backslash) => quoted.push_str("\\'"),
                ('\'', _) => quoted.push_str("''"),
                ('\\', StringEscape::Doubling) => quoted.push('\\'),
                ('\\', _) => quoted.push_str("\\\\"),
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
        assert_eq!(Dialect::MsSql.quote_identifier("a]b"), "[a]]b]");
        assert_eq!(Dialect::BigQuery.quote_identifier("events"), "`events`");
    }

    #[test]
    fn quotes_literals_per_dialect() {
        assert_eq!(Dialect::Postgres.quote_literal("it's \\n"), "'it''s \\n'");
        assert_eq!(Dialect::MySql.quote_literal("it's \\n"), "'it''s \\\\n'");
        assert_eq!(
            Dialect::BigQuery.quote_literal("it's \\n"),
            "'it\\'s \\\\n'"
        );
        assert_eq!(Dialect::Cypher.quote_literal("o'neil"), "'o\\'neil'");
    }

    #[test]
    fn request_languages_are_not_sql_like() {
        assert!(Dialect::Cql.is_sql_like());
        assert!(!Dialect::MongoCommand.is_sql_like());
        assert!(!Dialect::SearchRequest.is_sql_like());
    }
}
