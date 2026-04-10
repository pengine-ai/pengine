//! Load `mcp.json` and build [`ToolRegistry`] — same code path for `native` and `stdio`
//! (Docker is just `command` + `args` on a `stdio` entry).

use super::client::McpClient;
use super::native;
use super::registry::{Provider, ToolRegistry};
use super::types::{McpConfig, ServerEntry};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const FILESYSTEM_SERVER_KEY: &str = "filesystem";

/// Prefer project `mcp.json` under `src-tauri/` (or crate-root `mcp.json`) by walking up from
/// [`std::env::current_exe`], so resolution does not depend on process CWD. Falls back to
/// `mcp.json` next to `connection.json` in app data.
pub fn resolve_mcp_config_path(store_path: &Path) -> (PathBuf, &'static str) {
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.parent().map(Path::to_path_buf);
        for _ in 0..16 {
            let Some(ref d) = dir else {
                break;
            };
            let from_repo_root = d.join("src-tauri").join("mcp.json");
            if from_repo_root.exists() {
                return (from_repo_root, "project");
            }
            let in_crate_root = d.join("mcp.json");
            if d.join("Cargo.toml").exists() && in_crate_root.exists() {
                return (in_crate_root, "project");
            }
            dir = d.parent().map(Path::to_path_buf);
        }
    }

    let app_path = store_path
        .parent()
        .map(|p| p.join("mcp.json"))
        .unwrap_or_else(|| PathBuf::from("mcp.json"));
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

/// Host paths shared with the File Manager container (and previously the legacy npx filesystem MCP).
pub fn filesystem_allowed_paths(cfg: &McpConfig) -> Vec<String> {
    if !cfg.workspace_roots.is_empty() {
        return cfg.workspace_roots.clone();
    }
    let Some(ServerEntry::Stdio { args, .. }) = cfg.servers.get(FILESYSTEM_SERVER_KEY) else {
        return Vec::new();
    };
    let Some(pkg_idx) = args.iter().position(|a| a.contains("server-filesystem")) else {
        return Vec::new();
    };
    args[pkg_idx + 1..]
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

pub fn set_filesystem_allowed_paths(cfg: &mut McpConfig, paths: &[String]) {
    cfg.workspace_roots = paths
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    cfg.servers.remove(FILESYSTEM_SERVER_KEY);
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
    let Some(pkg_idx) = args.iter().position(|a| a.contains("server-filesystem")) else {
        return false;
    };
    let legacy: Vec<String> = args[pkg_idx + 1..]
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    if cfg.workspace_roots.is_empty() && !legacy.is_empty() {
        cfg.workspace_roots = legacy;
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
pub async fn connect_one_server(
    server_key: &str,
    entry: &ServerEntry,
) -> (Option<Provider>, String) {
    match entry {
        ServerEntry::Native { id } => match native::native_for(server_key, id) {
            Ok(p) => {
                let n = p.tools.len();
                let msg = format!(
                    "{server_key} native ({n} tool{})",
                    if n == 1 { "" } else { "s" }
                );
                (Some(Provider::Native(Arc::new(p))), msg)
            }
            Err(e) => (None, format!("{server_key} native failed: {e}")),
        },
        ServerEntry::Stdio {
            command,
            args,
            env,
            direct_return,
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
                let dr = if *direct_return { " direct_return" } else { "" };
                let msg = format!(
                    "{server_key} stdio ({n} tool{}{})",
                    if n == 1 { "" } else { "s" },
                    dr
                );
                (Some(Provider::Mcp(Arc::new(client))), msg)
            }
            Err(e) => (None, format!("{server_key} stdio failed: {e}")),
        },
    }
}

/// Build MCP providers only (native + stdio). Used by tests and by [`build_registry`].
pub async fn build_mcp_providers(cfg: &McpConfig) -> (Vec<Provider>, Vec<String>) {
    let mut providers = Vec::new();
    let mut status = Vec::new();

    for (server_key, entry) in &cfg.servers {
        let (prov, line) = connect_one_server(server_key, entry).await;
        status.push(line);
        if let Some(p) = prov {
            providers.push(p);
        }
    }

    (providers, status)
}

/// Build full registry from MCP config (native + stdio providers).
pub async fn build_registry(cfg: &McpConfig) -> (ToolRegistry, Vec<String>) {
    let (providers, status) = build_mcp_providers(cfg).await;
    (ToolRegistry::new(providers), status)
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
pub async fn rebuild_registry_into_state(state: &crate::shared::state::AppState) {
    let _rebuild = state.mcp_rebuild_mutex.lock().await;
    let cfg = {
        let _cfg_guard = state.mcp_config_mutex.lock().await;
        let mut cfg = match load_or_init_config(&state.mcp_config_path) {
            Ok(c) => c,
            Err(e) => {
                drop(_cfg_guard);
                state.emit_log("mcp", &format!("mcp.json error: {e}")).await;
                return;
            }
        };

        let paths = filesystem_allowed_paths(&cfg);
        if let Some(rt) = crate::modules::tool_engine::runtime::detect_runtime().await {
            match crate::modules::tool_engine::service::sync_workspace_mounted_tools_if_installed(
                &mut cfg, &paths, &rt,
            ) {
                Ok(changed) => {
                    if changed {
                        if let Err(e) = save_config(&state.mcp_config_path, &cfg) {
                            state
                                .emit_log(
                                    "mcp",
                                    &format!("mcp.json not saved after workspace sync: {e}"),
                                )
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
        }

        cfg
    };

    *state.cached_filesystem_paths.write().await = filesystem_allowed_paths(&cfg);

    // Publish the registry after each server so native tools (e.g. dice) are usable while slow
    // stdio servers (e.g. Podman-backed Tool Engine) are still connecting.
    let mut providers = Vec::new();
    for (server_key, entry) in &cfg.servers {
        let (prov, line) = connect_one_server(server_key, entry).await;
        state.emit_log("mcp", &line).await;
        if let Some(p) = prov {
            providers.push(p);
        }
        let registry = ToolRegistry::new(providers.clone());
        *state.mcp.write().await = registry;
    }

    let n = state.mcp.read().await.tool_names().len();
    state
        .emit_log(
            "mcp",
            &format!("ready ({n} tool{})", if n == 1 { "" } else { "s" }),
        )
        .await;
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
}
