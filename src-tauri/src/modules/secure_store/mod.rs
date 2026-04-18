//! OS-backed storage for per-bot secrets and MCP passthrough env vars.
//!
//! - macOS: Login keychain generic passwords (**no** per-operation Touch ID / passcode).
//!   Encrypted at rest; readable while the user is logged in. Combined with the in-memory
//!   cache after [`warm_app_secrets`], normal use does not hit the keychain on every request.
//! - Linux / Windows: `keyring` crate (Secret Service / Credential Manager).
//!
//! **Single keychain item:** Bot tokens and MCP passthrough values live in **one**
//! JSON blob (`AppSecretsV1`). Call [`warm_app_secrets`] once at startup so the rest
//! of the session reads secrets from RAM unless you explicitly save.
//!
//! Legacy per-bot / per-MCP items are merged into the unified blob on first cold load when
//! needed. We **do not** auto-delete legacy entries: deleting old ACL-protected items can
//! trigger the same password prompt as reading them; stale legacy rows are harmless once the
//! unified item is populated.
//!
//! **Tests:** Unit tests use an in-memory mock (`cfg!(test)`). Integration tests use
//! `tests/common.rs` to set `PENGINE_MOCK_KEYCHAIN=1` so the library never touches the OS store.
//!
//! **First launch after upgrading** may still trigger more than one keychain prompt while the
//! unified blob is filled from legacy items; that is expected.

mod mock_store;

#[cfg(target_os = "macos")]
mod macos;

#[cfg(any(target_os = "linux", target_os = "windows"))]
mod keyring_impl;

fn use_mock_store() -> bool {
    cfg!(test) || mock_env_enabled()
}

fn mock_env_enabled() -> bool {
    matches!(
        std::env::var("PENGINE_MOCK_KEYCHAIN"),
        Ok(s) if s == "1" || s.eq_ignore_ascii_case("true")
    )
}

#[cfg(target_os = "macos")]
fn os_save(service: &str, account: &str, value: &[u8]) -> Result<(), SecureStoreError> {
    macos::save(service, account, value)
}
#[cfg(target_os = "macos")]
fn os_load(service: &str, account: &str) -> Result<Vec<u8>, SecureStoreError> {
    macos::load(service, account)
}
#[cfg(target_os = "macos")]
fn os_delete(service: &str, account: &str) -> Result<(), SecureStoreError> {
    macos::delete(service, account)
}

#[cfg(any(target_os = "linux", target_os = "windows"))]
fn os_save(service: &str, account: &str, value: &[u8]) -> Result<(), SecureStoreError> {
    keyring_impl::save(service, account, value)
}
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn os_load(service: &str, account: &str) -> Result<Vec<u8>, SecureStoreError> {
    keyring_impl::load(service, account)
}
#[cfg(any(target_os = "linux", target_os = "windows"))]
fn os_delete(service: &str, account: &str) -> Result<(), SecureStoreError> {
    keyring_impl::delete(service, account)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn os_save(_s: &str, _a: &str, _v: &[u8]) -> Result<(), SecureStoreError> {
    Err(SecureStoreError::Unsupported)
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn os_load(_s: &str, _a: &str) -> Result<Vec<u8>, SecureStoreError> {
    Err(SecureStoreError::NotFound)
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn os_delete(_s: &str, _a: &str) -> Result<(), SecureStoreError> {
    Err(SecureStoreError::Unsupported)
}

fn kv_save(service: &str, account: &str, value: &[u8]) -> Result<(), SecureStoreError> {
    if use_mock_store() {
        mock_store::save(service, account, value)
    } else {
        os_save(service, account, value)
    }
}

fn kv_load(service: &str, account: &str) -> Result<Vec<u8>, SecureStoreError> {
    if use_mock_store() {
        mock_store::load(service, account)
    } else {
        os_load(service, account)
    }
}

fn kv_delete(service: &str, account: &str) -> Result<(), SecureStoreError> {
    if use_mock_store() {
        mock_store::delete(service, account)
    } else {
        os_delete(service, account)
    }
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// Legacy: one keychain item per bot id.
const BOT_TOKEN_SERVICE: &str = "com.maximedogawa.pengine.bot_token";
/// Legacy: MCP passthrough service (per-key + old JSON blob).
const MCP_PASSTHROUGH_SERVICE: &str = "com.maximedogawa.pengine.mcp_passthrough";
const MCP_PASSTHROUGH_BLOB_ACCOUNT: &str = "__pengine_mcp_passthrough_blob_v1__";

/// Unified store: one keychain item for the whole app secret set.
const UNIFIED_SERVICE: &str = "com.maximedogawa.pengine.app_secrets";
const UNIFIED_ACCOUNT: &str = "__pengine_app_secrets_v1__";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AppSecretsV1 {
    #[serde(default)]
    bots: HashMap<String, String>,
    #[serde(default)]
    mcp: HashMap<String, String>,
}

static APP_SECRETS: Mutex<Option<AppSecretsV1>> = Mutex::new(None);

fn composite_key(tool_id: &str, env_key: &str) -> String {
    format!("{tool_id}::{env_key}")
}

fn parse_unified(bytes: &[u8]) -> Result<AppSecretsV1, SecureStoreError> {
    if bytes.is_empty() {
        return Ok(AppSecretsV1::default());
    }
    let s = String::from_utf8(bytes.to_vec())
        .map_err(|e| SecureStoreError::Backend(format!("secrets blob was not valid UTF-8: {e}")))?;
    let t = s.trim();
    if t.is_empty() {
        return Ok(AppSecretsV1::default());
    }
    serde_json::from_str(t)
        .map_err(|e| SecureStoreError::Backend(format!("secrets blob invalid JSON: {e}")))
}

fn load_unified_from_keychain() -> Result<AppSecretsV1, SecureStoreError> {
    match kv_load(UNIFIED_SERVICE, UNIFIED_ACCOUNT) {
        Ok(bytes) => parse_unified(&bytes),
        Err(SecureStoreError::NotFound) => Ok(AppSecretsV1::default()),
        Err(e) => Err(e),
    }
}

fn save_unified_to_keychain(s: &AppSecretsV1) -> Result<(), SecureStoreError> {
    if s.bots.is_empty() && s.mcp.is_empty() {
        return kv_delete(UNIFIED_SERVICE, UNIFIED_ACCOUNT);
    }
    let json = serde_json::to_string(s)
        .map_err(|e| SecureStoreError::Backend(format!("encode secrets blob: {e}")))?;
    kv_save(UNIFIED_SERVICE, UNIFIED_ACCOUNT, json.as_bytes())
}

/// Read the pre-unification MCP JSON blob (map of `tool::KEY` → value), if present.
fn read_legacy_mcp_blob_map() -> Result<HashMap<String, String>, SecureStoreError> {
    match kv_load(MCP_PASSTHROUGH_SERVICE, MCP_PASSTHROUGH_BLOB_ACCOUNT) {
        Ok(bytes) => {
            if bytes.is_empty() {
                return Ok(HashMap::new());
            }
            let s = String::from_utf8(bytes).map_err(|e| {
                SecureStoreError::Backend(format!("legacy mcp blob was not valid UTF-8: {e}"))
            })?;
            let t = s.trim();
            if t.is_empty() {
                return Ok(HashMap::new());
            }
            serde_json::from_str(t).map_err(|e| {
                SecureStoreError::Backend(format!("legacy mcp blob invalid JSON: {e}"))
            })
        }
        Err(SecureStoreError::NotFound) => Ok(HashMap::new()),
        Err(e) => Err(e),
    }
}

#[derive(Clone, Copy)]
enum LegacyScan {
    /// First session load: may read the old MCP JSON blob once, then gap fills.
    Full,
    /// In-memory cache already exists: **never** re-scan the legacy MCP blob (avoids a keychain
    /// round-trip on every MCP registry rebuild).
    GapsOnly,
}

fn needs_legacy_gap_fetch(
    s: &AppSecretsV1,
    bot_ids: &[String],
    mcp_pairs: &[(String, String)],
) -> bool {
    bot_ids.iter().any(|b| !s.bots.contains_key(b))
        || mcp_pairs
            .iter()
            .any(|(t, k)| !s.mcp.contains_key(&composite_key(t, k)))
}

fn lock_secrets() -> Result<std::sync::MutexGuard<'static, Option<AppSecretsV1>>, SecureStoreError>
{
    APP_SECRETS
        .lock()
        .map_err(|_| SecureStoreError::Backend("app secrets mutex poisoned".into()))
}

/// Load secrets from the keychain **once**, migrate legacy items, cache in memory.
/// Call from app startup with every known `bot_id` and MCP passthrough `(tool_id, env_key)`
/// **before** spawning work that calls [`load_token`] or MCP connect.
pub fn warm_app_secrets(
    bot_ids: &[String],
    mcp_pairs: &[(String, String)],
) -> Result<(), SecureStoreError> {
    let mut guard = lock_secrets()?;
    match guard.as_mut() {
        Some(s) => extend_from_legacy_if_missing(s, bot_ids, mcp_pairs),
        None => {
            let mut s = load_unified_from_keychain()?;
            let dirty = if needs_legacy_gap_fetch(&s, bot_ids, mcp_pairs) {
                migrate_all_legacy_into(&mut s, bot_ids, mcp_pairs, LegacyScan::Full)?
            } else {
                false
            };
            if dirty {
                save_unified_to_keychain(&s)?;
            }
            *guard = Some(s);
            Ok(())
        }
    }
}

/// Extend an already-warmed cache: pick up new MCP keys or bots from legacy keychain only.
fn extend_from_legacy_if_missing(
    s: &mut AppSecretsV1,
    bot_ids: &[String],
    mcp_pairs: &[(String, String)],
) -> Result<(), SecureStoreError> {
    if !needs_legacy_gap_fetch(s, bot_ids, mcp_pairs) {
        return Ok(());
    }
    let dirty = migrate_all_legacy_into(s, bot_ids, mcp_pairs, LegacyScan::GapsOnly)?;
    if dirty {
        save_unified_to_keychain(s)?;
    }
    Ok(())
}

/// Merge legacy keychain data into `s`. Returns `true` if `s` changed.
fn migrate_all_legacy_into(
    s: &mut AppSecretsV1,
    bot_ids: &[String],
    mcp_pairs: &[(String, String)],
    scan: LegacyScan,
) -> Result<bool, SecureStoreError> {
    let mut dirty = false;

    if matches!(scan, LegacyScan::Full) {
        // Whole legacy MCP JSON blob (from the first iteration of blob storage).
        if let Ok(old_mcp) = read_legacy_mcp_blob_map() {
            for (k, v) in old_mcp {
                if !v.is_empty() && s.mcp.insert(k, v).is_none() {
                    dirty = true;
                }
            }
        }
    }

    for bid in bot_ids {
        if s.bots.contains_key(bid) {
            continue;
        }
        match kv_load(BOT_TOKEN_SERVICE, bid) {
            Ok(bytes) => {
                let tok = String::from_utf8(bytes).map_err(|e| {
                    SecureStoreError::Backend(format!("legacy bot token was not valid UTF-8: {e}"))
                })?;
                if !tok.is_empty() {
                    s.bots.insert(bid.clone(), tok);
                    dirty = true;
                }
            }
            Err(SecureStoreError::NotFound) => {}
            Err(e) => return Err(e),
        }
    }

    for (tool_id, env_key) in mcp_pairs {
        let ck = composite_key(tool_id, env_key);
        if s.mcp.contains_key(&ck) {
            continue;
        }
        match kv_load(MCP_PASSTHROUGH_SERVICE, &ck) {
            Ok(bytes) => {
                let val = String::from_utf8(bytes).map_err(|e| {
                    SecureStoreError::Backend(format!("legacy mcp secret was not valid UTF-8: {e}"))
                })?;
                if !val.is_empty() {
                    s.mcp.insert(ck, val);
                    dirty = true;
                }
            }
            Err(SecureStoreError::NotFound) => {}
            Err(e) => return Err(e),
        }
    }

    Ok(dirty)
}

/// Same as [`warm_app_secrets`] with no bot ids — used from MCP registry rebuild.
pub fn preload_mcp_passthrough_secrets(
    candidates: &[(String, String)],
) -> Result<(), SecureStoreError> {
    warm_app_secrets(&[], candidates)
}

pub fn save_token(bot_id: &str, token: &str) -> Result<(), SecureStoreError> {
    let snapshot = {
        let guard = lock_secrets()?;
        let base = match guard.as_ref() {
            Some(x) => x.clone(),
            None => load_unified_from_keychain()?,
        };
        let mut next = base;
        next.bots.insert(bot_id.to_string(), token.to_string());
        next
    };
    save_unified_to_keychain(&snapshot)?;
    let mut guard = lock_secrets()?;
    *guard = Some(snapshot);
    Ok(())
}

pub fn load_token(bot_id: &str) -> Result<String, SecureStoreError> {
    {
        let guard = lock_secrets()?;
        if let Some(s) = guard.as_ref() {
            if let Some(t) = s.bots.get(bot_id) {
                return Ok(t.clone());
            }
        }
    }
    warm_app_secrets(&[bot_id.to_string()], &[])?;
    let guard = lock_secrets()?;
    let s = guard
        .as_ref()
        .ok_or_else(|| SecureStoreError::Backend("secrets cache not initialized".into()))?;
    s.bots
        .get(bot_id)
        .cloned()
        .ok_or(SecureStoreError::NotFound)
}

pub fn delete_token(bot_id: &str) -> Result<(), SecureStoreError> {
    let snapshot = {
        let guard = lock_secrets()?;
        let base = match guard.as_ref() {
            Some(x) => x.clone(),
            None => load_unified_from_keychain()?,
        };
        let mut next = base;
        next.bots.remove(bot_id);
        next
    };
    save_unified_to_keychain(&snapshot)?;
    let mut guard = lock_secrets()?;
    *guard = Some(snapshot);
    Ok(())
}

pub fn save_mcp_secret(tool_id: &str, env_key: &str, value: &str) -> Result<(), SecureStoreError> {
    let ck = composite_key(tool_id, env_key);
    let snapshot = {
        let guard = lock_secrets()?;
        let base = match guard.as_ref() {
            Some(x) => x.clone(),
            None => load_unified_from_keychain()?,
        };
        let mut next = base;
        next.mcp.insert(ck, value.to_string());
        next
    };
    save_unified_to_keychain(&snapshot)?;
    let mut guard = lock_secrets()?;
    *guard = Some(snapshot);
    Ok(())
}

pub fn load_mcp_secret(tool_id: &str, env_key: &str) -> Result<String, SecureStoreError> {
    let ck = composite_key(tool_id, env_key);
    {
        let guard = lock_secrets()?;
        if let Some(s) = guard.as_ref() {
            if let Some(v) = s.mcp.get(&ck) {
                return Ok(v.clone());
            }
        }
    }
    warm_app_secrets(&[], &[(tool_id.to_string(), env_key.to_string())])?;
    let guard = lock_secrets()?;
    let s = guard
        .as_ref()
        .ok_or_else(|| SecureStoreError::Backend("secrets cache not initialized".into()))?;
    s.mcp.get(&ck).cloned().ok_or(SecureStoreError::NotFound)
}

pub fn delete_mcp_secret(tool_id: &str, env_key: &str) -> Result<(), SecureStoreError> {
    let ck = composite_key(tool_id, env_key);
    let snapshot = {
        let guard = lock_secrets()?;
        let base = match guard.as_ref() {
            Some(x) => x.clone(),
            None => load_unified_from_keychain()?,
        };
        let mut next = base;
        next.mcp.remove(&ck);
        next
    };
    save_unified_to_keychain(&snapshot)?;
    let mut guard = lock_secrets()?;
    *guard = Some(snapshot);
    Ok(())
}

#[derive(Debug)]
pub enum SecureStoreError {
    NotFound,
    UserCancelled,
    /// Keychain rejected access (e.g. locked, auth failed) without an explicit user cancel.
    AuthFailed,
    Unsupported,
    Backend(String),
}

impl std::fmt::Display for SecureStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "secret not found"),
            Self::UserCancelled => write!(f, "user cancelled authentication"),
            Self::AuthFailed => write!(f, "keychain authentication failed (locked or denied)"),
            Self::Unsupported => write!(f, "secure store not supported on this platform yet"),
            Self::Backend(msg) => write!(f, "secure store error: {msg}"),
        }
    }
}

impl std::error::Error for SecureStoreError {}
