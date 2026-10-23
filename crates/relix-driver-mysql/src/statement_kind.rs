/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::{Dialect, StatementOutcome};
use relix_query::top_level_words;

const DATA_MODIFICATION_KEYWORDS: [&str; 5] = ["INSERT", "UPDATE", "DELETE", "REPLACE", "WITH"];

pub fn outcome_without_rows(statement: &str, affected_rows: u64, info: &str) -> StatementOutcome {
    let words = top_level_words(statement, Dialect::MySql);
    let keyword = words.first();
    if keyword.is_some_and(|keyword| DATA_MODIFICATION_KEYWORDS.contains(&keyword)) {
        return StatementOutcome::Affected {
            rows: affected_rows,
        };
    }
    let message = if !info.trim().is_empty() {
        info.trim().to_owned()
    } else {
        match keyword {
            Some(keyword) => format!("{keyword} completed"),
            None => "statement completed".to_owned(),
        }
    };
    StatementOutcome::Completed { message }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_are_skipped_before_the_verb() {
        assert_eq!(
            outcome_without_rows("-- note\n/* block */ # hash\n update t set a = 1", 2, ""),
            StatementOutcome::Affected { rows: 2 }
        );
        assert_eq!(
            outcome_without_rows("  ;", 0, ""),
            StatementOutcome::Completed {
                message: "statement completed".into()
            }
        );
    }

    #[test]
    fn dml_reports_affected_rows() {
        assert_eq!(
            outcome_without_rows("delete from t", 3, ""),
            StatementOutcome::Affected { rows: 3 }
        );
        assert_eq!(
            outcome_without_rows("REPLACE INTO t VALUES (1)", 1, ""),
            StatementOutcome::Affected { rows: 1 }
        );
    }

    #[test]
    fn other_statements_complete_with_message() {
        assert_eq!(
            outcome_without_rows("create table t (id int)", 0, ""),
            StatementOutcome::Completed {
                message: "CREATE completed".into()
            }
        );
        assert_eq!(
            outcome_without_rows("ALTER TABLE t ADD c INT", 0, "Records: 0  Duplicates: 0"),
            StatementOutcome::Completed {
                message: "Records: 0  Duplicates: 0".into()
            }
        );
    }
}
