use security_framework::passwords::{
    delete_generic_password, generic_password, set_generic_password, PasswordOptions,
};

use super::SecureStoreError;

/// Numeric OSStatus values that security-framework-sys doesn't re-export.
const ERR_SEC_USER_CANCELED: i32 = -128;
const ERR_SEC_AUTH_FAILED: i32 = -25293;
const ERR_SEC_ITEM_NOT_FOUND: i32 = -25300;
/// `errSecDuplicateItem` — set failed because a matching item already exists.
const ERR_SEC_DUPLICATE_ITEM: i32 = -25299;

fn map_error(err: security_framework::base::Error) -> SecureStoreError {
    match err.code() {
        ERR_SEC_ITEM_NOT_FOUND => SecureStoreError::NotFound,
        ERR_SEC_USER_CANCELED => SecureStoreError::UserCancelled,
        ERR_SEC_AUTH_FAILED => SecureStoreError::AuthFailed,
        other => SecureStoreError::Backend(format!("OSStatus {other}: {err}")),
    }
}

/// Store generic passwords **without** `SecAccessControl` user-presence (Touch ID /
/// passcode on **every** read/write/delete). That model caused many prompts per session.
///
/// Secrets still live in the user login keychain (encrypted at rest, not synced to iCloud
/// unless the system does so for generic passwords — we use app-specific service strings).
/// Access is gated by the macOS user session, like most desktop apps.
///
/// Tries `set_generic_password` first so an interrupted write does not leave the item deleted;
/// on duplicate-item, deletes once and retries (replace semantics).
pub(super) fn save(service: &str, account: &str, value: &[u8]) -> Result<(), SecureStoreError> {
    match set_generic_password(service, account, value) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERR_SEC_DUPLICATE_ITEM => {
            match delete_generic_password(service, account) {
                Ok(()) => {}
                Err(e2) if e2.code() == ERR_SEC_ITEM_NOT_FOUND => {}
                Err(e2) => return Err(map_error(e2)),
            }
            set_generic_password(service, account, value).map_err(map_error)
        }
        Err(e) => Err(map_error(e)),
    }
}

pub(super) fn load(service: &str, account: &str) -> Result<Vec<u8>, SecureStoreError> {
    let opts = PasswordOptions::new_generic_password(service, account);
    generic_password(opts).map_err(map_error)
}

pub(super) fn delete(service: &str, account: &str) -> Result<(), SecureStoreError> {
    match delete_generic_password(service, account) {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERR_SEC_ITEM_NOT_FOUND => Ok(()),
        Err(e) => Err(map_error(e)),
    }
}
