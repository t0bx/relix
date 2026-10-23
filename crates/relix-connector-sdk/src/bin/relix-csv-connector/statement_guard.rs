/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::DriverError;

const READ_KEYWORDS: [&str; 4] = ["select", "with", "values", "explain"];

/// Accepts only statements that start (after comments and parentheses) with a read-only
/// keyword. The database is additionally switched to `query_only`, so a data-modifying
/// `WITH` statement is still refused by SQLite itself.
pub(crate) fn ensure_read_only(statement: &str) -> Result<(), DriverError> {
    let keyword: String = skip_comments(statement)
        .trim_start_matches(|ch: char| ch == '(' || ch.is_whitespace())
        .chars()
        .take_while(|ch| ch.is_ascii_alphabetic())
        .collect::<String>()
        .to_ascii_lowercase();
    if READ_KEYWORDS.contains(&keyword.as_str()) {
        return Ok(());
    }
    Err(DriverError::unsupported(
        "CSV data sources are read-only; only SELECT, WITH, VALUES and EXPLAIN are allowed",
    ))
}

fn skip_comments(mut text: &str) -> &str {
    loop {
        text = text.trim_start();
        if let Some(rest) = text.strip_prefix("--") {
            text = rest.split_once('\n').map_or("", |(_, after)| after);
        } else if let Some(rest) = text.strip_prefix("/*") {
            text = rest.split_once("*/").map_or("", |(_, after)| after);
        } else {
            return text;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_reading_statements_pass() {
        assert!(ensure_read_only("SELECT 1").is_ok());
        assert!(ensure_read_only("-- note\n /* x */ (select 1)").is_ok());
        assert!(ensure_read_only("with t as (select 1) select * from t").is_ok());
        assert!(ensure_read_only("DELETE FROM people").is_err());
        assert!(ensure_read_only("PRAGMA query_only = 0").is_err());
        assert!(ensure_read_only("").is_err());
    }
}
