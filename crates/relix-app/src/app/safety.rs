/*
 * @author t0bx
 * @since 27.10.2026
 */

use super::App;
use crate::notifications::Level;
use crate::overlay::{ConfirmState, Overlay, PendingAction};
use relix_core::DataSourceId;
use relix_safety::{Operation, SafetyTarget, Verdict, assess};

impl App {
    pub fn safety_target(&self, source: DataSourceId) -> Option<SafetyTarget> {
        let definition = self.data_source(source)?;
        let read_only_sessions = self
            .capabilities(source)
            .is_some_and(|capabilities| capabilities.read_only_sessions);
        let confirm_destructive = self
            .project
            .as_ref()
            .and_then(|project| project.settings.confirm_destructive)
            .unwrap_or(self.settings.query.confirm_destructive);
        Some(SafetyTarget::from_definition(
            definition,
            read_only_sessions,
            confirm_destructive,
        ))
    }

    pub fn is_read_only(&self, source: DataSourceId) -> bool {
        self.data_source(source)
            .is_some_and(|definition| definition.effective_execution_policy().is_read_only())
    }

    /// Consults the execution policy right before a write runs. Returns `true` when the
    /// write may proceed; denials are reported, and required confirmations are queued as
    /// `action`, which re-enters the write path with `confirmed` set. Denials can never be
    /// confirmed away, so re-entering still re-checks the current policy.
    pub(crate) fn guard_write(
        &mut self,
        source: DataSourceId,
        operation: &Operation<'_>,
        confirmed: bool,
        action: impl FnOnce() -> PendingAction,
    ) -> bool {
        let Some((target, verdict)) = self.assess_write(source, operation) else {
            return false;
        };
        match verdict {
            Verdict::Allow => true,
            Verdict::Confirm(_) if confirmed => true,
            Verdict::Confirm(confirmation) => {
                self.overlays.push(Overlay::Confirm(ConfirmState {
                    title: confirmation.title,
                    lines: confirmation.lines,
                    confirm_label: confirmation.confirm_label,
                    danger: true,
                    action: action(),
                }));
                false
            }
            Verdict::Deny(denial) => {
                self.report_denial(&target, denial.reason);
                false
            }
        }
    }

    /// Shows a confirmation the workflow always asks for (deletes, staged grid changes),
    /// headed by the exact target and policy. Writes the policy denies never get a dialog.
    pub(crate) fn confirm_write(
        &mut self,
        source: DataSourceId,
        operation: &Operation<'_>,
        confirm: ConfirmState,
    ) {
        let Some((target, verdict)) = self.assess_write(source, operation) else {
            return;
        };
        let danger = match verdict {
            Verdict::Deny(denial) => {
                self.report_denial(&target, denial.reason);
                return;
            }
            Verdict::Confirm(_) => true,
            Verdict::Allow => confirm.danger,
        };
        let mut lines = target.summary_lines();
        lines.push(String::new());
        lines.extend(confirm.lines);
        self.overlays.push(Overlay::Confirm(ConfirmState {
            lines,
            danger,
            ..confirm
        }));
    }

    fn assess_write(
        &mut self,
        source: DataSourceId,
        operation: &Operation<'_>,
    ) -> Option<(SafetyTarget, Verdict)> {
        let Some(target) = self.safety_target(source) else {
            self.notify(Level::Error, "the data source no longer exists");
            return None;
        };
        let verdict = assess(&target, operation);
        Some((target, verdict))
    }

    fn report_denial(&mut self, target: &SafetyTarget, reason: String) {
        tracing::info!(
            data_source = %target.name,
            policy = target.policy.label(),
            "write blocked by the execution policy"
        );
        self.notify(Level::Error, reason);
    }
}
