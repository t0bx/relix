/*
 * @author t0bx
 * @since 23.10.2026
 */

use keyring_core::{CredentialStore, Entry, Error};
use std::sync::{Arc, LazyLock};

static STORE: LazyLock<Result<(), String>> = LazyLock::new(install_platform_store);

/// Opens an entry in the operating system's credential store. The store is installed
/// once: Keychain on macOS, Credential Manager on Windows and the Secret Service (via
/// libdbus) on other Unix systems.
pub fn entry(service: &str, user: &str) -> Result<Entry, Error> {
    if let Err(reason) = STORE.as_ref() {
        return Err(Error::NoStorageAccess(reason.clone().into()));
    }
    Entry::new(service, user)
}

fn install_platform_store() -> Result<(), String> {
    let store = platform_store().map_err(|error| error.to_string())?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_store() -> Result<Arc<CredentialStore>, Error> {
    Ok(apple_native_keyring_store::keychain::Store::new()?)
}

#[cfg(windows)]
fn platform_store() -> Result<Arc<CredentialStore>, Error> {
    Ok(windows_native_keyring_store::Store::new()?)
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_store() -> Result<Arc<CredentialStore>, Error> {
    Ok(dbus_secret_service_keyring_store::Store::new()?)
}

#[cfg(not(any(unix, windows)))]
fn platform_store() -> Result<Arc<CredentialStore>, Error> {
    Err(Error::NotSupportedByStore(
        "no credential store on this platform".into(),
    ))
}
