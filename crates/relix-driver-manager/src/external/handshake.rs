/*
 * @author t0bx
 * @since 23.10.2026
 */

use super::client::IpcClient;
use relix_driver_api::ipc::{HandshakeResult, IPC_PROTOCOL_VERSION, IpcCall};
use relix_driver_api::{DRIVER_API_VERSION, DriverError, ErrorKind, with_timeout};
use std::time::Duration;

pub(crate) async fn perform_handshake(
    client: &IpcClient,
    timeout: Duration,
) -> Result<HandshakeResult, DriverError> {
    let result: HandshakeResult = with_timeout(
        timeout,
        client.call(IpcCall::Handshake {
            protocol_version: IPC_PROTOCOL_VERSION,
        }),
    )
    .await?;
    check_compatibility(&result)?;
    Ok(result)
}

fn check_compatibility(result: &HandshakeResult) -> Result<(), DriverError> {
    if result.protocol_version != IPC_PROTOCOL_VERSION {
        return Err(incompatible(format!(
            "connector speaks protocol version {}, RELIX expects {IPC_PROTOCOL_VERSION}",
            result.protocol_version
        )));
    }
    if result.api_version != DRIVER_API_VERSION {
        return Err(incompatible(format!(
            "connector targets driver API version {}, RELIX provides {DRIVER_API_VERSION}",
            result.api_version
        )));
    }
    if !result.capabilities.is_relational() {
        return Err(incompatible(
            "only relational connectors are supported by protocol version 1".to_owned(),
        ));
    }
    Ok(())
}

fn incompatible(message: String) -> DriverError {
    DriverError::new(ErrorKind::Protocol, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_driver_api::{ConnectionTarget, DriverCapabilities, DriverMetadata};

    fn handshake(protocol_version: u32, capabilities: DriverCapabilities) -> HandshakeResult {
        HandshakeResult {
            protocol_version,
            api_version: DRIVER_API_VERSION,
            metadata: DriverMetadata::built_in("x", "X", None, ConnectionTarget::File),
            capabilities,
        }
    }

    #[test]
    fn rejects_other_protocols_and_key_value_connectors() {
        assert!(check_compatibility(&handshake(1, DriverCapabilities::relational())).is_ok());
        assert!(check_compatibility(&handshake(2, DriverCapabilities::relational())).is_err());
        assert!(check_compatibility(&handshake(1, DriverCapabilities::key_value())).is_err());
    }
}
