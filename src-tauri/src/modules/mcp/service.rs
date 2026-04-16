//! Load `mcp.json` and build [`ToolRegistry`] — same code path for `native` and `stdio`
//! (Docker is just `command` + `args` on a `stdio` entry).

use super::client::McpClient;
use super::native;
use super::registry::{Provider, ToolRegistry};
use super::types::{McpConfig, ServerEntry};
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
    let mut cfg: McpConfig = serde_json::from_str(&raw).map_err(|e| {
        format!(
            "parse mcp.json: {e} — every server entry needs a \"type\" field (\"native\" or \"stdio\")"
        )
    })?;
    if migrate_legacy_npx_filesystem(&mut cfg) {
        save_config(path, &cfg)?;
    }
    Ok(cfg)
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
            ..
        } => match McpClient::connect(
            server_key.to_string(),
            command.clone(),
            args.clone(),
            env.clone(),
            *direct_return,
        )
        .await
        {
            Ok(client) => {
                let n = client.tools.len();
                let cmd_word = if n == 1 { "command" } else { "commands" };
                let dr = if *direct_return { " direct_return" } else { "" };
                let msg = format!("{server_key} stdio ({n} {cmd_word}{dr})");
                (Some(Provider::Mcp(Arc::new(client))), msg)
            }
            Err(e) => (None, format!("{server_key} stdio failed: {e}")),
        },
    }
}

/// Connect every server in `cfg` and return the providers + per-server status lines.
/// Used by tests and as a one-shot rebuild path; the live runtime uses
/// [`rebuild_registry_into_state`] which publishes incrementally.
pub async fn build_mcp_providers(cfg: &McpConfig) -> (Vec<Provider>, Vec<String>) {
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
}
