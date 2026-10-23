/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Dialect;
use relix_query::{TopLevelWords, completion_message, top_level_words};

const QUERY_VERBS: [&str; 12] = [
    "SELECT",
    "FROM",
    "WITH",
    "VALUES",
    "TABLE",
    "SHOW",
    "DESCRIBE",
    "DESC",
    "SUMMARIZE",
    "EXPLAIN",
    "PIVOT",
    "UNPIVOT",
];
const DATA_CHANGE_VERBS: [&str; 5] = ["INSERT", "UPDATE", "DELETE", "MERGE", "COPY"];
const STATUS_COLUMNS: [(&str, &str); 2] = [("Count", "BIGINT"), ("Success", "BOOLEAN")];

/// Decides how a statement's result is reported. DuckDB answers every statement with
/// a result set, including DDL (`Count`/`Success` columns), so the leading keyword tells
/// apart a real one-column query from a status result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatementPlan {
    words: TopLevelWords,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusResult {
    ChangedRows,
    Completed,
}

impl StatementPlan {
    pub(crate) fn of(sql: &str) -> Self {
        Self {
            words: top_level_words(sql, Dialect::DuckDb),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.words.words.is_empty()
    }

    pub(crate) fn has_multiple_statements(&self) -> bool {
        self.words.has_multiple_statements
    }

    pub(crate) fn status_result(&self, columns: &[(String, String)]) -> Option<StatusResult> {
        let verb = self.words.first()?;
        if QUERY_VERBS.contains(&verb) || self.has_word("RETURNING") {
            return None;
        }
        let [(name, type_name)] = columns else {
            return None;
        };
        if !STATUS_COLUMNS.contains(&(name.as_str(), type_name.as_str())) {
            return None;
        }
        if name == "Count" && DATA_CHANGE_VERBS.contains(&verb) {
            Some(StatusResult::ChangedRows)
        } else {
            Some(StatusResult::Completed)
        }
    }

    pub(crate) fn completion_message(&self) -> String {
        completion_message(&self.words)
    }

    fn has_word(&self, word: &str) -> bool {
        self.words.words.iter().any(|candidate| candidate == word)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn columns(name: &str, type_name: &str) -> Vec<(String, String)> {
        vec![(name.to_owned(), type_name.to_owned())]
    }

    #[test]
    fn status_results_are_recognised_by_verb_and_shape() {
        let insert = StatementPlan::of("INSERT INTO t VALUES (1)");
        assert_eq!(
            insert.status_result(&columns("Count", "BIGINT")),
            Some(StatusResult::ChangedRows)
        );
        let create = StatementPlan::of("create table t (a int)");
        assert_eq!(
            create.status_result(&columns("Count", "BIGINT")),
            Some(StatusResult::Completed)
        );
        assert_eq!(create.completion_message(), "CREATE TABLE");
        let begin = StatementPlan::of("BEGIN");
        assert_eq!(
            begin.status_result(&columns("Success", "BOOLEAN")),
            Some(StatusResult::Completed)
        );
    }

    #[test]
    fn queries_and_returning_clauses_keep_their_rows() {
        let query = StatementPlan::of("SELECT count(*) AS \"Count\" FROM t");
        assert_eq!(query.status_result(&columns("Count", "BIGINT")), None);
        let returning = StatementPlan::of("DELETE FROM t RETURNING id AS \"Count\"");
        assert_eq!(returning.status_result(&columns("Count", "BIGINT")), None);
        let pragma = StatementPlan::of("PRAGMA table_info('t')");
        assert_eq!(pragma.status_result(&columns("cid", "INTEGER")), None);
    }
}
