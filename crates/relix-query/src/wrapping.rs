/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::classify::{StatementKind, classify};
use crate::lexer::{significant, tokenize};
use relix_core::{Dialect, SortKey};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultView<'a> {
    pub filter: Option<&'a str>,
    pub sort: &'a [SortKey],
    pub offset: u64,
    pub limit: u32,
}

/// Rewrites a read-only query as a sub-select so results can be filtered, sorted and
/// paged on the server. Returns `None` for statements whose re-execution could have side
/// effects or that cannot be used as a derived table (`SELECT … INTO`, locking reads,
/// data-modifying CTEs, `SHOW`, `PRAGMA`, …).
pub fn wrap_query(statement: &str, dialect: Dialect, view: &ResultView<'_>) -> Option<String> {
    if !is_wrappable(statement, dialect) {
        return None;
    }
    let inner = statement.trim().trim_end_matches(';').trim_end();
    let mut sql = format!("SELECT * FROM ({inner}\n) AS relix_result");
    if let Some(filter) = view
        .filter
        .map(str::trim)
        .filter(|filter| !filter.is_empty())
    {
        sql.push_str(&format!(" WHERE ({filter})"));
    }
    if !view.sort.is_empty() {
        let terms: Vec<String> = view
            .sort
            .iter()
            .map(|key| {
                format!(
                    "{} {}",
                    dialect.quote_identifier(&key.column),
                    key.direction.sql_keyword()
                )
            })
            .collect();
        sql.push_str(&format!(" ORDER BY {}", terms.join(", ")));
    }
    sql.push_str(&format!(
        " LIMIT {} OFFSET {}",
        u64::from(view.limit) + 1,
        view.offset
    ));
    Some(sql)
}

pub fn is_wrappable(statement: &str, dialect: Dialect) -> bool {
    let derived_tables_with_limit = matches!(
        dialect,
        Dialect::Postgres
            | Dialect::MySql
            | Dialect::Sqlite
            | Dialect::Generic
            | Dialect::ClickHouse
            | Dialect::DuckDb
            | Dialect::Snowflake
            | Dialect::BigQuery
            | Dialect::InfluxSql
    );
    if !derived_tables_with_limit || classify(statement, dialect).kind != StatementKind::Query {
        return false;
    }
    let tokens = tokenize(statement, dialect);
    let mut words = significant(&tokens).filter(|token| token.kind.is_word());
    let starts_with_select = words.next().is_some_and(|token| {
        token.is_keyword("SELECT") || token.is_keyword("WITH") || token.is_keyword("VALUES")
    });
    let forbidden = ["INTO", "FOR", "LOCK"];
    starts_with_select
        && !significant(&tokens)
            .any(|token| forbidden.iter().any(|keyword| token.is_keyword(keyword)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::SortDirection;

    #[test]
    fn wraps_plain_selects() {
        let sort = [SortKey {
            column: "name".into(),
            direction: SortDirection::Ascending,
        }];
        let view = ResultView {
            filter: Some("age > 3"),
            sort: &sort,
            offset: 10,
            limit: 5,
        };
        assert_eq!(
            wrap_query("select * from users;", Dialect::MySql, &view).unwrap(),
            "SELECT * FROM (select * from users\n) AS relix_result WHERE (age > 3) ORDER BY `name` ASC LIMIT 6 OFFSET 10"
        );
    }

    #[test]
    fn refuses_statements_with_side_effects() {
        let view = ResultView::default();
        for sql in [
            "select * into new_table from t",
            "select * from t for update",
            "with d as (delete from t returning *) select * from d",
            "show tables",
            "update t set a = 1",
        ] {
            assert_eq!(wrap_query(sql, Dialect::Postgres, &view), None, "{sql}");
        }
    }
}
