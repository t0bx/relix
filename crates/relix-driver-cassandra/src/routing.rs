/*
 * @author t0bx
 * @since 23.10.2026
 */

use async_trait::async_trait;
use scylla::errors::TranslationError;
use scylla::policies::address_translator::{AddressTranslator, UntranslatedPeer};
use std::net::SocketAddr;

/// Routes every cluster node to the configured endpoint. Peer addresses advertised by
/// the cluster (container IPs, private networks) are usually unreachable from a
/// workstation, an SSH tunnel forwards a single port and the TLS relay serves one
/// server, so all requests go through the node the user configured, which coordinates
/// them for the rest of the cluster.
#[derive(Debug)]
pub(crate) struct SingleEndpoint(pub(crate) SocketAddr);

#[async_trait]
impl AddressTranslator for SingleEndpoint {
    async fn translate_address(
        &self,
        _untranslated_peer: &UntranslatedPeer,
    ) -> Result<SocketAddr, TranslationError> {
        Ok(self.0)
    }
}
