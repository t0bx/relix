/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Dialect;
use relix_query::top_level_words;

const DATA_CHANGING_VERBS: [&str; 4] = ["INSERT", "UPDATE", "DELETE", "REPLACE"];
const DDL_VERBS: [&str; 3] = ["CREATE", "DROP", "ALTER"];
const DDL_OBJECTS: [&str; 4] = ["TABLE", "VIEW", "INDEX", "TRIGGER"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StatementKind {
    keywords: Vec<String>,
}

impl StatementKind {
    pub(crate) fn of(sql: &str) -> Self {
        Self {
            keywords: top_level_words(sql, Dialect::Sqlite).words,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.keywords.is_empty()
    }

    pub(crate) fn reports_affected_rows(&self, readonly: bool) -> bool {
        match self.keywords.first().map(String::as_str) {
            Some(verb) if DATA_CHANGING_VERBS.contains(&verb) => true,
            Some("WITH") => !readonly,
            _ => false,
        }
    }

    pub(crate) fn completion_message(&self) -> String {
        let Some(verb) = self.keywords.first() else {
            return "OK".to_owned();
        };
        if !DDL_VERBS.contains(&verb.as_str()) {
            return verb.clone();
        }
        self.keywords[1..]
            .iter()
            .find(|word| DDL_OBJECTS.contains(&word.as_str()))
            .map_or_else(|| verb.clone(), |object| format!("{verb} {object}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_whitespace_are_skipped() {
        let kind = StatementKind::of("  -- note\n /* block */ create temp table t (id int)");
        assert_eq!(kind.completion_message(), "CREATE TABLE");
    }

    #[test]
    fn ddl_messages_name_the_object_kind() {
        assert_eq!(
            StatementKind::of("CREATE UNIQUE INDEX i ON t (a)").completion_message(),
            "CREATE INDEX"
        );
        assert_eq!(
            StatementKind::of("drop view v").completion_message(),
            "DROP VIEW"
        );
        assert_eq!(StatementKind::of("begin").completion_message(), "BEGIN");
    }

    #[test]
    fn data_changes_report_affected_rows() {
        assert!(StatementKind::of("insert into t values (1)").reports_affected_rows(false));
        assert!(
            StatementKind::of("WITH x AS (SELECT 1) DELETE FROM t").reports_affected_rows(false)
        );
        assert!(!StatementKind::of("CREATE TABLE t (a)").reports_affected_rows(false));
    }

    #[test]
    fn comment_only_input_is_empty() {
        assert!(StatementKind::of(" -- nothing\n ; /* here */").is_empty());
    }
}
