/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::StatementOutcome;

const DATA_MODIFICATION_KEYWORDS: [&str; 5] = ["INSERT", "UPDATE", "DELETE", "REPLACE", "WITH"];

pub fn outcome_without_rows(statement: &str, affected_rows: u64, info: &str) -> StatementOutcome {
    let keyword = leading_keyword(statement);
    if keyword
        .as_deref()
        .is_some_and(|keyword| DATA_MODIFICATION_KEYWORDS.contains(&keyword))
    {
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

/// Returns the first keyword of a statement in upper case, skipping whitespace, opening
/// parentheses and `--`, `#` and `/* */` comments.
pub fn leading_keyword(statement: &str) -> Option<String> {
    let mut rest = statement;
    loop {
        rest = rest.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '(');
        if rest.starts_with("--") || rest.starts_with('#') {
            rest = rest.split_once('\n').map_or("", |(_, tail)| tail);
        } else if let Some(comment) = rest.strip_prefix("/*") {
            rest = comment.split_once("*/").map_or("", |(_, tail)| tail);
        } else {
            break;
        }
    }
    let keyword: String = rest.chars().take_while(char::is_ascii_alphabetic).collect();
    (!keyword.is_empty()).then(|| keyword.to_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leading_keyword_skips_comments_and_parentheses() {
        assert_eq!(leading_keyword("  select 1").as_deref(), Some("SELECT"));
        assert_eq!(
            leading_keyword("-- note\n/* block */ # hash\n update t set a = 1").as_deref(),
            Some("UPDATE")
        );
        assert_eq!(leading_keyword("(SELECT 1)").as_deref(), Some("SELECT"));
        assert_eq!(leading_keyword("  ;"), None);
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
