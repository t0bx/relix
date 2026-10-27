/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::execution_policy::ExecutionPolicy;
use crate::ids::{DataSourceId, DriverId};
use crate::network::NetworkSettings;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    #[default]
    None,
    Development,
    Testing,
    Staging,
    Production,
}

impl Environment {
    pub const ALL: [Environment; 5] = [
        Self::None,
        Self::Development,
        Self::Testing,
        Self::Staging,
        Self::Production,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Development => "dev",
            Self::Testing => "test",
            Self::Staging => "staging",
            Self::Production => "PROD",
        }
    }

    pub fn requires_confirmation(self) -> bool {
        matches!(self, Self::Production | Self::Staging)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TlsMode {
    Disable,
    #[default]
    Prefer,
    Require,
    VerifyFull,
}

impl TlsMode {
    pub const ALL: [TlsMode; 4] = [Self::Disable, Self::Prefer, Self::Require, Self::VerifyFull];

    pub fn label(self) -> &'static str {
        match self {
            Self::Disable => "disable",
            Self::Prefer => "prefer",
            Self::Require => "require",
            Self::VerifyFull => "verify-full",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TlsSettings {
    pub mode: TlsMode,
    pub ca_certificate: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ConnectionSettings {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub username: Option<String>,
    pub database: Option<String>,
    pub file_path: Option<PathBuf>,
    pub create_if_missing: bool,
    pub connect_timeout_secs: u64,
    pub tls: TlsSettings,
    pub save_password: bool,
    pub query_timeout_secs: Option<u64>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub properties: BTreeMap<String, String>,
}

impl Default for ConnectionSettings {
    fn default() -> Self {
        Self {
            host: None,
            port: None,
            username: None,
            database: None,
            file_path: None,
            create_if_missing: false,
            connect_timeout_secs: 10,
            tls: TlsSettings::default(),
            save_password: true,
            query_timeout_secs: None,
            properties: BTreeMap::new(),
        }
    }
}

impl ConnectionSettings {
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_secs.max(1))
    }

    pub fn query_timeout(&self) -> Option<Duration> {
        self.query_timeout_secs
            .filter(|seconds| *seconds > 0)
            .map(Duration::from_secs)
    }

    pub fn property(&self, name: &str) -> Option<&str> {
        self.properties
            .get(name)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSourceDefinition {
    pub id: DataSourceId,
    pub name: String,
    pub driver: DriverId,
    #[serde(default)]
    pub environment: Environment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ExecutionPolicy>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub settings: ConnectionSettings,
    #[serde(default)]
    pub network: NetworkSettings,
}

impl DataSourceDefinition {
    pub fn new(name: impl Into<String>, driver: DriverId) -> Self {
        Self {
            id: DataSourceId::new(),
            name: name.into(),
            driver,
            environment: Environment::None,
            execution_policy: None,
            color: None,
            settings: ConnectionSettings::default(),
            network: NetworkSettings::default(),
        }
    }

    /// The explicitly chosen execution policy, or the default of the source's environment.
    pub fn effective_execution_policy(&self) -> ExecutionPolicy {
        self.execution_policy
            .unwrap_or_else(|| ExecutionPolicy::for_environment(self.environment))
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionConfig {
    pub settings: ConnectionSettings,
    pub password: Option<Secret>,
    pub tls_server_name: Option<String>,
    pub read_only: bool,
}

impl ConnectionConfig {
    pub fn new(settings: ConnectionSettings, password: Option<Secret>) -> Self {
        Self {
            settings,
            password,
            tls_server_name: None,
            read_only: false,
        }
    }

    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn host_or_default(&self) -> &str {
        self.settings.host.as_deref().unwrap_or("localhost")
    }

    pub fn tls_host(&self) -> &str {
        self.tls_server_name
            .as_deref()
            .unwrap_or_else(|| self.host_or_default())
    }

    pub fn redirected(&self, host: &str, port: u16) -> Self {
        let mut redirected = self.clone();
        redirected.tls_server_name = Some(self.tls_host().to_owned());
        redirected.settings.host = Some(host.to_owned());
        redirected.settings.port = Some(port);
        redirected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let config = ConnectionConfig::new(ConnectionSettings::default(), Some(Secret::new("pw")));
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("pw\""));
        assert!(rendered.contains("Secret(***)"));
    }

    #[test]
    fn missing_settings_fields_use_defaults() {
        let settings: ConnectionSettings = serde_json::from_str("{\"host\":\"db\"}").unwrap();
        assert_eq!(settings.host.as_deref(), Some("db"));
        assert_eq!(settings.connect_timeout_secs, 10);
        assert_eq!(settings.tls.mode, TlsMode::Prefer);
        assert!(settings.properties.is_empty());
        assert_eq!(settings.query_timeout(), None);
    }

    #[test]
    fn execution_policy_falls_back_to_the_environment_default() {
        let mut definition = DataSourceDefinition::new("prod", DriverId::new("postgres"));
        definition.environment = Environment::Production;
        assert_eq!(
            definition.effective_execution_policy(),
            ExecutionPolicy::ReadOnly
        );
        definition.execution_policy = Some(ExecutionPolicy::ConfirmWrites);
        assert_eq!(
            definition.effective_execution_policy(),
            ExecutionPolicy::ConfirmWrites
        );
    }

    #[test]
    fn definitions_without_a_policy_keep_their_serialized_shape() {
        let definition = DataSourceDefinition::new("dev", DriverId::new("sqlite"));
        let encoded = serde_json::to_string(&definition).unwrap();
        assert!(!encoded.contains("execution_policy"));
    }

    #[test]
    fn redirected_configs_keep_the_tls_server_name() {
        let settings = ConnectionSettings {
            host: Some("db.internal".into()),
            port: Some(5432),
            ..ConnectionSettings::default()
        };
        let config = ConnectionConfig::new(settings, None);
        let tunneled = config.redirected("127.0.0.1", 40123);
        assert_eq!(tunneled.host_or_default(), "127.0.0.1");
        assert_eq!(tunneled.settings.port, Some(40123));
        assert_eq!(tunneled.tls_host(), "db.internal");
        assert_eq!(
            tunneled.redirected("127.0.0.1", 1).tls_host(),
            "db.internal"
        );
    }

    #[test]
    fn blank_properties_are_ignored() {
        let mut settings = ConnectionSettings::default();
        settings.properties.insert("warehouse".into(), "  ".into());
        settings
            .properties
            .insert("role".into(), " analyst ".into());
        assert_eq!(settings.property("warehouse"), None);
        assert_eq!(settings.property("role"), Some("analyst"));
    }
}
