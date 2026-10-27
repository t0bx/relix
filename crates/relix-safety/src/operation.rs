/*
 * @author t0bx
 * @since 27.10.2026
 */

use crate::key_value_commands::classify_command;
use crate::risk::Risk;
use relix_core::keyvalue::KeyValueWrite;
use relix_core::value::truncate_chars;
use relix_core::{Dialect, DocumentWrite};
use relix_query::{StatementKind, StatementTraits, classify};

const SUMMARY_WIDTH: usize = 70;

#[derive(Debug, Clone, Copy)]
pub enum Operation<'a> {
    Statements {
        statements: &'a [String],
        dialect: Dialect,
    },
    TableChanges {
        table: &'a str,
        changes: usize,
    },
    KeyValueWrites(&'a [KeyValueWrite]),
    KeyValueCommand(&'a [String]),
    DocumentWrite {
        collection: &'a str,
        write: &'a DocumentWrite,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationStep {
    pub summary: String,
    pub writes: bool,
    pub risk: Option<Risk>,
}

impl OperationStep {
    fn write(summary: String, risk: Option<Risk>) -> Self {
        Self {
            summary,
            writes: true,
            risk,
        }
    }
}

impl Operation<'_> {
    /// Splits the operation into the individually assessed steps, so a multi-statement
    /// script is judged as a whole while every statement keeps its own risk.
    pub fn steps(&self) -> Vec<OperationStep> {
        match *self {
            Self::Statements {
                statements,
                dialect,
            } => statements
                .iter()
                .map(|statement| statement_step(statement, dialect))
                .collect(),
            Self::TableChanges { table, changes } => vec![OperationStep::write(
                format!("{changes} staged change(s) to {table}"),
                None,
            )],
            Self::KeyValueWrites(writes) => writes.iter().map(key_value_write_step).collect(),
            Self::KeyValueCommand(arguments) => {
                let traits = classify_command(arguments);
                vec![OperationStep {
                    summary: single_line(&arguments.join(" ")),
                    writes: traits.writes,
                    risk: traits.risk,
                }]
            }
            Self::DocumentWrite { collection, write } => {
                vec![document_write_step(collection, write)]
            }
        }
    }
}

fn statement_step(statement: &str, dialect: Dialect) -> OperationStep {
    let traits = classify(statement, dialect);
    OperationStep {
        summary: single_line(statement),
        writes: traits.writes,
        risk: statement_risk(traits),
    }
}

fn statement_risk(traits: StatementTraits) -> Option<Risk> {
    if !traits.destructive {
        return None;
    }
    Some(match traits.kind {
        StatementKind::Update | StatementKind::Delete | StatementKind::Merge => {
            Risk::UnboundedWrite
        }
        StatementKind::Definition => Risk::DestructiveDefinition,
        _ => Risk::DataRemoval,
    })
}

fn key_value_write_step(write: &KeyValueWrite) -> OperationStep {
    let (summary, risk) = match write {
        KeyValueWrite::SetString { key, .. } => (format!("SET {}", key.display()), None),
        KeyValueWrite::HashSet { key, .. } => (format!("HSET {}", key.display()), None),
        KeyValueWrite::HashDelete { key, .. } => (format!("HDEL {}", key.display()), None),
        KeyValueWrite::ListPush { key, tail, .. } => (
            format!(
                "{} {}",
                if *tail { "RPUSH" } else { "LPUSH" },
                key.display()
            ),
            None,
        ),
        KeyValueWrite::ListSet { key, index, .. } => {
            (format!("LSET {} {index}", key.display()), None)
        }
        KeyValueWrite::SetAdd { key, .. } => (format!("SADD {}", key.display()), None),
        KeyValueWrite::SetRemove { key, .. } => (format!("SREM {}", key.display()), None),
        KeyValueWrite::SortedSetAdd { key, .. } => (format!("ZADD {}", key.display()), None),
        KeyValueWrite::SortedSetRemove { key, .. } => (format!("ZREM {}", key.display()), None),
        KeyValueWrite::Expire { key, seconds } => {
            (format!("EXPIRE {} {seconds}", key.display()), None)
        }
        KeyValueWrite::Persist { key } => (format!("PERSIST {}", key.display()), None),
        KeyValueWrite::Rename { key, new_key } => (
            format!("RENAME {} {}", key.display(), new_key.display()),
            None,
        ),
        KeyValueWrite::Delete { key } => {
            (format!("DEL {}", key.display()), Some(Risk::DataRemoval))
        }
    };
    OperationStep::write(single_line(&summary), risk)
}

fn document_write_step(collection: &str, write: &DocumentWrite) -> OperationStep {
    let (summary, risk) = match write {
        DocumentWrite::Insert { .. } => (format!("insert a document into {collection}"), None),
        DocumentWrite::Replace { key, .. } => {
            (format!("replace document {key} in {collection}"), None)
        }
        DocumentWrite::Delete { key } => (
            format!("delete document {key} from {collection}"),
            Some(Risk::DataRemoval),
        ),
        DocumentWrite::CreateIndex { .. } => (format!("create an index on {collection}"), None),
        DocumentWrite::DropIndex { name } => (
            format!("drop index {name} on {collection}"),
            Some(Risk::DestructiveDefinition),
        ),
    };
    OperationStep::write(single_line(&summary), risk)
}

fn single_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate_chars(&collapsed, SUMMARY_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::keyvalue::KeyName;

    #[test]
    fn scripts_yield_one_step_per_statement() {
        let statements = vec![
            "SELECT 1".to_owned(),
            "DELETE FROM users".to_owned(),
            "DROP TABLE logs".to_owned(),
        ];
        let steps = Operation::Statements {
            statements: &statements,
            dialect: Dialect::Postgres,
        }
        .steps();
        assert_eq!(steps.len(), 3);
        assert!(!steps[0].writes);
        assert_eq!(steps[1].risk, Some(Risk::UnboundedWrite));
        assert_eq!(steps[2].risk, Some(Risk::DestructiveDefinition));
    }

    #[test]
    fn summaries_are_single_lines() {
        let statements = vec!["UPDATE users\n   SET a = 1\n WHERE id = 2".to_owned()];
        let steps = Operation::Statements {
            statements: &statements,
            dialect: Dialect::MySql,
        }
        .steps();
        assert_eq!(steps[0].summary, "UPDATE users SET a = 1 WHERE id = 2");
        assert_eq!(steps[0].risk, None);
    }

    #[test]
    fn key_value_deletes_remove_data() {
        let write = KeyValueWrite::Delete {
            key: KeyName::from("session:1"),
        };
        let steps = Operation::KeyValueWrites(&[write]).steps();
        assert_eq!(steps[0].summary, "DEL session:1");
        assert_eq!(steps[0].risk, Some(Risk::DataRemoval));
    }

    #[test]
    fn console_reads_are_not_writes() {
        let arguments = vec!["GET".to_owned(), "a".to_owned()];
        assert!(!Operation::KeyValueCommand(&arguments).steps()[0].writes);
    }
}
