/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_driver_api::{DriverError, ErrorKind};
use rustls_pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use tokio_rustls::client::TlsStream;

#[derive(Clone)]
pub(crate) struct TlsTarget {
    host: String,
    port: u16,
    server_name: ServerName<'static>,
    connector: TlsConnector,
}

impl TlsTarget {
    pub(crate) fn new(
        host: &str,
        port: u16,
        server_name: &str,
        config: tokio_rustls::rustls::ClientConfig,
    ) -> Result<Self, DriverError> {
        let server_name = ServerName::try_from(server_name.to_owned()).map_err(|_| {
            DriverError::new(
                ErrorKind::Tls,
                format!("'{server_name}' is not a valid TLS server name"),
            )
        })?;
        Ok(Self {
            host: host.to_owned(),
            port,
            server_name,
            connector: TlsConnector::from(Arc::new(config)),
        })
    }

    pub(crate) async fn connect(&self) -> Result<TlsStream<TcpStream>, DriverError> {
        let stream = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .map_err(|error| {
                DriverError::connection(format!(
                    "cannot connect to {}:{}: {error}",
                    self.host, self.port
                ))
            })?;
        let _ = stream.set_nodelay(true);
        self.connector
            .connect(self.server_name.clone(), stream)
            .await
            .map_err(|error| {
                DriverError::new(ErrorKind::Tls, "TLS handshake failed").with_detail(format!(
                    "{error}; check that the server has TLS enabled on port {}",
                    self.port
                ))
            })
    }
}
