//! Load `mcp.json` and build [`ToolRegistry`] — same code path for `native` and `stdio`
//! (Docker is just `command` + `args` on a `stdio` entry).

use super::client::McpClient;
use super::native;
use super::registry::{Provider, ToolRegistry};
use super::types::{McpConfig, ServerEntry};
use std::path::{Path, PathBuf};

const FILESYSTEM_SERVER_KEY: &str = "filesystem";
const FILESYSTEM_PKG: &str = "@modelcontextprotocol/server-filesystem";

/// Prefer project `mcp.json` under `src-tauri/` (or `./mcp.json` when cwd is `src-tauri`),
/// otherwise `mcp.json` next to `connection.json` in app data.
pub fn resolve_mcp_config_path(store_path: &Path) -> (PathBuf, &'static str) {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let from_repo_root = cwd.join("src-tauri").join("mcp.json");
    if from_repo_root.exists() {
        return (from_repo_root, "project");
    }

    let in_crate_root = cwd.join("mcp.json");
    if cwd.join("Cargo.toml").exists() && in_crate_root.exists() {
        return (in_crate_root, "project");
    }

    let app_path = store_path
        .parent()
        .map(|p| p.join("mcp.json"))
        .unwrap_or_else(|| PathBuf::from("mcp.json"));
    (app_path, "app_data")
}

pub fn read_config(path: &Path) -> Result<McpConfig, String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read mcp.json: {e}"))?;
    serde_json::from_str(&raw).map_err(|e| {
        format!(
            "parse mcp.json: {e} — every server entry needs a \"type\" field (\"native\" or \"stdio\")"
        )
    })
}

pub fn save_config(path: &Path, cfg: &McpConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let pretty = serde_json::to_string_pretty(cfg).map_err(|e| format!("encode mcp.json: {e}"))?;
    std::fs::write(path, pretty).map_err(|e| format!("write mcp.json: {e}"))
}

/// Allowed folder for the official MCP filesystem stdio server (last path segment in `args`).
pub fn filesystem_allowed_path(cfg: &McpConfig) -> Option<String> {
    let ServerEntry::Stdio { args, .. } = cfg.servers.get(FILESYSTEM_SERVER_KEY)? else {
        return None;
    };
    if !args.iter().any(|a| a.contains("server-filesystem")) {
        return None;
    }
    args.last().cloned()
}

pub fn set_filesystem_allowed_path(cfg: &mut McpConfig, abs_path: &str) {
    let entry = ServerEntry::Stdio {
        command: "npx".into(),
        args: vec![
            "-y".into(),
            FILESYSTEM_PKG.into(),
            abs_path.trim().to_string(),
        ],
        env: Default::default(),
        direct_return: true,
    };
    cfg.servers.insert(FILESYSTEM_SERVER_KEY.into(), entry);
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
        let _ = std::fs::create_dir_all(parent);
    }
    let default = default_config_value();
    let pretty = serde_json::to_string_pretty(&default)
        .map_err(|e| format!("encode default mcp.json: {e}"))?;
    std::fs::write(path, pretty).map_err(|e| format!("write mcp.json: {e}"))?;
    serde_json::from_value(default).map_err(|e| e.to_string())
}

/// Connect every server in order (stable `BTreeMap` keys). Returns registry + status lines.
pub async fn build_registry(cfg: &McpConfig) -> (ToolRegistry, Vec<String>) {
    let mut providers = Vec::new();
    let mut status = Vec::new();

    for (server_key, entry) in &cfg.servers {
        match entry {
            ServerEntry::Native { id } => match native::native_for(server_key, id) {
                Ok(p) => {
                    let n = p.tools.len();
                    providers.push(Provider::Native(p));
                    status.push(format!(
                        "{server_key} native ({n} tool{})",
                        if n == 1 { "" } else { "s" }
                    ));
                }
                Err(e) => status.push(format!("{server_key} native failed: {e}")),
            },
            ServerEntry::Stdio {
                command,
                args,
                env,
                direct_return,
            } => match McpClient::connect(
                server_key.clone(),
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
                    providers.push(Provider::Mcp(Box::new(client)));
                    status.push(format!(
                        "{server_key} stdio ({n} tool{}{dr})",
                        if n == 1 { "" } else { "s" }
                    ));
                }
                Err(e) => status.push(format!("{server_key} stdio failed: {e}")),
            },
        }
    }

    (ToolRegistry::new(providers), status)
}

/// Replace in-memory tools after a config change (writes should use [`save_config`] first).
pub async fn rebuild_registry_into_state(state: &crate::shared::state::AppState, cfg: &McpConfig) {
    let (registry, status) = build_registry(cfg).await;
    for line in status {
        state.emit_log("mcp", &line).await;
    }
    let n = registry.tool_names().len();
    *state.mcp.write().await = registry;
    state
        .emit_log(
            "mcp",
            &format!("ready ({n} tool{})", if n == 1 { "" } else { "s" }),
        )
        .await;
}
