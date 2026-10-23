/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::client::IpcClient;
use super::connection::ExternalConnection;
use super::handshake::perform_handshake;
use crate::installed::InstalledDriver;
use async_trait::async_trait;
use relix_core::ConnectionConfig;
use relix_driver_api::ipc::{IpcCall, IpcConnectionConfig};
use relix_driver_api::{
    DatabaseConnection, DatabaseDriver, DriverCapabilities, DriverError, DriverMetadata,
    DriverOrigin, ErrorKind, ServerInfo, with_timeout,
};
use std::path::{Path, PathBuf};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const PROBE_CLOSE_GRACE: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct ExternalDriver {
    executable: PathBuf,
    metadata: DriverMetadata,
    capabilities: DriverCapabilities,
}

impl ExternalDriver {
    pub fn new(
        executable: impl Into<PathBuf>,
        metadata: DriverMetadata,
        capabilities: DriverCapabilities,
    ) -> Self {
        Self {
            executable: executable.into(),
            metadata,
            capabilities,
        }
    }

    pub fn from_installed(installed: &InstalledDriver, drivers_dir: &Path) -> Self {
        Self::new(
            installed.executable_path(drivers_dir),
            installed.metadata(),
            installed.capabilities,
        )
    }

    /// Starts `executable` once to learn its metadata through the handshake, then stops it.
    /// Intended for connector development; installed packages are described by their
    /// manifest instead and are never started before the user approved them.
    pub async fn probe(executable: impl Into<PathBuf>) -> Result<Self, DriverError> {
        let executable = executable.into();
        let client = IpcClient::spawn(&executable)?;
        let handshake = perform_handshake(&client, PROBE_TIMEOUT).await;
        let _ = client.shutdown(PROBE_CLOSE_GRACE).await;
        let handshake = handshake?;
        let mut metadata = handshake.metadata;
        metadata.origin = DriverOrigin::External {
            package: format!("local:{}", executable.display()),
        };
        Ok(Self::new(executable, metadata, handshake.capabilities))
    }

    pub fn executable(&self) -> &Path {
        &self.executable
    }

    async fn open_session(
        &self,
        client: &IpcClient,
        config: &ConnectionConfig,
    ) -> Result<ServerInfo, DriverError> {
        let timeout = config.settings.connect_timeout();
        let handshake = perform_handshake(client, timeout).await?;
        if handshake.metadata.id != self.metadata.id {
            return Err(DriverError::new(
                ErrorKind::Protocol,
                format!(
                    "connector identifies as '{}', expected '{}'",
                    handshake.metadata.id, self.metadata.id
                ),
            ));
        }
        let call = IpcCall::Connect(IpcConnectionConfig::from(config));
        with_timeout(timeout, client.call(call)).await
    }
}

#[async_trait]
impl DatabaseDriver for ExternalDriver {
    fn metadata(&self) -> DriverMetadata {
        self.metadata.clone()
    }

    fn capabilities(&self) -> DriverCapabilities {
        self.capabilities
    }

    async fn connect(
        &self,
        config: &ConnectionConfig,
    ) -> Result<Box<dyn DatabaseConnection>, DriverError> {
        let client = IpcClient::spawn(&self.executable)?;
        match self.open_session(&client, config).await {
            Ok(server_info) => Ok(Box::new(ExternalConnection::new(client, server_info))),
            Err(error) => {
                client.kill().await;
                Err(error)
            }
        }
    }
}
