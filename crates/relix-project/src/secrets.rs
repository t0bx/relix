/*
 * @author t0bx
 * @since 23.10.2026
 */

use crate::error::ProjectError;
use relix_core::{DataSourceId, ProjectId, Secret};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

const KEYRING_SERVICE: &str = "relix";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretKey(String);

impl SecretKey {
    pub fn for_data_source(project: ProjectId, data_source: DataSourceId) -> Self {
        Self(format!("project/{project}/datasource/{data_source}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &SecretKey) -> Result<Option<Secret>, ProjectError>;

    fn set(&self, key: &SecretKey, secret: &Secret) -> Result<(), ProjectError>;

    fn delete(&self, key: &SecretKey) -> Result<(), ProjectError>;

    fn is_persistent(&self) -> bool;
}

#[derive(Debug, Default)]
pub struct MemorySecretStore {
    secrets: Mutex<HashMap<SecretKey, Secret>>,
}

impl MemorySecretStore {
    fn with_secrets<T>(&self, action: impl FnOnce(&mut HashMap<SecretKey, Secret>) -> T) -> T {
        let mut guard = self
            .secrets
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        action(&mut guard)
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, key: &SecretKey) -> Result<Option<Secret>, ProjectError> {
        Ok(self.with_secrets(|secrets| secrets.get(key).cloned()))
    }

    fn set(&self, key: &SecretKey, secret: &Secret) -> Result<(), ProjectError> {
        self.with_secrets(|secrets| secrets.insert(key.clone(), secret.clone()));
        Ok(())
    }

    fn delete(&self, key: &SecretKey) -> Result<(), ProjectError> {
        self.with_secrets(|secrets| secrets.remove(key));
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        false
    }
}

#[derive(Debug, Default)]
pub struct KeyringSecretStore;

impl KeyringSecretStore {
    fn entry(key: &SecretKey) -> Result<keyring::Entry, keyring::Error> {
        keyring::Entry::new(KEYRING_SERVICE, key.as_str())
    }
}

impl SecretStore for KeyringSecretStore {
    fn get(&self, key: &SecretKey) -> Result<Option<Secret>, ProjectError> {
        match Self::entry(key).and_then(|entry| entry.get_password()) {
            Ok(password) => Ok(Some(Secret::new(password))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(ProjectError::Secret(error.to_string())),
        }
    }

    fn set(&self, key: &SecretKey, secret: &Secret) -> Result<(), ProjectError> {
        Self::entry(key)
            .and_then(|entry| entry.set_password(secret.expose()))
            .map_err(|error| ProjectError::Secret(error.to_string()))
    }

    fn delete(&self, key: &SecretKey) -> Result<(), ProjectError> {
        match Self::entry(key).and_then(|entry| entry.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(ProjectError::Secret(error.to_string())),
        }
    }

    fn is_persistent(&self) -> bool {
        true
    }
}

/// Uses the operating system keyring and silently degrades to an in-memory store when
/// the platform keyring is unavailable (e.g. no Secret Service on a headless Linux box).
/// Callers can surface the degradation through [`FallbackSecretStore::is_degraded`].
pub struct FallbackSecretStore {
    primary: Box<dyn SecretStore>,
    memory: MemorySecretStore,
    degraded: AtomicBool,
}

impl Default for FallbackSecretStore {
    fn default() -> Self {
        Self::new(Box::new(KeyringSecretStore))
    }
}

impl FallbackSecretStore {
    pub fn new(primary: Box<dyn SecretStore>) -> Self {
        Self {
            primary,
            memory: MemorySecretStore::default(),
            degraded: AtomicBool::new(false),
        }
    }

    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::Relaxed)
    }

    fn degrade(&self) {
        self.degraded.store(true, Ordering::Relaxed);
    }
}

impl SecretStore for FallbackSecretStore {
    fn get(&self, key: &SecretKey) -> Result<Option<Secret>, ProjectError> {
        if let Some(secret) = self.memory.get(key)? {
            return Ok(Some(secret));
        }
        if self.is_degraded() {
            return Ok(None);
        }
        self.primary.get(key).or_else(|_| {
            self.degrade();
            Ok(None)
        })
    }

    fn set(&self, key: &SecretKey, secret: &Secret) -> Result<(), ProjectError> {
        if !self.is_degraded() && self.primary.set(key, secret).is_ok() {
            return self.memory.delete(key);
        }
        self.degrade();
        self.memory.set(key, secret)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), ProjectError> {
        self.memory.delete(key)?;
        if !self.is_degraded() {
            let _ = self.primary.delete(key);
        }
        Ok(())
    }

    fn is_persistent(&self) -> bool {
        !self.is_degraded() && self.primary.is_persistent()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BrokenStore;

    impl SecretStore for BrokenStore {
        fn get(&self, _key: &SecretKey) -> Result<Option<Secret>, ProjectError> {
            Err(ProjectError::Secret("no keyring".into()))
        }

        fn set(&self, _key: &SecretKey, _secret: &Secret) -> Result<(), ProjectError> {
            Err(ProjectError::Secret("no keyring".into()))
        }

        fn delete(&self, _key: &SecretKey) -> Result<(), ProjectError> {
            Err(ProjectError::Secret("no keyring".into()))
        }

        fn is_persistent(&self) -> bool {
            true
        }
    }

    fn key() -> SecretKey {
        SecretKey::for_data_source(ProjectId::new(), DataSourceId::new())
    }

    #[test]
    fn memory_store_roundtrip() {
        let store = MemorySecretStore::default();
        let key = key();
        store.set(&key, &Secret::new("pw")).unwrap();
        assert_eq!(store.get(&key).unwrap(), Some(Secret::new("pw")));
        store.delete(&key).unwrap();
        assert_eq!(store.get(&key).unwrap(), None);
    }

    #[test]
    fn fallback_degrades_to_memory_when_keyring_fails() {
        let store = FallbackSecretStore::new(Box::new(BrokenStore));
        let key = key();
        assert!(store.is_persistent());
        store.set(&key, &Secret::new("pw")).unwrap();
        assert!(store.is_degraded());
        assert!(!store.is_persistent());
        assert_eq!(store.get(&key).unwrap(), Some(Secret::new("pw")));
        store.delete(&key).unwrap();
        assert_eq!(store.get(&key).unwrap(), None);
    }

    #[test]
    fn fallback_prefers_primary_when_available() {
        let store = FallbackSecretStore::new(Box::new(MemorySecretStore::default()));
        let key = key();
        store.set(&key, &Secret::new("pw")).unwrap();
        assert!(!store.is_degraded());
        assert_eq!(store.get(&key).unwrap(), Some(Secret::new("pw")));
    }
}
