/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::lexer::{Token, significant, tokenize};
use crate::predicate::has_effective_predicate;
use relix_core::Dialect;

const MODIFYING_KEYWORDS: [&str; 4] = ["INSERT", "UPDATE", "DELETE", "MERGE"];
const EXPLAINABLE_KEYWORDS: [&str; 9] = [
    "SELECT", "INSERT", "UPDATE", "DELETE", "MERGE", "WITH", "VALUES", "TABLE", "REPLACE",
];

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

/// Classifies a single statement by its leading keywords. `writes` flags statements that
/// may change data or schema (transaction control does not). `destructive` flags
/// statements that should trigger a confirmation on protected connections: `DROP`,
/// `TRUNCATE`, `ALTER … DROP`, and `UPDATE`/`DELETE` without an effective top-level
/// `WHERE` clause. `EXPLAIN ANALYZE` runs its statement, so it is classified as that
/// statement.
pub fn classify(statement: &str, dialect: Dialect) -> StatementTraits {
    if let Some(traits) = crate::native_classify::classify_native(statement, dialect) {
        return traits;
    }
    let tokens = tokenize(statement, dialect);
    let words: Vec<Token<'_>> = significant(&tokens).collect();
    let first = leading_keyword(&words);
    if first == "EXPLAIN"
        && let Some(explained) = analyzed_statement(&words)
    {
        let traits = classify(&statement[explained..], dialect);
        return StatementTraits {
            kind: if traits.writes {
                traits.kind
            } else {
                StatementKind::Query
            },
            ..traits
        };
    }
    let (kind, main) = match first.as_str() {
        "SELECT" | "VALUES" | "TABLE" | "SHOW" | "EXPLAIN" | "DESCRIBE" | "DESC" | "PRAGMA" => {
            (StatementKind::Query, first.as_str())
        }
        "WITH" => with_kind(&words),
        "INSERT" | "REPLACE" | "UPSERT" | "COPY" | "LOAD" => {
            (StatementKind::Insert, first.as_str())
        }
        "UPDATE" => (StatementKind::Update, "UPDATE"),
        "DELETE" => (StatementKind::Delete, "DELETE"),
        "MERGE" => (StatementKind::Merge, "MERGE"),
        "CREATE" | "ALTER" | "DROP" | "TRUNCATE" | "RENAME" | "COMMENT" | "GRANT" | "REVOKE" => {
            (StatementKind::Definition, first.as_str())
        }
        "BEGIN" | "START" => (StatementKind::TransactionStart, ""),
        "COMMIT" | "END" => (StatementKind::TransactionEnd, ""),
        "ROLLBACK" if !contains(&words, "TO") => (StatementKind::TransactionEnd, ""),
        "SET" | "USE" | "RESET" | "ATTACH" | "DETACH" => (StatementKind::Session, ""),
        _ => (StatementKind::Other, ""),
    };
    let destructive = match main {
        "DROP" | "TRUNCATE" => true,
        "ALTER" => contains(&words, "DROP"),
        "UPDATE" | "DELETE" => !has_effective_predicate(&words),
        _ => false,
    };
    StatementTraits {
        kind,
        destructive,
        writes: kind_writes(kind),
    }
}

pub(crate) fn kind_writes(kind: StatementKind) -> bool {
    !matches!(
        kind,
        StatementKind::Query
            | StatementKind::Session
            | StatementKind::TransactionStart
            | StatementKind::TransactionEnd
    )
}

/// Byte offset of the statement an `EXPLAIN ANALYZE` executes, skipping option lists such
/// as `(ANALYZE, BUFFERS)`.
fn analyzed_statement(words: &[Token<'_>]) -> Option<usize> {
    if !contains(words, "ANALYZE") && !contains(words, "ANALYSE") {
        return None;
    }
    let mut depth = 0usize;
    for token in words.iter().skip(1) {
        if token.is_symbol("(") {
            depth += 1;
        } else if token.is_symbol(")") {
            depth = depth.saturating_sub(1);
        } else if depth == 0
            && EXPLAINABLE_KEYWORDS
                .iter()
                .any(|keyword| token.is_keyword(keyword))
        {
            return Some(token.start);
        }
    }
    None
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

/// The kind of a statement with common table expressions: its main statement after the
/// CTE list decides, and data-modifying CTEs make an otherwise reading query a write.
fn with_kind(words: &[Token<'_>]) -> (StatementKind, &'static str) {
    let mut depth = 0usize;
    let mut main = None;
    for token in words.iter().skip(1) {
        if token.is_symbol("(") {
            depth += 1;
        } else if token.is_symbol(")") {
            depth = depth.saturating_sub(1);
        } else if depth == 0 && token.is_keyword("SELECT") {
            main = Some("SELECT");
            break;
        } else if depth == 0
            && let Some(keyword) = MODIFYING_KEYWORDS
                .iter()
                .find(|keyword| token.is_keyword(keyword))
        {
            main = Some(*keyword);
            break;
        }
    }
    match main {
        Some("UPDATE") => (StatementKind::Update, "UPDATE"),
        Some("DELETE") => (StatementKind::Delete, "DELETE"),
        Some("INSERT") => (StatementKind::Insert, "INSERT"),
        Some("MERGE") => (StatementKind::Merge, "MERGE"),
        _ if MODIFYING_KEYWORDS
            .iter()
            .any(|keyword| contains(words, keyword)) =>
        {
            (StatementKind::Other, "")
        }
        _ => (StatementKind::Query, "SELECT"),
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
        assert!(!classify("COMMIT", Dialect::Generic).writes);
        assert!(!classify("ROLLBACK", Dialect::Generic).writes);
    }

    #[test]
    fn trivial_or_nested_predicates_stay_destructive() {
        let destructive = |sql| classify(sql, Dialect::Postgres).destructive;
        assert!(destructive("DELETE FROM users WHERE 1=1"));
        assert!(destructive(
            "UPDATE users SET a = (SELECT b FROM c WHERE c.id = 1)"
        ));
        assert!(!destructive(
            "DELETE FROM users WHERE id IN (SELECT id FROM gone)"
        ));
    }

    #[test]
    fn common_table_expressions_follow_their_main_statement() {
        let traits = classify(
            "WITH old AS (SELECT 1) DELETE FROM users",
            Dialect::Postgres,
        );
        assert_eq!(traits.kind, StatementKind::Delete);
        assert!(traits.writes && traits.destructive);
        let filtered = classify(
            "WITH old AS (SELECT id FROM t) DELETE FROM users WHERE id IN (SELECT id FROM old)",
            Dialect::Postgres,
        );
        assert!(filtered.writes && !filtered.destructive);
    }

    #[test]
    fn explain_analyze_is_classified_as_the_statement_it_runs() {
        let plain = classify("EXPLAIN DELETE FROM users", Dialect::Postgres);
        assert_eq!(plain.kind, StatementKind::Query);
        assert!(!plain.writes);
        let analyzed = classify(
            "EXPLAIN (ANALYZE, BUFFERS) DELETE FROM users",
            Dialect::Postgres,
        );
        assert_eq!(analyzed.kind, StatementKind::Delete);
        assert!(analyzed.writes && analyzed.destructive);
        let select = classify("EXPLAIN ANALYZE SELECT * FROM users", Dialect::Postgres);
        assert_eq!(select.kind, StatementKind::Query);
        assert!(!select.writes);
    }
}
