/*
 * @author t0bx
 * @since 24.10.2026
 */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransactionCommand {
    Begin,
    Commit,
    Rollback,
}

impl TransactionCommand {
    /// Recognises the console commands that control an explicit transaction: `BEGIN`,
    /// `COMMIT` and `ROLLBACK`, each optionally followed by `TRANSACTION`. Cypher itself
    /// has no such statements; they are executed through Bolt transaction messages.
    pub(crate) fn parse(statement: &str) -> Option<Self> {
        let words: Vec<String> = statement
            .trim()
            .trim_end_matches(';')
            .split_whitespace()
            .map(str::to_uppercase)
            .collect();
        let command = match words.first()?.as_str() {
            "BEGIN" => Self::Begin,
            "COMMIT" => Self::Commit,
            "ROLLBACK" => Self::Rollback,
            _ => return None,
        };
        match words.get(1).map(String::as_str) {
            None => Some(command),
            Some("TRANSACTION") if words.len() == 2 => Some(command),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transaction_commands_are_recognised() {
        assert_eq!(
            TransactionCommand::parse("begin"),
            Some(TransactionCommand::Begin)
        );
        assert_eq!(
            TransactionCommand::parse(" COMMIT TRANSACTION; "),
            Some(TransactionCommand::Commit)
        );
        assert_eq!(
            TransactionCommand::parse("rollback"),
            Some(TransactionCommand::Rollback)
        );
        assert_eq!(TransactionCommand::parse("MATCH (n) RETURN n"), None);
        assert_eq!(TransactionCommand::parse("BEGIN something else"), None);
    }
}
