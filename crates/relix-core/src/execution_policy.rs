/*
 * @author t0bx
 * @since 27.10.2026
 */

use crate::datasource::Environment;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionPolicy {
    ReadOnly,
    ConfirmWrites,
    Unrestricted,
}

impl ExecutionPolicy {
    pub const ALL: [ExecutionPolicy; 3] = [Self::ReadOnly, Self::ConfirmWrites, Self::Unrestricted];

    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::ConfirmWrites => "confirm writes",
            Self::Unrestricted => "unrestricted",
        }
    }

    pub fn for_environment(environment: Environment) -> Self {
        match environment {
            Environment::Production => Self::ReadOnly,
            Environment::Staging => Self::ConfirmWrites,
            Environment::None | Environment::Development | Environment::Testing => {
                Self::Unrestricted
            }
        }
    }

    pub fn is_read_only(self) -> bool {
        self == Self::ReadOnly
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_environments_default_to_stricter_policies() {
        assert_eq!(
            ExecutionPolicy::for_environment(Environment::Production),
            ExecutionPolicy::ReadOnly
        );
        assert_eq!(
            ExecutionPolicy::for_environment(Environment::Staging),
            ExecutionPolicy::ConfirmWrites
        );
        assert_eq!(
            ExecutionPolicy::for_environment(Environment::Development),
            ExecutionPolicy::Unrestricted
        );
    }

    #[test]
    fn policies_serialize_in_snake_case() {
        let encoded = serde_json::to_string(&ExecutionPolicy::ConfirmWrites).unwrap();
        assert_eq!(encoded, "\"confirm_writes\"");
    }
}
