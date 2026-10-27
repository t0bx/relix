/*
 * @author t0bx
 * @since 27.10.2026
 */

use relix_core::{DataSourceDefinition, Environment, ExecutionPolicy};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SafetyTarget {
    pub name: String,
    pub environment: Environment,
    pub database: Option<String>,
    pub policy: ExecutionPolicy,
    pub database_enforced: bool,
    pub confirm_destructive: bool,
}

impl SafetyTarget {
    /// Describes the data source an operation would run against. `read_only_sessions`
    /// tells whether the driver makes the database itself refuse writes for read-only
    /// sources; `confirm_destructive` is the project/global destructive-statement setting.
    pub fn from_definition(
        definition: &DataSourceDefinition,
        read_only_sessions: bool,
        confirm_destructive: bool,
    ) -> Self {
        let policy = definition.effective_execution_policy();
        Self {
            name: definition.name.clone(),
            environment: definition.environment,
            database: definition
                .settings
                .database
                .clone()
                .filter(|database| !database.trim().is_empty()),
            policy,
            database_enforced: policy.is_read_only() && read_only_sessions,
            confirm_destructive,
        }
    }

    pub fn identity(&self) -> String {
        let mut parts = vec![format!("'{}'", self.name)];
        if let Some(database) = &self.database {
            parts.push(format!("database {database}"));
        }
        if self.environment != Environment::None {
            parts.push(self.environment.label().to_owned());
        }
        parts.join(" · ")
    }

    pub fn policy_description(&self) -> String {
        match (self.policy, self.database_enforced) {
            (ExecutionPolicy::ReadOnly, true) => "read-only (enforced by the database)".into(),
            (ExecutionPolicy::ReadOnly, false) => "read-only (checked by RELIX only)".into(),
            (policy, _) => policy.label().into(),
        }
    }

    /// The header every confirmation shows, so a prompt always names its exact target.
    pub fn summary_lines(&self) -> Vec<String> {
        vec![
            format!("Target: {}", self.identity()),
            format!("Policy: {}", self.policy_description()),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::DriverId;

    fn production() -> DataSourceDefinition {
        let mut definition = DataSourceDefinition::new("orders", DriverId::new("postgres"));
        definition.environment = Environment::Production;
        definition.settings.database = Some("shop".into());
        definition
    }

    #[test]
    fn identity_names_source_database_and_environment() {
        let target = SafetyTarget::from_definition(&production(), true, true);
        assert_eq!(target.identity(), "'orders' · database shop · PROD");
        assert_eq!(
            target.summary_lines()[1],
            "Policy: read-only (enforced by the database)"
        );
    }

    #[test]
    fn read_only_without_driver_support_is_only_client_checked() {
        let target = SafetyTarget::from_definition(&production(), false, true);
        assert!(!target.database_enforced);
        assert_eq!(
            target.policy_description(),
            "read-only (checked by RELIX only)"
        );
    }

    #[test]
    fn enforcement_only_applies_to_read_only_policies() {
        let mut definition = production();
        definition.execution_policy = Some(ExecutionPolicy::ConfirmWrites);
        let target = SafetyTarget::from_definition(&definition, true, true);
        assert!(!target.database_enforced);
        assert_eq!(target.policy_description(), "confirm writes");
    }
}
