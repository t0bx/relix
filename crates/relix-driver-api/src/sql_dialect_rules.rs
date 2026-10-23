/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::value::hex_string;
use relix_core::{Dialect, TableRef, Value};

pub(crate) fn qualifiers(dialect: Dialect, table: &TableRef) -> Vec<&str> {
    let database = table.database.as_deref();
    let schema = table.schema.as_deref();
    let parts = match dialect {
        Dialect::Postgres | Dialect::Oracle => vec![schema],
        Dialect::MySql | Dialect::ClickHouse | Dialect::Cql => vec![database.or(schema)],
        Dialect::Sqlite | Dialect::Generic => vec![schema.or(database)],
        Dialect::MsSql
        | Dialect::DuckDb
        | Dialect::Trino
        | Dialect::Snowflake
        | Dialect::BigQuery => vec![database, schema],
        Dialect::InfluxSql
        | Dialect::PartiQl
        | Dialect::Cypher
        | Dialect::MongoCommand
        | Dialect::SearchRequest => Vec::new(),
    };
    parts.into_iter().flatten().collect()
}

pub(crate) fn page_clause(dialect: Dialect, has_order: bool, offset: u64, fetch: u64) -> String {
    match dialect {
        Dialect::MsSql => {
            let order = if has_order {
                ""
            } else {
                " ORDER BY (SELECT NULL)"
            };
            format!("{order} OFFSET {offset} ROWS FETCH NEXT {fetch} ROWS ONLY")
        }
        Dialect::Oracle => format!(" OFFSET {offset} ROWS FETCH NEXT {fetch} ROWS ONLY"),
        Dialect::Trino => format!(" OFFSET {offset} LIMIT {fetch}"),
        Dialect::Cql => format!(" LIMIT {fetch}"),
        _ => format!(" LIMIT {fetch} OFFSET {offset}"),
    }
}

pub(crate) fn empty_insert_suffix(dialect: Dialect) -> &'static str {
    match dialect {
        Dialect::MySql => " () VALUES ()",
        _ => " DEFAULT VALUES",
    }
}

pub(crate) fn null_safe_operator(dialect: Dialect) -> Option<&'static str> {
    match dialect {
        Dialect::MySql => Some(" <=> "),
        Dialect::Sqlite => Some(" IS "),
        Dialect::Postgres
        | Dialect::DuckDb
        | Dialect::Trino
        | Dialect::Snowflake
        | Dialect::BigQuery => Some(" IS NOT DISTINCT FROM "),
        _ => None,
    }
}

pub(crate) fn binary_literal(dialect: Dialect, bytes: &[u8]) -> String {
    let hex = &hex_string(bytes)[2..];
    match dialect {
        Dialect::Postgres => format!("'\\x{hex}'"),
        Dialect::MsSql => format!("0x{hex}"),
        Dialect::Oracle => format!("HEXTORAW('{hex}')"),
        Dialect::ClickHouse => format!("unhex('{hex}')"),
        Dialect::BigQuery => format!("FROM_HEX('{hex}')"),
        Dialect::Snowflake => format!("TO_BINARY('{hex}', 'HEX')"),
        Dialect::DuckDb => format!("from_hex('{hex}')"),
        Dialect::Cql => format!("0x{hex}"),
        _ => format!("X'{hex}'"),
    }
}

/// Strictly typed engines reject comparisons between numbers and string literals, so
/// numbers and booleans are written as typed literals there. The original engines keep
/// quoting every value, which their implicit casts handle and which preserves exact
/// textual comparisons.
pub(crate) fn typed_literal(dialect: Dialect, value: &Value) -> Option<String> {
    let strict = !matches!(
        dialect,
        Dialect::Postgres | Dialect::MySql | Dialect::Sqlite | Dialect::Generic
    );
    if !strict {
        return None;
    }
    match value {
        Value::Int(number) => Some(number.to_string()),
        Value::UInt(number) => Some(number.to_string()),
        Value::Float(number) if number.is_finite() => Some(float_literal(*number)),
        Value::Bool(flag) if dialect == Dialect::MsSql => Some(u8::from(*flag).to_string()),
        Value::Bool(flag) => Some(if *flag { "TRUE" } else { "FALSE" }.to_owned()),
        _ => None,
    }
}

fn float_literal(number: f64) -> String {
    let text = number.to_string();
    if text.contains(['.', 'e', 'E']) {
        text
    } else {
        format!("{text}.0")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_part_names_keep_present_parts() {
        let table = TableRef::new(Some("analytics".into()), Some("sales".into()), "orders");
        assert_eq!(
            qualifiers(Dialect::Trino, &table),
            vec!["analytics", "sales"]
        );
        let unqualified = TableRef::new(None, Some("dbo".into()), "orders");
        assert_eq!(qualifiers(Dialect::MsSql, &unqualified), vec!["dbo"]);
        assert!(qualifiers(Dialect::InfluxSql, &table).is_empty());
    }

    #[test]
    fn page_clauses_follow_dialect_syntax() {
        assert_eq!(
            page_clause(Dialect::MsSql, false, 10, 21),
            " ORDER BY (SELECT NULL) OFFSET 10 ROWS FETCH NEXT 21 ROWS ONLY"
        );
        assert_eq!(page_clause(Dialect::Trino, true, 0, 5), " OFFSET 0 LIMIT 5");
        assert_eq!(page_clause(Dialect::Cql, true, 40, 5), " LIMIT 5");
        assert_eq!(
            page_clause(Dialect::ClickHouse, true, 3, 5),
            " LIMIT 5 OFFSET 3"
        );
    }

    #[test]
    fn strict_dialects_use_typed_literals() {
        assert_eq!(
            typed_literal(Dialect::Trino, &Value::Int(4)),
            Some("4".into())
        );
        assert_eq!(
            typed_literal(Dialect::BigQuery, &Value::Float(2.0)),
            Some("2.0".into())
        );
        assert_eq!(
            typed_literal(Dialect::MsSql, &Value::Bool(true)),
            Some("1".into())
        );
        assert_eq!(typed_literal(Dialect::Postgres, &Value::Int(4)), None);
        assert_eq!(
            typed_literal(Dialect::Trino, &Value::Text("4".into())),
            None
        );
    }

    #[test]
    fn binary_literals_use_native_hex_functions() {
        assert_eq!(binary_literal(Dialect::MsSql, &[1, 255]), "0x01ff");
        assert_eq!(binary_literal(Dialect::Oracle, &[1]), "HEXTORAW('01')");
        assert_eq!(binary_literal(Dialect::Sqlite, &[1]), "X'01'");
    }
}
