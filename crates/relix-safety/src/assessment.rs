/*
 * @author t0bx
 * @since 27.10.2026
 */

use crate::operation::{Operation, OperationStep};
use crate::target::SafetyTarget;
use crate::verdict::{Confirmation, Denial, Verdict};
use relix_core::ExecutionPolicy;

const MAX_LISTED_STEPS: usize = 8;

/// The single enforcement point every write path consults before touching a database.
/// Read-only sources deny any writing step, `confirm writes` asks before every write, and
/// unrestricted sources only ask for risky steps on staging/production when destructive
/// confirmations are enabled. Reads are always allowed.
pub fn assess(target: &SafetyTarget, operation: &Operation<'_>) -> Verdict {
    let steps = operation.steps();
    let writes: Vec<&OperationStep> = steps.iter().filter(|step| step.writes).collect();
    if writes.is_empty() {
        return Verdict::Allow;
    }
    match target.policy {
        ExecutionPolicy::ReadOnly => Verdict::Deny(denial(target, &writes)),
        ExecutionPolicy::ConfirmWrites => Verdict::Confirm(confirmation(
            target,
            format!("Write to '{}'?", target.name),
            &writes,
            steps.len(),
        )),
        ExecutionPolicy::Unrestricted => {
            let risky: Vec<&OperationStep> = writes
                .iter()
                .copied()
                .filter(|step| step.risk.is_some())
                .collect();
            if risky.is_empty()
                || !target.confirm_destructive
                || !target.environment.requires_confirmation()
            {
                return Verdict::Allow;
            }
            Verdict::Confirm(confirmation(
                target,
                format!("Run destructive operation on '{}'?", target.name),
                &risky,
                steps.len(),
            ))
        }
    }
}

fn denial(target: &SafetyTarget, writes: &[&OperationStep]) -> Denial {
    let first = writes
        .first()
        .map(|step| step.summary.as_str())
        .unwrap_or_default();
    let others = match writes.len() {
        0 | 1 => String::new(),
        count => format!(" and {} more write(s)", count - 1),
    };
    Denial {
        reason: format!(
            "blocked: {} is {} — {first}{others}",
            target.identity(),
            target.policy_description()
        ),
    }
}

fn confirmation(
    target: &SafetyTarget,
    title: String,
    listed: &[&OperationStep],
    total: usize,
) -> Confirmation {
    let mut lines = target.summary_lines();
    lines.push(String::new());
    lines.push(if total > 1 {
        format!("{} of {total} step(s) need confirmation:", listed.len())
    } else {
        "This operation needs confirmation:".to_owned()
    });
    lines.extend(
        listed
            .iter()
            .take(MAX_LISTED_STEPS)
            .map(|step| match step.risk {
                Some(risk) => format!("  ! {} ({})", step.summary, risk.label()),
                None => format!("    {}", step.summary),
            }),
    );
    if listed.len() > MAX_LISTED_STEPS {
        lines.push(format!("  … and {} more", listed.len() - MAX_LISTED_STEPS));
    }
    Confirmation {
        title,
        lines,
        confirm_label: "Execute".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::{Dialect, Environment};

    fn target(policy: ExecutionPolicy, environment: Environment) -> SafetyTarget {
        SafetyTarget {
            name: "orders".into(),
            environment,
            database: Some("shop".into()),
            policy,
            database_enforced: false,
            confirm_destructive: true,
        }
    }

    fn statements(sql: &[&str]) -> Vec<String> {
        sql.iter()
            .map(|statement| (*statement).to_owned())
            .collect()
    }

    fn assess_sql(target: &SafetyTarget, sql: &[&str]) -> Verdict {
        let statements = statements(sql);
        assess(
            target,
            &Operation::Statements {
                statements: &statements,
                dialect: Dialect::Postgres,
            },
        )
    }

    #[test]
    fn reads_are_always_allowed() {
        for policy in ExecutionPolicy::ALL {
            let target = target(policy, Environment::Production);
            assert_eq!(
                assess_sql(&target, &["SELECT 1", "BEGIN", "COMMIT"]),
                Verdict::Allow
            );
        }
    }

    #[test]
    fn read_only_denies_scripts_containing_any_write() {
        let target = target(ExecutionPolicy::ReadOnly, Environment::Development);
        let Verdict::Deny(denial) = assess_sql(
            &target,
            &["SELECT 1", "INSERT INTO t VALUES (1)", "DELETE FROM t"],
        ) else {
            panic!("expected a denial");
        };
        assert!(denial.reason.contains("'orders' · database shop · dev"));
        assert!(
            denial
                .reason
                .contains("INSERT INTO t VALUES (1) and 1 more write(s)")
        );
    }

    #[test]
    fn confirm_writes_lists_every_write_with_its_target() {
        let target = target(ExecutionPolicy::ConfirmWrites, Environment::Staging);
        let Verdict::Confirm(confirmation) = assess_sql(
            &target,
            &[
                "SELECT 1",
                "UPDATE t SET a = 1 WHERE id = 1",
                "DELETE FROM t",
            ],
        ) else {
            panic!("expected a confirmation");
        };
        assert_eq!(confirmation.title, "Write to 'orders'?");
        assert_eq!(
            confirmation.lines[0],
            "Target: 'orders' · database shop · staging"
        );
        assert_eq!(confirmation.lines[1], "Policy: confirm writes");
        assert_eq!(confirmation.lines[3], "2 of 3 step(s) need confirmation:");
        assert_eq!(confirmation.lines[4], "    UPDATE t SET a = 1 WHERE id = 1");
        assert_eq!(
            confirmation.lines[5],
            "  ! DELETE FROM t (no effective WHERE clause)"
        );
    }

    #[test]
    fn unrestricted_only_confirms_risky_steps_on_protected_environments() {
        let production = target(ExecutionPolicy::Unrestricted, Environment::Production);
        assert_eq!(
            assess_sql(&production, &["INSERT INTO t VALUES (1)"]),
            Verdict::Allow
        );
        let Verdict::Confirm(confirmation) =
            assess_sql(&production, &["INSERT INTO t VALUES (1)", "TRUNCATE t"])
        else {
            panic!("expected a confirmation");
        };
        assert_eq!(confirmation.lines.len(), 5);
        assert!(confirmation.lines[4].contains("TRUNCATE t"));
        let development = target(ExecutionPolicy::Unrestricted, Environment::Development);
        assert_eq!(assess_sql(&development, &["DROP TABLE t"]), Verdict::Allow);
    }

    #[test]
    fn disabled_destructive_confirmations_allow_unrestricted_writes() {
        let mut production = target(ExecutionPolicy::Unrestricted, Environment::Production);
        production.confirm_destructive = false;
        assert_eq!(assess_sql(&production, &["DROP TABLE t"]), Verdict::Allow);
    }

    #[test]
    fn long_scripts_are_summarized() {
        let target = target(ExecutionPolicy::ConfirmWrites, Environment::None);
        let sql: Vec<String> = (0..12)
            .map(|index| format!("INSERT INTO t VALUES ({index})"))
            .collect();
        let refs: Vec<&str> = sql.iter().map(String::as_str).collect();
        let Verdict::Confirm(confirmation) = assess_sql(&target, &refs) else {
            panic!("expected a confirmation");
        };
        assert_eq!(confirmation.lines.last().unwrap(), "  … and 4 more");
        assert_eq!(confirmation.lines[0], "Target: 'orders' · database shop");
    }

    #[test]
    fn every_write_path_shares_the_policy() {
        let target = target(ExecutionPolicy::ReadOnly, Environment::Production);
        let changes = Operation::TableChanges {
            table: "public.users",
            changes: 2,
        };
        assert!(assess(&target, &changes).is_denied());
        let command = vec!["SET".to_owned(), "a".to_owned(), "1".to_owned()];
        assert!(assess(&target, &Operation::KeyValueCommand(&command)).is_denied());
        let write = relix_core::DocumentWrite::Insert {
            document: serde_json::json!({}),
        };
        let document = Operation::DocumentWrite {
            collection: "orders",
            write: &write,
        };
        assert!(assess(&target, &document).is_denied());
    }
}
