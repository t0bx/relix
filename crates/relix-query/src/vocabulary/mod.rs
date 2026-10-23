/*
 * @author t0bx
 * @since 23.10.2026
 */

mod analytics;
mod classic;
mod common;
mod enterprise;
mod nosql;

use analytics::*;
use classic::*;
use common::*;
use enterprise::*;
use nosql::*;
use relix_core::Dialect;

type Words = &'static [&'static str];

struct DialectWords {
    keywords: Words,
    functions: Words,
    types: Words,
}

const fn words(keywords: Words, functions: Words, types: Words) -> DialectWords {
    DialectWords {
        keywords,
        functions,
        types,
    }
}

fn dialect_words(dialect: Dialect) -> DialectWords {
    match dialect {
        Dialect::Postgres => words(POSTGRES_KEYWORDS, POSTGRES_FUNCTIONS, POSTGRES_TYPES),
        Dialect::MySql => words(MYSQL_KEYWORDS, MYSQL_FUNCTIONS, MYSQL_TYPES),
        Dialect::Sqlite => words(SQLITE_KEYWORDS, SQLITE_FUNCTIONS, SQLITE_TYPES),
        Dialect::MsSql => words(MSSQL_KEYWORDS, MSSQL_FUNCTIONS, MSSQL_TYPES),
        Dialect::Oracle => words(ORACLE_KEYWORDS, ORACLE_FUNCTIONS, ORACLE_TYPES),
        Dialect::ClickHouse => words(CLICKHOUSE_KEYWORDS, CLICKHOUSE_FUNCTIONS, CLICKHOUSE_TYPES),
        Dialect::DuckDb => words(DUCKDB_KEYWORDS, DUCKDB_FUNCTIONS, DUCKDB_TYPES),
        Dialect::Trino => words(TRINO_KEYWORDS, TRINO_FUNCTIONS, TRINO_TYPES),
        Dialect::Snowflake => words(SNOWFLAKE_KEYWORDS, SNOWFLAKE_FUNCTIONS, SNOWFLAKE_TYPES),
        Dialect::BigQuery => words(BIGQUERY_KEYWORDS, BIGQUERY_FUNCTIONS, BIGQUERY_TYPES),
        Dialect::InfluxSql => words(INFLUX_KEYWORDS, INFLUX_FUNCTIONS, INFLUX_TYPES),
        Dialect::Cql => words(CQL_KEYWORDS, CQL_FUNCTIONS, CQL_TYPES),
        Dialect::Cypher => words(CYPHER_KEYWORDS, CYPHER_FUNCTIONS, &[]),
        Dialect::PartiQl => words(PARTIQL_KEYWORDS, PARTIQL_FUNCTIONS, &[]),
        Dialect::Generic | Dialect::MongoCommand | Dialect::SearchRequest => words(&[], &[], &[]),
    }
}

fn shares_common_sql(dialect: Dialect) -> bool {
    !matches!(
        dialect,
        Dialect::Cypher | Dialect::PartiQl | Dialect::MongoCommand | Dialect::SearchRequest
    )
}

fn with_common(
    dialect: Dialect,
    common: Words,
    specific: Words,
) -> impl Iterator<Item = &'static str> {
    let common: Words = if shares_common_sql(dialect) {
        common
    } else {
        &[]
    };
    common.iter().chain(specific).copied()
}

pub fn keywords(dialect: Dialect) -> impl Iterator<Item = &'static str> {
    with_common(dialect, COMMON_KEYWORDS, dialect_words(dialect).keywords)
}

pub fn functions(dialect: Dialect) -> impl Iterator<Item = &'static str> {
    with_common(dialect, COMMON_FUNCTIONS, dialect_words(dialect).functions)
}

pub fn data_types(dialect: Dialect) -> impl Iterator<Item = &'static str> {
    with_common(dialect, COMMON_TYPES, dialect_words(dialect).types)
}

pub fn is_keyword(dialect: Dialect, word: &str) -> bool {
    keywords(dialect).any(|keyword| keyword.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialects_extend_common_vocabulary() {
        assert!(is_keyword(Dialect::Postgres, "ilike"));
        assert!(!is_keyword(Dialect::MySql, "ILIKE"));
        assert!(is_keyword(Dialect::Sqlite, "pragma"));
        assert!(functions(Dialect::MySql).any(|name| name == "GROUP_CONCAT"));
        assert!(data_types(Dialect::Postgres).any(|name| name == "JSONB"));
        assert!(!data_types(Dialect::Generic).any(|name| name == "JSONB"));
    }

    #[test]
    fn new_dialects_have_native_vocabulary() {
        assert!(is_keyword(Dialect::MsSql, "top"));
        assert!(is_keyword(Dialect::ClickHouse, "prewhere"));
        assert!(is_keyword(Dialect::Cql, "allow"));
        assert!(data_types(Dialect::BigQuery).any(|name| name == "INT64"));
        assert!(functions(Dialect::Oracle).any(|name| name == "NVL"));
    }

    #[test]
    fn graph_and_document_languages_skip_sql_keywords() {
        assert!(is_keyword(Dialect::Cypher, "match"));
        assert!(!is_keyword(Dialect::Cypher, "join"));
        assert!(!is_keyword(Dialect::MongoCommand, "select"));
    }
}
