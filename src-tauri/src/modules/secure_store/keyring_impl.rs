//! Linux + Windows backend for secret storage via the `keyring` crate.
//!
//! - Linux: Secret Service (gnome-keyring / KWallet). The user's desktop keyring
//!   unlocks with their login password; most distros auto-unlock on session login,
//!   otherwise the Secret Service prompts the first time per session.
//! - Windows: Credential Manager, scoped to the logged-in Windows user.

use super::SecureStoreError;
use keyring::{Entry, Error as KeyringError};

fn open(service: &str, account: &str) -> Result<Entry, SecureStoreError> {
    Entry::new(service, account).map_err(map_error)
}

fn map_error(e: KeyringError) -> SecureStoreError {
    match e {
        KeyringError::NoEntry => SecureStoreError::NotFound,
        KeyringError::NoStorageAccess(inner) => SecureStoreError::Backend(format!(
            "no access to OS credential store: {inner} (is the desktop keyring \
             running and unlocked?)"
        )),
        other => SecureStoreError::Backend(other.to_string()),
    }
}

pub(super) fn save(service: &str, account: &str, value: &[u8]) -> Result<(), SecureStoreError> {
    let entry = open(service, account)?;
    // The keyring crate's cross-platform API takes UTF-8 strings; callers are
    // expected to store UTF-8 (tokens, API keys). If we ever need raw bytes we
    // can switch to `set_secret`, but today all callers pass text.
    let text = std::str::from_utf8(value)
        .map_err(|e| SecureStoreError::Backend(format!("secret value was not valid UTF-8: {e}")))?;
    entry.set_password(text).map_err(map_error)
}

pub(super) fn load(service: &str, account: &str) -> Result<Vec<u8>, SecureStoreError> {
    let entry = open(service, account)?;
    entry
        .get_password()
        .map(|s| s.into_bytes())
        .map_err(map_error)
}

pub(super) fn delete(service: &str, account: &str) -> Result<(), SecureStoreError> {
    let entry = open(service, account)?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(KeyringError::NoEntry) => Ok(()),
        Err(e) => Err(map_error(e)),
    }
}
