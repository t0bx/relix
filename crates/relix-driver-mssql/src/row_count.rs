/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Dialect;
use relix_query::top_level_words;

pub const ROW_COUNT_COLUMN: &str = "__relix_rows_affected";
const MODIFYING_KEYWORDS: [&str; 4] = ["INSERT", "UPDATE", "DELETE", "MERGE"];

/// A SQL batch only reports row counts in DONE tokens, which tiberius does not expose
/// for row streams. Data modifications are therefore followed by a `SELECT @@ROWCOUNT`
/// in the same batch. Only trailing semicolons and whitespace are removed from the
/// statement, so line numbers in error messages still match the editor.
pub fn with_row_count(statement: &str) -> String {
    let body = statement.trim_end().trim_end_matches(';');
    format!("{body}\n;SELECT CAST(@@ROWCOUNT AS BIGINT) AS [{ROW_COUNT_COLUMN}]")
}

pub fn modifies_rows(statement: &str) -> bool {
    let scan = top_level_words(statement, Dialect::MsSql);
    match scan.first() {
        Some("WITH") => scan
            .words
            .iter()
            .any(|word| MODIFYING_KEYWORDS.contains(&word.as_str())),
        Some(keyword) => MODIFYING_KEYWORDS.contains(&keyword),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_modifications_are_detected() {
        assert!(modifies_rows("  -- note\n update t set a = 1"));
        assert!(modifies_rows("WITH d AS (SELECT 1 AS x) DELETE FROM t"));
        assert!(modifies_rows(
            "MERGE t USING s ON 1 = 0 WHEN NOT MATCHED THEN INSERT (a) VALUES (1);"
        ));
        assert!(!modifies_rows("SELECT 'update' FROM t"));
        assert!(!modifies_rows("WITH c AS (SELECT 1 AS x) SELECT * FROM c"));
        assert!(!modifies_rows("EXEC dbo.touch"));
    }

    #[test]
    fn row_count_follows_the_statement_in_the_same_batch() {
        assert_eq!(
            with_row_count("DELETE FROM t -- all;  \n"),
            format!(
                "DELETE FROM t -- all\n;SELECT CAST(@@ROWCOUNT AS BIGINT) AS [{ROW_COUNT_COLUMN}]"
            )
        );
    }
}
