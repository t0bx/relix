/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::table_names::{qualified_name, quote};
use relix_core::TableQuery;

const ALLOW_FILTERING: &str = " ALLOW FILTERING";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CqlFilter {
    condition: Option<String>,
    allow_filtering: bool,
}

impl CqlFilter {
    /// Splits a grid filter into the `WHERE` condition and a trailing `ALLOW FILTERING`.
    /// CQL only accepts that clause at the very end of a `SELECT`, after `LIMIT`, so it
    /// is moved there. The driver never adds it on its own: filtering on columns outside
    /// the primary key scans the table and the user has to opt in explicitly.
    pub(crate) fn parse(filter: Option<&str>) -> Self {
        let trimmed = filter
            .unwrap_or_default()
            .trim()
            .trim_end_matches(';')
            .trim();
        let stripped = strip_suffix_word(trimmed, "FILTERING")
            .and_then(|rest| strip_suffix_word(rest.trim_end(), "ALLOW"));
        let (condition, allow_filtering) = match stripped {
            Some(rest) => (rest.trim(), true),
            None => (trimmed, false),
        };
        Self {
            condition: (!condition.is_empty()).then(|| condition.to_owned()),
            allow_filtering,
        }
    }

    fn where_clause(&self) -> String {
        self.condition
            .as_ref()
            .map(|condition| format!(" WHERE {condition}"))
            .unwrap_or_default()
    }

    fn allow_clause(&self) -> &'static str {
        if self.allow_filtering {
            ALLOW_FILTERING
        } else {
            ""
        }
    }
}

pub(crate) fn page_statement(keyspace: &str, table: &str, query: &TableQuery) -> String {
    let filter = CqlFilter::parse(query.filter.as_deref());
    let mut cql = format!(
        "SELECT * FROM {}{}",
        qualified_name(keyspace, table),
        filter.where_clause()
    );
    if !query.sort.is_empty() {
        let order: Vec<String> = query
            .sort
            .iter()
            .map(|key| format!("{} {}", quote(&key.column), key.direction.sql_keyword()))
            .collect();
        cql.push_str(&format!(" ORDER BY {}", order.join(", ")));
    }
    let limit = query
        .offset
        .saturating_add(u64::from(query.limit))
        .saturating_add(1)
        .min(i32::MAX as u64);
    cql.push_str(&format!(" LIMIT {limit}{}", filter.allow_clause()));
    cql
}

pub(crate) fn count_statement(keyspace: &str, table: &str, filter: Option<&str>) -> String {
    let filter = CqlFilter::parse(filter);
    format!(
        "SELECT COUNT(*) FROM {}{}{}",
        qualified_name(keyspace, table),
        filter.where_clause(),
        filter.allow_clause()
    )
}

fn strip_suffix_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let split = text.len().checked_sub(word.len())?;
    let (rest, suffix) = (text.get(..split)?, text.get(split..)?);
    let separated = rest.is_empty() || rest.ends_with(char::is_whitespace);
    (suffix.eq_ignore_ascii_case(word) && separated).then_some(rest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use relix_core::{SortDirection, SortKey};

    #[test]
    fn allow_filtering_moves_behind_the_limit() {
        let query = TableQuery {
            filter: Some("city = 'Berlin' allow   filtering;".into()),
            sort: vec![],
            tiebreaker: vec!["id".into()],
            offset: 40,
            limit: 20,
        };
        assert_eq!(
            page_statement("shop", "users", &query),
            "SELECT * FROM \"shop\".\"users\" WHERE city = 'Berlin' LIMIT 61 ALLOW FILTERING"
        );
    }

    #[test]
    fn sorting_uses_only_explicit_keys() {
        let query = TableQuery {
            filter: Some("tenant = 1".into()),
            sort: vec![SortKey {
                column: "created".into(),
                direction: SortDirection::Descending,
            }],
            tiebreaker: vec!["id".into()],
            offset: 0,
            limit: 10,
        };
        assert_eq!(
            page_statement("shop", "events", &query),
            "SELECT * FROM \"shop\".\"events\" WHERE tenant = 1 ORDER BY \"created\" DESC LIMIT 11"
        );
    }

    #[test]
    fn counts_keep_the_filter_opt_in() {
        assert_eq!(
            count_statement("shop", "users", Some("ALLOW FILTERING")),
            "SELECT COUNT(*) FROM \"shop\".\"users\" ALLOW FILTERING"
        );
        assert_eq!(
            count_statement("shop", "users", None),
            "SELECT COUNT(*) FROM \"shop\".\"users\""
        );
    }

    #[test]
    fn words_inside_values_are_not_mistaken_for_the_clause() {
        let filter = CqlFilter::parse(Some("note = 'disallow filtering'"));
        assert!(!filter.allow_filtering);
        assert_eq!(
            filter.condition.as_deref(),
            Some("note = 'disallow filtering'")
        );
    }
}
