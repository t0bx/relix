/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::lexer::{Token, significant, tokenize};
use relix_core::Dialect;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    Query,
    Insert,
    Update,
    Delete,
    Merge,
    Definition,
    TransactionStart,
    TransactionEnd,
    Session,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatementTraits {
    pub kind: StatementKind,
    pub destructive: bool,
    pub writes: bool,
}

/// Classifies a single statement by its leading keywords. `destructive` flags statements
/// that should trigger a confirmation on protected connections: `DROP`, `TRUNCATE`,
/// `ALTER … DROP`, and `UPDATE`/`DELETE` without a `WHERE` clause.
pub fn classify(statement: &str, dialect: Dialect) -> StatementTraits {
    let tokens = tokenize(statement, dialect);
    let words: Vec<Token<'_>> = significant(&tokens).collect();
    let first = leading_keyword(&words);
    let kind = match first.as_str() {
        "SELECT" | "VALUES" | "TABLE" | "SHOW" | "EXPLAIN" | "DESCRIBE" | "DESC" | "PRAGMA" => {
            StatementKind::Query
        }
        "WITH" => with_kind(&words),
        "INSERT" | "REPLACE" | "UPSERT" | "COPY" | "LOAD" => StatementKind::Insert,
        "UPDATE" => StatementKind::Update,
        "DELETE" => StatementKind::Delete,
        "MERGE" => StatementKind::Merge,
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "RENAME" | "COMMENT" | "GRANT" | "REVOKE" => {
            StatementKind::Definition
        }
        "BEGIN" | "START" => StatementKind::TransactionStart,
        "COMMIT" | "END" => StatementKind::TransactionEnd,
        "ROLLBACK" if !contains(&words, "TO") => StatementKind::TransactionEnd,
        "SET" | "USE" | "RESET" | "ATTACH" | "DETACH" => StatementKind::Session,
        _ => StatementKind::Other,
    };
    let destructive = match first.as_str() {
        "DROP" | "TRUNCATE" => true,
        "ALTER" => contains(&words, "DROP"),
        "UPDATE" | "DELETE" => !contains(&words, "WHERE"),
        _ => false,
    };
    let writes = !matches!(
        kind,
        StatementKind::Query | StatementKind::Session | StatementKind::TransactionStart
    );
    StatementTraits {
        kind,
        destructive,
        writes,
    }
}

fn leading_keyword(words: &[Token<'_>]) -> String {
    words
        .iter()
        .find(|token| token.kind.is_word())
        .map(|token| token.text.to_ascii_uppercase())
        .unwrap_or_default()
}

fn contains(words: &[Token<'_>], keyword: &str) -> bool {
    words.iter().any(|token| token.is_keyword(keyword))
}

fn with_kind(words: &[Token<'_>]) -> StatementKind {
    let modifies = ["INSERT", "UPDATE", "DELETE", "MERGE"]
        .iter()
        .any(|keyword| contains(words, keyword));
    if modifies {
        StatementKind::Other
    } else {
        StatementKind::Query
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind(sql: &str) -> StatementKind {
        classify(sql, Dialect::Postgres).kind
    }

    #[test]
    fn classifies_leading_keyword() {
        assert_eq!(kind("  -- note\n select 1"), StatementKind::Query);
        assert_eq!(
            kind("WITH a AS (SELECT 1) SELECT * FROM a"),
            StatementKind::Query
        );
        assert_eq!(
            kind("WITH d AS (DELETE FROM t RETURNING *) SELECT 1"),
            StatementKind::Other
        );
        assert_eq!(kind("begin"), StatementKind::TransactionStart);
        assert_eq!(kind("ROLLBACK"), StatementKind::TransactionEnd);
        assert_eq!(kind("ROLLBACK TO SAVEPOINT a"), StatementKind::Other);
        assert_eq!(kind("create table x(a int)"), StatementKind::Definition);
    }

    #[test]
    fn flags_destructive_statements() {
        let destructive = |sql| classify(sql, Dialect::MySql).destructive;
        assert!(destructive("DROP TABLE users"));
        assert!(destructive("delete from users"));
        assert!(destructive("UPDATE users SET a = 1"));
        assert!(!destructive("UPDATE users SET a = 1 WHERE id = 3"));
        assert!(destructive("ALTER TABLE users DROP COLUMN a"));
        assert!(!destructive("SELECT 'drop table'"));
    }

    #[test]
    fn write_detection() {
        assert!(!classify("select 1", Dialect::Generic).writes);
        assert!(classify("insert into t values (1)", Dialect::Generic).writes);
    }
}
