/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::SshError;
use crate::hop::ResolvedHop;
use crate::known_hosts::HostKeyStore;
use russh::keys::PublicKeyOrCertificate;
use std::sync::{Arc, Mutex, PoisonError};

pub(crate) type Rejection = Arc<Mutex<Option<SshError>>>;

pub(crate) struct ClientHandler {
    store: HostKeyStore,
    hop: ResolvedHop,
    rejection: Rejection,
}

impl ClientHandler {
    pub(crate) fn new(store: HostKeyStore, hop: ResolvedHop) -> (Self, Rejection) {
        let rejection = Rejection::default();
        let handler = Self {
            store,
            hop,
            rejection: rejection.clone(),
        };
        (handler, rejection)
    }

    fn reject(&self, error: SshError) {
        *self
            .rejection
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(error);
    }
}

pub(crate) fn take_rejection(rejection: &Rejection) -> Option<SshError> {
    rejection
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

impl russh::client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = server_public_key else {
            self.reject(SshError::Configuration(format!(
                "{} presented an SSH host certificate, which Relix does not verify",
                self.hop.endpoint()
            )));
            return Ok(false);
        };
        match self.store.verify(&self.hop, key) {
            Ok(()) => Ok(true),
            Err(error) => {
                self.reject(error);
                Ok(false)
            }
        }
    }
}
