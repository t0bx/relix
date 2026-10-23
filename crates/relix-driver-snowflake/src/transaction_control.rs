/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::Dialect;
use relix_driver_api::DriverError;
use relix_query::top_level_words;

/// The SQL API runs every request in its own session and rolls back transactions left
/// open at its end, so `BEGIN`/`COMMIT` sent as separate statements would silently do
/// nothing. They are rejected with an explanation instead. Snowflake Scripting blocks
/// (`BEGIN <statements> END`) are not transaction control and pass.
pub(crate) fn reject_transaction_control(statement: &str) -> Result<(), DriverError> {
    let words = top_level_words(statement, Dialect::Snowflake).words;
    let second = words.get(1).map(String::as_str);
    let controls_transaction = match words.first().map(String::as_str) {
        Some("BEGIN") => matches!(second, None | Some("TRANSACTION" | "WORK" | "NAME")),
        Some("START") => second == Some("TRANSACTION"),
        Some("COMMIT" | "ROLLBACK") => true,
        _ => false,
    };
    if !controls_transaction {
        return Ok(());
    }
    Err(DriverError::unsupported(
        "Snowflake's SQL API runs each statement in its own session, so transactions cannot \
         span several statements",
    )
    .with_detail(
        "run the whole transaction in one Snowflake Scripting block: \
         BEGIN BEGIN TRANSACTION; …; COMMIT; END;",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_statements_are_rejected() {
        for statement in [
            "BEGIN",
            "begin transaction;",
            "START TRANSACTION",
            "COMMIT",
            "rollback",
        ] {
            assert!(
                reject_transaction_control(statement).is_err(),
                "{statement}"
            );
        }
    }

    #[test]
    fn scripting_blocks_and_queries_pass() {
        assert!(reject_transaction_control("BEGIN SELECT 1; END").is_ok());
        assert!(reject_transaction_control("SELECT 'BEGIN'").is_ok());
        assert!(reject_transaction_control("UPDATE t SET begin = 1").is_ok());
    }
}
