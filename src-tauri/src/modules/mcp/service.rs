//! Load `mcp.json` and build [`ToolRegistry`] — same code path for `native` and `stdio`
//! (Docker is just `command` + `args` on a `stdio` entry).

use super::client::McpClient;
use super::native;
use super::registry::{Provider, ToolRegistry};
use super::types::{McpConfig, ServerEntry};
use crate::modules::secure_store;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::Emitter;

const FILESYSTEM_SERVER_KEY: &str = "filesystem";
const REGISTRY_CHANGED_EVENT: &str = "pengine-registry-changed";

async fn emit_registry_changed_event(state: &crate::shared::state::AppState) {
    if let Some(handle) = state.app_handle.lock().await.as_ref() {
        let _ = handle.emit(REGISTRY_CHANGED_EVENT, ());
    }
}

fn app_data_mcp_path(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .map(|p| p.join("mcp.json"))
        .unwrap_or_else(|| PathBuf::from("mcp.json"))
}

/// If set, absolute or relative path to `mcp.json` (overrides all other resolution).
const MCP_CONFIG_ENV: &str = "PENGINE_MCP_CONFIG";

/// Resolve the active `mcp.json` path.
///
/// - Optional override: [`MCP_CONFIG_ENV`] → use that file.
/// - Otherwise: always `$APP_DATA/mcp.json` next to `connection.json`, so Tool Engine installs
///   and workspace folders persist regardless of cwd or where the binary lives (debug or release).
pub fn resolve_mcp_config_path(store_path: &Path) -> (PathBuf, &'static str) {
    if let Ok(raw) = std::env::var(MCP_CONFIG_ENV) {
        let t = raw.trim();
        if !t.is_empty() {
            return (PathBuf::from(t), "env");
        }
    }

    let app_path = app_data_mcp_path(store_path);
    (app_path, "app_data")
}

pub fn read_config(path: &Path) -> Result<McpConfig, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read mcp.json: {e}"))?;
    let mut value: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        format!(
            "parse mcp.json: {e} — every server entry needs a \"type\" field (\"native\" or \"stdio\")"
        )
    })?;

    // Must run before serde deserialises into `ServerEntry::Stdio` — the old field name
    // (`catalog_passthrough`) no longer exists on the struct, so a plain `from_value` would
    // silently drop any pre-migration secrets that are still sitting in `mcp.json`.
    let migrated_passthrough = migrate_legacy_catalog_passthrough(&mut value)?;

    let mut cfg: McpConfig = serde_json::from_value(value).map_err(|e| {
        format!(
            "parse mcp.json: {e} — every server entry needs a \"type\" field (\"native\" or \"stdio\")"
        )
    })?;
    let migrated_npx = migrate_legacy_npx_filesystem(&mut cfg);
    if migrated_passthrough || migrated_npx {
        save_config(path, &cfg)?;
    }
    Ok(cfg)
}

/// Derive a catalog tool id from its `mcp.json` server key (inverse of
/// `tool_engine::service::server_key`). Returns `None` for non-catalog keys (`te_custom_*`,
/// bare native entries, etc.) where there are no passthrough secrets to migrate or inject.
fn tool_id_from_catalog_server_key(server_key: &str) -> Option<String> {
    let rest = server_key.strip_prefix("te_")?;
    if rest.starts_with("custom_") {
        return None;
    }
    Some(rest.replacen('-', "/", 1))
}

/// Every `(catalog tool id, passthrough env key)` configured in `mcp.json` — used to warm the
/// OS keychain blob once before connecting stdio servers (one unlock instead of N).
pub fn catalog_passthrough_key_pairs(cfg: &McpConfig) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (server_key, entry) in &cfg.servers {
        let ServerEntry::Stdio {
            catalog_passthrough_keys,
            ..
        } = entry
        else {
            continue;
        };
        let Some(tool_id) = tool_id_from_catalog_server_key(server_key) else {
            continue;
        };
        if catalog_passthrough_keys.is_empty() {
            continue;
        }
        for k in catalog_passthrough_keys {
            let t = k.trim();
            if !t.is_empty() {
                out.push((tool_id.clone(), t.to_string()));
            }
        }
    }
    out
}

/// Move pre-migration `catalog_passthrough: {KEY: VAL}` secrets from `mcp.json` into the OS
/// keychain, strip any `--env=KEY=VAL` in the stored argv for those keys, and replace the field
/// with `catalog_passthrough_keys: [KEY, …]`. Operates on raw JSON so the serde model can drop
/// the legacy field cleanly — serde would otherwise silently discard the secrets.
fn migrate_legacy_catalog_passthrough(raw: &mut serde_json::Value) -> Result<bool, String> {
    let Some(servers) = raw.get_mut("servers").and_then(|v| v.as_object_mut()) else {
        return Ok(false);
    };

    let mut any_migrated = false;
    for (server_key, server) in servers.iter_mut() {
        let Some(tool_id) = tool_id_from_catalog_server_key(server_key) else {
            continue;
        };
        let Some(obj) = server.as_object_mut() else {
            continue;
        };
        if obj.get("type").and_then(|v| v.as_str()) != Some("stdio") {
            continue;
        }
        let Some(legacy_val) = obj.remove("catalog_passthrough") else {
            continue;
        };
        let Some(legacy_map) = legacy_val.as_object() else {
            continue;
        };
        if legacy_map.is_empty() {
            continue;
        }

        let entries: Vec<(String, serde_json::Value)> = legacy_map
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut remaining = serde_json::Map::new();
        let mut migrated_keys: Vec<String> = Vec::new();
        for (env_key, env_val) in entries {
            let Some(val_str) = env_val.as_str() else {
                remaining.insert(env_key, env_val);
                continue;
            };
            if val_str.trim().is_empty() {
                remaining.insert(env_key, serde_json::Value::String(val_str.to_string()));
                continue;
            }
            match secure_store::save_mcp_secret(&tool_id, &env_key, val_str) {
                Ok(()) => migrated_keys.push(env_key),
                Err(e) => {
                    log::warn!(
                        "migrate legacy catalog_passthrough: could not save {tool_id}/{env_key} \
                         into OS keychain: {e}"
                    );
                    remaining.insert(env_key, serde_json::Value::String(val_str.to_string()));
                }
            }
        }

        let legacy_reinserted = !remaining.is_empty();
        if legacy_reinserted {
            obj.insert(
                "catalog_passthrough".to_string(),
                serde_json::Value::Object(remaining),
            );
        }

        if let Some(args) = obj.get_mut("args").and_then(|v| v.as_array_mut()) {
            args.retain(|arg| {
                let Some(s) = arg.as_str() else {
                    return true;
                };
                let Some(rest) = s.strip_prefix("--env=") else {
                    return true;
                };
                let Some((name, _)) = rest.split_once('=') else {
                    return true;
                };
                !migrated_keys.iter().any(|k| k == name)
            });
        }

        if !migrated_keys.is_empty() {
            migrated_keys.sort();
            migrated_keys.dedup();
            obj.insert(
                "catalog_passthrough_keys".to_string(),
                serde_json::Value::Array(
                    migrated_keys
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
            any_migrated = true;
        } else if legacy_reinserted && migrated_keys.is_empty() {
            // Legacy map was rewritten (e.g. non-string values coalesced) even though nothing
            // reached the keychain.
            any_migrated = true;
        }
    }
    Ok(any_migrated)
}

pub fn save_config(path: &Path, cfg: &McpConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create parent dirs for mcp.json: {e}"))?;
    }
    let pretty = serde_json::to_string_pretty(cfg).map_err(|e| format!("encode mcp.json: {e}"))?;
    std::fs::write(path, pretty).map_err(|e| format!("write mcp.json: {e}"))
}

/// Host folders shared with the File Manager container. After [`migrate_legacy_npx_filesystem`]
/// runs (in [`read_config`]), this is exactly `cfg.workspace_roots`.
pub fn filesystem_allowed_paths(cfg: &McpConfig) -> Vec<String> {
    cfg.workspace_roots.clone()
}

pub fn set_filesystem_allowed_paths(cfg: &mut McpConfig, paths: &[String]) {
    cfg.workspace_roots = sanitize_path_list(paths);
}

/// Trim each path and drop empties — used by both the public setter and the legacy migration.
fn sanitize_path_list(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// Drop legacy `npx @modelcontextprotocol/server-filesystem` server; keep paths in `workspace_roots`.
fn migrate_legacy_npx_filesystem(cfg: &mut McpConfig) -> bool {
    let Some(ServerEntry::Stdio { command, args, .. }) = cfg.servers.get(FILESYSTEM_SERVER_KEY)
    else {
        return false;
    };
    if command != "npx" {
        return false;
    }
    if let Some(pkg_idx) = args.iter().position(|a| a.contains("server-filesystem")) {
        let legacy = sanitize_path_list(&args[pkg_idx + 1..]);
        if cfg.workspace_roots.is_empty() && !legacy.is_empty() {
            cfg.workspace_roots = legacy;
        }
    }
    cfg.servers.remove(FILESYSTEM_SERVER_KEY);
    true
}

fn default_config_value() -> serde_json::Value {
    serde_json::json!({
        "servers": {
            "dice": {
                "type": "native",
                "id": "dice"
            },
            "tool_manager": {
                "type": "native",
                "id": "tool_manager"
            }
        }
    })
}

/// Resolve a catalog passthrough value: host `std::env` first (if set), then OS keychain.
///
/// Env-first avoids touching the keychain when running tests or when developers export a key
/// for one-off runs; in normal GUI use the variable is usually unset and the keychain path runs
/// after [`crate::modules::secure_store::warm_app_secrets`] (in-memory cache, no per-request unlock).
fn resolve_passthrough_value(tool_id: &str, env_key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(env_key) {
        let v = v.trim().to_string();
        if !v.is_empty() {
            return Some(v);
        }
    }
    match secure_store::load_mcp_secret(tool_id, env_key) {
        Ok(v) if !v.trim().is_empty() => Some(v),
        Ok(_) => None,
        Err(secure_store::SecureStoreError::NotFound) => None,
        Err(e) => {
            log::warn!("mcp passthrough: keychain load failed for {tool_id}/{env_key}: {e}");
            None
        }
    }
}

/// Splice `--env=KEY=VAL` flags for each passthrough key into `podman|docker run` argv at the
/// slot just before the image reference (first non-flag arg after `run`). Keys that resolve to
/// no value are skipped silently so the spawn still gets a chance to succeed with other env.
fn splice_passthrough_env_into_argv(
    argv: &[String],
    tool_id: &str,
    keys: &[String],
) -> Vec<String> {
    if keys.is_empty() {
        return argv.to_vec();
    }
    let insert_at = argv
        .iter()
        .enumerate()
        .skip_while(|(_, a)| a.as_str() == "run")
        .find(|(_, a)| !a.starts_with('-'))
        .map(|(i, _)| i)
        .unwrap_or(argv.len());

    let mut out: Vec<String> = Vec::with_capacity(argv.len() + keys.len());
    out.extend_from_slice(&argv[..insert_at]);
    for key in keys {
        if let Some(val) = resolve_passthrough_value(tool_id, key) {
            out.push(format!("--env={key}={val}"));
        }
    }
    out.extend_from_slice(&argv[insert_at..]);
    out
}

pub fn load_or_init_config(path: &Path) -> Result<McpConfig, String> {
    if path.exists() {
        return read_config(path);
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create parent dirs for mcp.json: {e}"))?;
    }
    let default = default_config_value();
    let pretty = serde_json::to_string_pretty(&default)
        .map_err(|e| format!("encode default mcp.json: {e}"))?;
    std::fs::write(path, pretty).map_err(|e| format!("write mcp.json: {e}"))?;
    serde_json::from_value(default).map_err(|e| e.to_string())
}

/// Connect one server from config (native or stdio). Shared by tests and incremental rebuilds.
/// `app_state` is needed for stateful native tools (e.g. `tool_manager`); pass `None` in tests.
pub async fn connect_one_server(
    server_key: &str,
    entry: &ServerEntry,
    app_state: Option<&crate::shared::state::AppState>,
) -> (Option<Provider>, String) {
    match entry {
        ServerEntry::Native { id } => match native::native_for(server_key, id, app_state) {
            Ok(p) => {
                let n = p.tools.len();
                let cmd_word = if n == 1 { "command" } else { "commands" };
                let msg = format!("{server_key} native ({n} {cmd_word})");
                (Some(Provider::Native(Arc::new(p))), msg)
            }
            Err(e) => (None, format!("{server_key} native failed: {e}")),
        },
        ServerEntry::Stdio {
            command,
            args,
            env,
            direct_return,
            catalog_passthrough_keys,
            ..
        } => {
            let spawn_args = match tool_id_from_catalog_server_key(server_key) {
                Some(tool_id) if !catalog_passthrough_keys.is_empty() => {
                    splice_passthrough_env_into_argv(args, &tool_id, catalog_passthrough_keys)
                }
                _ => args.clone(),
            };
            match McpClient::connect(
                server_key.to_string(),
                command.clone(),
                spawn_args,
                env.clone(),
                *direct_return,
            )
            .await
            {
                Ok(client) => {
                    let n = client.tools().len();
                    let cmd_word = if n == 1 { "command" } else { "commands" };
                    let dr = if *direct_return { " direct_return" } else { "" };
                    let msg = format!("{server_key} stdio ({n} {cmd_word}{dr})");
                    (Some(Provider::Mcp(Arc::new(client))), msg)
                }
                Err(e) => (None, format!("{server_key} stdio failed: {e}")),
            }
        }
    }
}

/// Connect every server in `cfg` and return the providers + per-server status lines.
/// Used by tests and as a one-shot rebuild path; the live runtime uses
/// [`rebuild_registry_into_state`] which publishes incrementally.
pub async fn build_mcp_providers(cfg: &McpConfig) -> (Vec<Provider>, Vec<String>) {
    let pairs = catalog_passthrough_key_pairs(cfg);
    if let Err(e) = secure_store::preload_mcp_passthrough_secrets(&pairs) {
        log::warn!("mcp passthrough: keychain preload failed: {e}");
    }

    let mut providers = Vec::new();
    let mut status = Vec::new();

    for (server_key, entry) in &cfg.servers {
        let (prov, line) = connect_one_server(server_key, entry, None).await;
        status.push(line);
        if let Some(p) = prov {
            providers.push(p);
        }
    }

    (providers, status)
}

/// Flip `direct_return` on every tool for one connected stdio server — no new MCP handshake and no
/// reconnect for other servers. Used when `mcp.json` changes only that flag.
///
/// Returns `true` if a matching stdio provider was found and updated.
pub async fn patch_stdio_direct_return_in_registry(
    state: &crate::shared::state::AppState,
    server_key: &str,
    direct_return: bool,
) -> bool {
    let _rebuild = state.mcp_rebuild_mutex.lock().await;
    let mut patched = false;
    {
        let reg = state.mcp.read().await;
        for p in reg.providers() {
            if !p.server_name().eq_ignore_ascii_case(server_key) {
                continue;
            }
            if let Provider::Mcp(client) = p {
                client.set_all_direct_return(direct_return);
                patched = true;
                break;
            }
        }
    }
    if patched {
        state
            .emit_log(
                "mcp",
                &format!("server '{server_key}' direct_return updated (no full reconnect)"),
            )
            .await;
        emit_registry_changed_event(state).await;
    }
    patched
}

/// Reload `mcp.json` from disk and replace the in-memory tool registry.
///
/// Call only after the file on disk is up to date. Holds `mcp_rebuild_mutex` for the full connect
/// phase; uses `mcp_config_mutex` only while reading the file so HTTP config reads are not blocked
/// by slow stdio servers (Podman, npx, …).
///
/// Before connecting, refreshes every installed Tool Engine entry with `mount_workspace` so `podman run`
/// argv matches `workspace_roots` (empty → placeholder root `/tmp` in the image). Saves `mcp.json` when
/// sync succeeds.
pub async fn rebuild_registry_into_state(
    state: &crate::shared::state::AppState,
) -> Result<(), String> {
    let _rebuild = state.mcp_rebuild_mutex.lock().await;
    let catalog_result = crate::modules::tool_engine::service::load_catalog().await;
    let cfg = {
        let _cfg_guard = state.mcp_config_mutex.lock().await;
        let mut cfg = match load_or_init_config(&state.mcp_config_path) {
            Ok(c) => c,
            Err(e) => {
                drop(_cfg_guard);
                let msg = format!("mcp.json error: {e}");
                state.emit_log("mcp", &msg).await;
                return Err(msg);
            }
        };

        let paths = filesystem_allowed_paths(&cfg);
        let bot_id = state
            .connection
            .lock()
            .await
            .as_ref()
            .map(|c| c.bot_id.clone());
        let mut ws_changed = false;
        match &catalog_result {
            Ok(cat) => {
                match crate::modules::tool_engine::service::sync_workspace_mounted_tools_for_catalog(
                    &mut cfg,
                    &paths,
                    cat,
                    &state.mcp_config_path,
                    bot_id,
                ) {
                    Ok(changed) => ws_changed |= changed,
                    Err(e) => {
                        state
                            .emit_log("toolengine", &format!("workspace mount sync skipped: {e}"))
                            .await;
                    }
                }
            }
            Err(e) => {
                state
                    .emit_log("toolengine", &format!("workspace mount sync skipped: {e}"))
                    .await;
            }
        }
        ws_changed |=
            crate::modules::tool_engine::service::sync_custom_tools_if_installed(&mut cfg, &paths);

        if ws_changed {
            if let Err(e) = save_config(&state.mcp_config_path, &cfg) {
                state
                    .emit_log(
                        "mcp",
                        &format!("mcp.json not saved after workspace sync: {e}"),
                    )
                    .await;
            }
        }

        // Ensure tool_manager is always present (auto-add for existing configs).
        if !cfg.servers.contains_key(native::TOOL_MANAGER_ID) {
            cfg.servers.insert(
                native::TOOL_MANAGER_ID.to_string(),
                ServerEntry::Native {
                    id: native::TOOL_MANAGER_ID.to_string(),
                },
            );
            if let Err(e) = save_config(&state.mcp_config_path, &cfg) {
                log::warn!(
                    "failed to save mcp.json after auto-inserting native server {:?}: {} (path={})",
                    native::TOOL_MANAGER_ID,
                    e,
                    state.mcp_config_path.display()
                );
            }
        }

        cfg
    };

    let passthrough_pairs = catalog_passthrough_key_pairs(&cfg);
    if let Err(e) = secure_store::preload_mcp_passthrough_secrets(&passthrough_pairs) {
        log::warn!("mcp passthrough: keychain preload failed: {e}");
    }

    *state.cached_filesystem_paths.write().await = filesystem_allowed_paths(&cfg);

    // Publish the registry after each *successful* connect so native tools (e.g. dice) are usable
    // while slow stdio servers (Podman-backed Tool Engine, npx, …) are still connecting. Failed
    // connects only emit a log line — no need to rebuild the registry.
    //
    // Emit `pengine-registry-changed` after each incremental update so the dashboard reloads MCP
    // commands as soon as a server connects, instead of waiting for every server in `mcp.json`
    // (install returns before background rebuild finishes; stdio can take minutes).
    let mut providers = Vec::new();
    for (server_key, entry) in &cfg.servers {
        let (prov, line) = connect_one_server(server_key, entry, Some(state)).await;
        state.emit_log("mcp", &line).await;
        let Some(p) = prov else { continue };
        providers.push(p);
        *state.mcp.write().await = ToolRegistry::new(providers.clone());
        emit_registry_changed_event(state).await;
    }

    let n = state.mcp.read().await.tool_names().len();
    state
        .emit_log(
            "mcp",
            &format!("ready ({n} tool{})", if n == 1 { "" } else { "s" }),
        )
        .await;

    // If every server failed to connect, nothing in the loop emitted — still notify once so the
    // dashboard can stop waiting on the same stale list as before rebuild.
    if providers.is_empty() {
        emit_registry_changed_event(state).await;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_json(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        p.push(format!("pengine-mcp-svc-{name}-{n}.json"));
        p
    }

    #[test]
    fn read_config_migrates_npx_filesystem_to_workspace_roots() {
        let path = temp_json("migrate");
        std::fs::write(
            &path,
            r#"{"servers":{"filesystem":{"type":"stdio","command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","/host/proj"],"env":{},"direct_return":true},"dice":{"type":"native","id":"dice"}}}"#,
        )
        .unwrap();
        let cfg = read_config(&path).expect("read");
        assert_eq!(cfg.workspace_roots, vec!["/host/proj"]);
        assert!(!cfg.servers.contains_key("filesystem"));
        let round = read_config(&path).expect("read again");
        assert_eq!(round.workspace_roots, vec!["/host/proj"]);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn set_filesystem_paths_writes_workspace_roots_not_npx_server() {
        let mut cfg: McpConfig = serde_json::from_value(serde_json::json!({
            "servers": { "dice": { "type": "native", "id": "dice" } }
        }))
        .unwrap();
        set_filesystem_allowed_paths(&mut cfg, &["/a".into(), "/b".into()]);
        assert_eq!(cfg.workspace_roots, vec!["/a", "/b"]);
        assert!(!cfg.servers.contains_key("filesystem"));
    }

    /// Default resolution: `mcp.json` next to `connection.json` (no project-tree walk).
    #[test]
    fn resolve_mcp_config_uses_app_data_adjacent_to_store() {
        let store = PathBuf::from("/tmp/pengine-fake-app/connection.json");
        let (path, src) = resolve_mcp_config_path(&store);
        assert_eq!(src, "app_data");
        assert_eq!(path, PathBuf::from("/tmp/pengine-fake-app/mcp.json"));
    }

    #[test]
    fn splice_passthrough_env_inserts_before_image_ref() {
        // Stored argv (from disk) has no passthrough --env=; the image ref is the first
        // non-flag arg after `run`.
        let argv = vec![
            "run".into(),
            "--rm".into(),
            "-i".into(),
            "ghcr.io/example/tool:latest".into(),
            "--ignore-robots-txt".into(),
        ];

        std::env::set_var("TEST_PASSTHROUGH_SPLICE_KEY", "host-value");
        let spliced = splice_passthrough_env_into_argv(
            &argv,
            "pengine/nonexistent-tool",
            &["TEST_PASSTHROUGH_SPLICE_KEY".into()],
        );
        std::env::remove_var("TEST_PASSTHROUGH_SPLICE_KEY");

        assert_eq!(
            spliced,
            vec![
                "run".to_string(),
                "--rm".into(),
                "-i".into(),
                "--env=TEST_PASSTHROUGH_SPLICE_KEY=host-value".into(),
                "ghcr.io/example/tool:latest".into(),
                "--ignore-robots-txt".into(),
            ],
            "passthrough --env must land directly before the image reference"
        );
    }

    #[test]
    fn splice_passthrough_env_is_noop_when_no_value_resolvable() {
        let argv = vec!["run".into(), "--rm".into(), "img:tag".into()];
        // Guaranteed-missing env var; keychain will also miss under the test-only tool id.
        let spliced = splice_passthrough_env_into_argv(
            &argv,
            "pengine/nonexistent-tool",
            &["DEFINITELY_NOT_SET_IN_ENV_ZZZ".into()],
        );
        assert_eq!(spliced, argv);
    }

    #[test]
    fn tool_id_from_catalog_server_key_skips_custom_and_native() {
        assert_eq!(
            tool_id_from_catalog_server_key("te_pengine-brave-search").as_deref(),
            Some("pengine/brave-search")
        );
        assert_eq!(tool_id_from_catalog_server_key("te_custom_my-tool"), None);
        assert_eq!(tool_id_from_catalog_server_key("dice"), None);
    }
}
