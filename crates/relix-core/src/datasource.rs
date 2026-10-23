/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::ids::{DataSourceId, DriverId};
use serde::{Deserialize, Serialize};
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
        }
    }
}

impl ConnectionSettings {
    pub fn connect_timeout(&self) -> Duration {
        Duration::from_secs(self.connect_timeout_secs.max(1))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataSourceDefinition {
    pub id: DataSourceId,
    pub name: String,
    pub driver: DriverId,
    #[serde(default)]
    pub environment: Environment,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub settings: ConnectionSettings,
}

impl DataSourceDefinition {
    pub fn new(name: impl Into<String>, driver: DriverId) -> Self {
        Self {
            id: DataSourceId::new(),
            name: name.into(),
            driver,
            environment: Environment::None,
            color: None,
            settings: ConnectionSettings::default(),
        }
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
}

impl ConnectionConfig {
    pub fn new(settings: ConnectionSettings, password: Option<Secret>) -> Self {
        Self { settings, password }
    }

    pub fn host_or_default(&self) -> &str {
        self.settings.host.as_deref().unwrap_or("localhost")
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
    }
}
