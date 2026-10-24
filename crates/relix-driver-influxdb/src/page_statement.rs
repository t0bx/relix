/*
 * @author t0bx
 * @since 24.10.2026
 */

use relix_core::{Dialect, TableQuery};

pub(crate) fn qualified_name(schema: &str, table: &str) -> String {
    format!(
        "{}.{}",
        Dialect::InfluxSql.quote_identifier(schema),
        Dialect::InfluxSql.quote_identifier(table)
    )
}

/// Builds the page query. The filter is used verbatim as the `WHERE` condition, so time
/// ranges such as `time > now() - INTERVAL '1 hour'` keep the scan small. Without an
/// explicit sort, tables with a `time` column are shown newest first.
pub(crate) fn page_statement(
    schema: &str,
    table: &str,
    query: &TableQuery,
    has_time: bool,
) -> String {
    let mut sql = format!(
        "SELECT * FROM {}{}",
        qualified_name(schema, table),
        where_clause(query.filter.as_deref())
    );
    let order: Vec<String> = if query.sort.is_empty() && has_time {
        vec!["\"time\" DESC".to_owned()]
    } else {
        query
            .sort
            .iter()
            .map(|key| {
                format!(
                    "{} {}",
                    Dialect::InfluxSql.quote_identifier(&key.column),
                    key.direction.sql_keyword()
                )
            })
            .collect()
    };
    if !order.is_empty() {
        sql.push_str(&format!(" ORDER BY {}", order.join(", ")));
    }
    sql.push_str(&format!(
        " LIMIT {} OFFSET {}",
        u64::from(query.limit) + 1,
        query.offset
    ));
    sql
}

pub(crate) fn count_statement(schema: &str, table: &str, filter: Option<&str>) -> String {
    format!(
        "SELECT COUNT(*) AS count FROM {}{}",
        qualified_name(schema, table),
        where_clause(filter)
    )
}

fn where_clause(filter: Option<&str>) -> String {
    let condition = filter
        .unwrap_or_default()
        .trim()
        .trim_end_matches(';')
        .trim();
    if condition.is_empty() {
        String::new()
    } else {
        format!(" WHERE ({condition})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use relix_core::{SortDirection, SortKey};

    #[test]
    fn measurements_default_to_newest_first() {
        let query = TableQuery {
            filter: Some("time > now() - INTERVAL '1 hour';".into()),
            offset: 40,
            limit: 20,
            ..TableQuery::default()
        };
        assert_eq!(
            page_statement("iox", "cpu", &query, true),
            "SELECT * FROM \"iox\".\"cpu\" WHERE (time > now() - INTERVAL '1 hour') \
             ORDER BY \"time\" DESC LIMIT 21 OFFSET 40"
        );
    }

    #[test]
    fn explicit_sorts_replace_the_default() {
        let query = TableQuery {
            sort: vec![SortKey {
                column: "host".into(),
                direction: SortDirection::Ascending,
            }],
            ..TableQuery::default()
        };
        assert_eq!(
            page_statement("system", "queries", &query, false),
            "SELECT * FROM \"system\".\"queries\" ORDER BY \"host\" ASC LIMIT 201 OFFSET 0"
        );
        assert_eq!(
            count_statement("iox", "cpu", None),
            "SELECT COUNT(*) AS count FROM \"iox\".\"cpu\""
        );
    }
}
