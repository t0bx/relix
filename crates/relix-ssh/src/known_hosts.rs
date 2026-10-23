/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::{HostKeyPrompt, SshError};
use crate::hop::ResolvedHop;
use crate::openssh_config::home_dir;
use relix_core::HostKeyPolicy;
use russh::keys::known_hosts::{check_known_hosts_path, learn_known_hosts_path};
use russh::keys::{HashAlg, PublicKey};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyStore {
    relix_file: PathBuf,
    user_file: Option<PathBuf>,
}

impl HostKeyStore {
    pub fn new(relix_file: impl Into<PathBuf>) -> Self {
        Self {
            relix_file: relix_file.into(),
            user_file: home_dir().map(|home| home.join(".ssh").join("known_hosts")),
        }
    }

    pub fn isolated(relix_file: impl Into<PathBuf>) -> Self {
        Self {
            relix_file: relix_file.into(),
            user_file: None,
        }
    }

    /// Verifies a server key against the configured known hosts files. A key that is
    /// recorded with a different value is always a hard failure; an unknown key yields
    /// a prompt the user has to accept explicitly (or, with the strict policy, an error
    /// that tells them to add the key themselves). Verification is never skipped.
    pub fn verify(&self, hop: &ResolvedHop, key: &PublicKey) -> Result<(), SshError> {
        let files = self.read_files(hop);
        for file in &files {
            match check_known_hosts_path(&hop.host, hop.port, key, file) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(russh::keys::Error::KeyChanged { line }) => {
                    return Err(SshError::HostKeyMismatch {
                        endpoint: hop.endpoint(),
                        fingerprint: fingerprint(key),
                        known_hosts: file.clone(),
                        line,
                    });
                }
                Err(error) => {
                    tracing::warn!(file = %file.display(), %error, "unreadable known_hosts file");
                }
            }
        }
        Err(SshError::HostKeyUnknown(Box::new(HostKeyPrompt {
            server: hop.name.clone(),
            host: hop.host.clone(),
            port: hop.port,
            algorithm: key.algorithm().to_string(),
            fingerprint: fingerprint(key),
            public_key: key.to_openssh().unwrap_or_default(),
            known_hosts: self.write_file(hop),
            trust_allowed: hop.host_key_policy == HostKeyPolicy::AskOnFirstUse,
        })))
    }

    pub fn trust(&self, prompt: &HostKeyPrompt) -> Result<(), SshError> {
        if !prompt.trust_allowed {
            return Err(SshError::Configuration(format!(
                "'{}' only accepts keys already present in known_hosts",
                prompt.server
            )));
        }
        let key = PublicKey::from_openssh(&prompt.public_key)
            .map_err(|error| SshError::Configuration(format!("invalid host key: {error}")))?;
        learn_known_hosts_path(&prompt.host, prompt.port, &key, &prompt.known_hosts).map_err(
            |error| {
                SshError::Configuration(format!(
                    "cannot write {}: {error}",
                    prompt.known_hosts.display()
                ))
            },
        )?;
        restrict(&prompt.known_hosts);
        Ok(())
    }

    fn read_files(&self, hop: &ResolvedHop) -> Vec<PathBuf> {
        match &hop.known_hosts_file {
            Some(custom) => vec![custom.clone()],
            None => self
                .user_file
                .iter()
                .cloned()
                .chain(std::iter::once(self.relix_file.clone()))
                .collect(),
        }
    }

    fn write_file(&self, hop: &ResolvedHop) -> PathBuf {
        hop.known_hosts_file
            .clone()
            .unwrap_or_else(|| self.relix_file.clone())
    }
}

pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

#[cfg(unix)]
fn restrict(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict(_path: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;
    use relix_core::SshServerConfig;
    use russh::keys::{Algorithm, PrivateKey};

    fn hop(policy: HostKeyPolicy) -> ResolvedHop {
        let mut config = SshServerConfig::new("Bastion", "bastion.example.com");
        config.username = "deploy".into();
        config.auth = relix_core::SshAuthMethod::Agent;
        config.port = 2222;
        config.host_key_policy = policy;
        ResolvedHop::from_config(&config).unwrap()
    }

    fn key() -> PublicKey {
        PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .unwrap()
            .public_key()
            .clone()
    }

    #[test]
    fn unknown_keys_prompt_and_trusted_keys_verify() {
        let dir = tempfile::tempdir().unwrap();
        let store = HostKeyStore::isolated(dir.path().join("known_hosts"));
        let hop = hop(HostKeyPolicy::AskOnFirstUse);
        let key = key();
        let SshError::HostKeyUnknown(prompt) = store.verify(&hop, &key).unwrap_err() else {
            panic!("expected an unknown host key");
        };
        assert!(prompt.fingerprint.starts_with("SHA256:"));
        store.trust(&prompt).unwrap();
        assert_eq!(store.verify(&hop, &key), Ok(()));
    }

    #[test]
    fn changed_keys_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = HostKeyStore::isolated(dir.path().join("known_hosts"));
        let hop = hop(HostKeyPolicy::AskOnFirstUse);
        let SshError::HostKeyUnknown(prompt) = store.verify(&hop, &key()).unwrap_err() else {
            panic!("expected an unknown host key");
        };
        store.trust(&prompt).unwrap();
        let error = store.verify(&hop, &key()).unwrap_err();
        assert!(matches!(error, SshError::HostKeyMismatch { .. }));
        assert!(error.to_string().contains("HOST KEY MISMATCH"));
    }

    #[test]
    fn strict_policy_never_learns_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = HostKeyStore::isolated(dir.path().join("known_hosts"));
        let SshError::HostKeyUnknown(prompt) = store
            .verify(&hop(HostKeyPolicy::Strict), &key())
            .unwrap_err()
        else {
            panic!("expected an unknown host key");
        };
        assert!(!prompt.trust_allowed);
        assert!(store.trust(&prompt).is_err());
        assert!(!dir.path().join("known_hosts").exists());
    }
}
