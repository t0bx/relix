/*
 * @author t0bx
 * @since 23.10.2026
 */

use relix_core::TlsSettings;
use relix_driver_api::DriverError;
use tokio_postgres_rustls::MakeRustlsConnect;

pub(crate) async fn tls_connector(
    settings: &TlsSettings,
) -> Result<Option<MakeRustlsConnect>, DriverError> {
    Ok(relix_tls::client_config(settings)
        .await?
        .map(MakeRustlsConnect::new))
}
