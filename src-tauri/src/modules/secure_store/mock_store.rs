//! In-memory stand-in for the OS credential store. Used when `cfg!(test)` is true (unit tests in
//! this crate) or when `PENGINE_MOCK_KEYCHAIN=1` / `true` is set (integration tests and CI).

use super::SecureStoreError;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

type Key = (String, String);

static MOCK: LazyLock<Mutex<HashMap<Key, Vec<u8>>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn key(service: &str, account: &str) -> Key {
    (service.to_string(), account.to_string())
}

fn lock() -> Result<std::sync::MutexGuard<'static, HashMap<Key, Vec<u8>>>, SecureStoreError> {
    MOCK.lock()
        .map_err(|_| SecureStoreError::Backend("mock keychain mutex poisoned".into()))
}

pub(super) fn save(service: &str, account: &str, value: &[u8]) -> Result<(), SecureStoreError> {
    let mut g = lock()?;
    g.insert(key(service, account), value.to_vec());
    Ok(())
}

pub(super) fn load(service: &str, account: &str) -> Result<Vec<u8>, SecureStoreError> {
    let g = lock()?;
    g.get(&key(service, account))
        .cloned()
        .ok_or(SecureStoreError::NotFound)
}

pub(super) fn delete(service: &str, account: &str) -> Result<(), SecureStoreError> {
    let mut g = lock()?;
    g.remove(&key(service, account));
    Ok(())
}
