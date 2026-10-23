/*
 * @author t0bx
 * @since 23.10.2026
 */

//! Wire types of the external connector protocol.
//!
//! An external connector is a separate executable that RELIX starts as a child process.
//! Both sides exchange newline-delimited JSON documents: requests are written to the
//! connector's stdin, responses are read from its stdout, one document per line. Several
//! requests may be in flight at the same time (for example `cancel` while `execute` is
//! running) and responses may arrive in any order; they are matched by `id`.
//!
//! Version 1 of the protocol only covers relational (SQL) connectors: every method maps to
//! [`crate::SqlIntrospector`], [`crate::QueryExecutor`] or [`crate::TableDataAccess`].
//! Key-value connectors cannot be implemented as external connectors yet.

use crate::capabilities::DriverCapabilities;
use crate::connection::ServerInfo;
use crate::driver::DriverMetadata;
use crate::error::DriverError;
use crate::sql::ExecuteOptions;
use relix_core::{
    ChangeSet, ConnectionConfig, ConnectionSettings, ObjectInfo, Secret, TableQuery, TableRef,
};
use serde::{Deserialize, Serialize};
use std::fmt;

pub const IPC_PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcRequest {
    pub id: u64,
    #[serde(flatten)]
    pub call: IpcCall,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", content = "params", rename_all = "snake_case")]
pub enum IpcCall {
    Handshake {
        protocol_version: u32,
    },
    Connect(IpcConnectionConfig),
    Ping {},
    Close {},
    ServerInfo {},
    Databases {},
    Schemas {
        database: String,
    },
    Objects {
        database: String,
        schema: Option<String>,
    },
    TableDetails {
        table: TableRef,
    },
    ObjectDefinition {
        object: ObjectInfo,
    },
    Execute {
        statement: String,
        options: ExecuteOptions,
    },
    Cancel {},
    FetchTablePage {
        table: TableRef,
        query: TableQuery,
    },
    CountRows {
        table: TableRef,
        filter: Option<String>,
    },
    ApplyChanges {
        changes: ChangeSet,
    },
}

impl IpcCall {
    pub fn method(&self) -> &'static str {
        match self {
            Self::Handshake { .. } => "handshake",
            Self::Connect(_) => "connect",
            Self::Ping {} => "ping",
            Self::Close {} => "close",
            Self::ServerInfo {} => "server_info",
            Self::Databases {} => "databases",
            Self::Schemas { .. } => "schemas",
            Self::Objects { .. } => "objects",
            Self::TableDetails { .. } => "table_details",
            Self::ObjectDefinition { .. } => "object_definition",
            Self::Execute { .. } => "execute",
            Self::Cancel {} => "cancel",
            Self::FetchTablePage { .. } => "fetch_table_page",
            Self::CountRows { .. } => "count_rows",
            Self::ApplyChanges { .. } => "apply_changes",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IpcResponse {
    pub id: u64,
    #[serde(flatten)]
    pub outcome: IpcOutcome,
}

impl IpcResponse {
    pub fn success(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            outcome: IpcOutcome::Result(result),
        }
    }

    pub fn failure(id: u64, error: DriverError) -> Self {
        Self {
            id,
            outcome: IpcOutcome::Error(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IpcOutcome {
    Result(serde_json::Value),
    Error(DriverError),
}

impl IpcOutcome {
    pub fn into_result(self) -> Result<serde_json::Value, DriverError> {
        match self {
            Self::Result(value) => Ok(value),
            Self::Error(error) => Err(error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeResult {
    pub protocol_version: u32,
    pub api_version: u32,
    pub metadata: DriverMetadata,
    pub capabilities: DriverCapabilities,
}

pub type ConnectResult = ServerInfo;

/// Connection parameters as sent to a connector. Unlike [`ConnectionConfig`] the password
/// is a plain string, because it has to cross the process boundary; it is redacted from
/// the `Debug` output and must never be logged by either side.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcConnectionConfig {
    pub settings: ConnectionSettings,
    #[serde(default)]
    pub password: Option<String>,
}

impl IpcConnectionConfig {
    pub fn into_config(self) -> ConnectionConfig {
        ConnectionConfig::new(self.settings, self.password.map(Secret::new))
    }
}

impl From<&ConnectionConfig> for IpcConnectionConfig {
    fn from(config: &ConnectionConfig) -> Self {
        Self {
            settings: config.settings.clone(),
            password: config
                .password
                .as_ref()
                .map(|secret| secret.expose().to_owned()),
        }
    }
}

impl fmt::Debug for IpcConnectionConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IpcConnectionConfig")
            .field("settings", &self.settings)
            .field("password", &self.password.as_ref().map(|_| "***"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorKind;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn requests_use_method_and_params_fields() {
        let request = IpcRequest {
            id: 7,
            call: IpcCall::Schemas {
                database: "app".into(),
            },
        };
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(
            encoded,
            json!({"id": 7, "method": "schemas", "params": {"database": "app"}})
        );
        let decoded: IpcRequest = serde_json::from_value(encoded).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn parameterless_methods_accept_empty_params() {
        let decoded: IpcRequest =
            serde_json::from_str(r#"{"id":1,"method":"ping","params":{}}"#).unwrap();
        assert_eq!(decoded.call, IpcCall::Ping {});
        assert_eq!(decoded.call.method(), "ping");
    }

    #[test]
    fn responses_carry_either_result_or_error() {
        let success = IpcResponse::success(3, json!(42));
        assert_eq!(
            serde_json::to_value(&success).unwrap(),
            json!({"id": 3, "result": 42})
        );
        let failure: IpcResponse = serde_json::from_value(json!({
            "id": 4,
            "error": {"kind": "cancelled", "message": "stopped", "detail": null, "position": null}
        }))
        .unwrap();
        assert_eq!(failure.id, 4);
        assert_eq!(
            failure.outcome.into_result().unwrap_err().kind,
            ErrorKind::Cancelled
        );
    }

    #[test]
    fn connection_config_roundtrips_password_but_redacts_debug() {
        let config = ConnectionConfig::new(ConnectionSettings::default(), Some(Secret::new("pw")));
        let wire = IpcConnectionConfig::from(&config);
        assert!(!format!("{wire:?}").contains("pw"));
        let restored = wire.into_config();
        assert_eq!(restored.password.as_ref().map(Secret::expose), Some("pw"));
    }
}
