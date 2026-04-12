use super::types::ToolDef;
use crate::modules::mcp::service as mcp_service;
use crate::modules::tool_engine::runtime as tool_engine_runtime;
use crate::modules::tool_engine::service as tool_engine_service;
use crate::shared::state::AppState;
use serde_json::{json, Value};
use std::collections::HashSet;

const MAX_SIDES: u64 = 1_000_000;

/// Server key / native id used in `mcp.json` for the built-in tool manager.
pub const TOOL_MANAGER_ID: &str = "tool_manager";

enum NativeKind {
    Dice,
    ToolManager(AppState),
}

pub struct NativeProvider {
    pub server_name: String,
    pub tools: Vec<ToolDef>,
    kind: NativeKind,
}

impl NativeProvider {
    pub async fn call(&self, tool_name: &str, args: &Value) -> Result<String, String> {
        if !self.tools.iter().any(|t| t.name == tool_name) {
            return Err(format!("unknown native tool: {tool_name}"));
        }
        match &self.kind {
            NativeKind::Dice => handle_dice(tool_name, args),
            NativeKind::ToolManager(state) => handle_tool_manager(tool_name, args, state).await,
        }
    }
}

// ── Dice ────────────────────────────────────────────────────────────

pub fn dice_named(server_key: &str) -> NativeProvider {
    NativeProvider {
        server_name: server_key.to_string(),
        tools: vec![ToolDef {
            server_name: server_key.to_string(),
            name: "roll_dice".to_string(),
            description: Some(
                "Roll a die with the given number of sides and return the result.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sides": {
                        "type": "integer",
                        "description": "Number of sides (default 6, max 1 000 000)"
                    }
                }
            }),
            direct_return: true,
        }],
        kind: NativeKind::Dice,
    }
}

pub fn dice() -> NativeProvider {
    dice_named("dice")
}

fn handle_dice(_tool_name: &str, args: &Value) -> Result<String, String> {
    let sides = args
        .get("sides")
        .and_then(|v| v.as_u64())
        .unwrap_or(6)
        .clamp(2, MAX_SIDES);

    let result = fastrand::u64(1..=sides);
    Ok(format!("Rolled a d{sides}: {result}"))
}

// ── Tool Manager ────────────────────────────────────────────────────

pub fn tool_manager_named(server_key: &str, state: AppState) -> NativeProvider {
    NativeProvider {
        server_name: server_key.to_string(),
        tools: vec![ToolDef {
            server_name: server_key.to_string(),
            name: "manage_tools".to_string(),
            description: Some(
                "Manage container-based tools: list available tools, install a tool, or uninstall a tool. \
                 Use action 'list' to see all available tools and their install status. \
                 Use action 'install' with a tool_id to install a new tool. \
                 Use action 'uninstall' with a tool_id to remove an installed tool."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "install", "uninstall"],
                        "description": "The operation to perform"
                    },
                    "tool_id": {
                        "type": "string",
                        "description": "Tool identifier (required for install/uninstall, e.g. 'pengine/file-manager')"
                    }
                }
            }),
            direct_return: false,
        }],
        kind: NativeKind::ToolManager(state),
    }
}

async fn handle_tool_manager(
    _tool_name: &str,
    args: &Value,
    state: &AppState,
) -> Result<String, String> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("missing 'action' parameter")?;

    match action {
        "list" => handle_list_tools(state).await,
        "install" => {
            let tool_id = args
                .get("tool_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'tool_id' for install")?;
            handle_install_tool(tool_id, state).await
        }
        "uninstall" => {
            let tool_id = args
                .get("tool_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'tool_id' for uninstall")?;
            handle_uninstall_tool(tool_id, state).await
        }
        _ => Err(format!("unknown action: {action}")),
    }
}

async fn handle_list_tools(state: &AppState) -> Result<String, String> {
    let catalog = tool_engine_service::load_catalog()?;
    let installed = {
        let _cfg_guard = state.mcp_config_mutex.lock().await;
        tool_engine_service::installed_tool_ids(&state.mcp_config_path)
    };
    let installed_set: HashSet<&str> = installed.iter().map(|s| s.as_str()).collect();

    let mut lines = Vec::new();
    for tool in &catalog.tools {
        let status = if installed_set.contains(tool.id.as_str()) {
            "installed"
        } else {
            "not installed"
        };
        lines.push(format!(
            "- {} (id: {}, v{}): {} [{}]",
            tool.name, tool.id, tool.version, tool.description, status
        ));
    }

    if lines.is_empty() {
        Ok("No tools available in the catalog.".to_string())
    } else {
        Ok(format!("Available tools:\n{}", lines.join("\n")))
    }
}

async fn handle_install_tool(tool_id: &str, state: &AppState) -> Result<String, String> {
    run_tool_mutation(tool_id, state, "install", ToolAction::Install).await?;
    Ok(format!(
        "Tool '{tool_id}' installed successfully and is now available."
    ))
}

async fn handle_uninstall_tool(tool_id: &str, state: &AppState) -> Result<String, String> {
    run_tool_mutation(tool_id, state, "uninstall", ToolAction::Uninstall).await?;
    Ok(format!("Tool '{tool_id}' uninstalled successfully."))
}

enum ToolAction {
    Install,
    Uninstall,
}

/// Shared sequence for install / uninstall: detect runtime, lock, log, act, log, rebuild.
async fn run_tool_mutation(
    tool_id: &str,
    state: &AppState,
    verb: &str,
    action: ToolAction,
) -> Result<(), String> {
    let runtime = tool_engine_runtime::detect_runtime().await.ok_or(
        "No container runtime (Docker/Podman) found. Please install Docker or Podman first.",
    )?;

    {
        let _te_guard = state.tool_engine_mutex.lock().await;
        state
            .emit_log("toolengine", &format!("{verb}ing {tool_id} via chat…"))
            .await;
        match action {
            ToolAction::Install => {
                tool_engine_service::install_tool(
                    tool_id,
                    &runtime,
                    &state.mcp_config_path,
                    &state.mcp_config_mutex,
                )
                .await?;
            }
            ToolAction::Uninstall => {
                tool_engine_service::uninstall_tool(
                    tool_id,
                    &runtime,
                    &state.mcp_config_path,
                    &state.mcp_config_mutex,
                )
                .await?;
            }
        }
        state
            .emit_log("toolengine", &format!("{tool_id} {verb}ed via chat"))
            .await;
    }

    if let Err(e) = mcp_service::rebuild_registry_into_state(state).await {
        state
            .emit_log("mcp", &format!("registry rebuild after {verb} failed: {e}"))
            .await;
    }
    Ok(())
}

// ── Registry ────────────────────────────────────────────────────────

/// Resolve `id` from `mcp.json` (`type: native`) into a provider under `server_key`.
/// `app_state` is required for stateful natives like `tool_manager`.
pub fn native_for(
    server_key: &str,
    id: &str,
    app_state: Option<&AppState>,
) -> Result<NativeProvider, String> {
    match id {
        "dice" => Ok(dice_named(server_key)),
        TOOL_MANAGER_ID => {
            let state = app_state.ok_or_else(|| format!("{TOOL_MANAGER_ID} requires AppState"))?;
            Ok(tool_manager_named(server_key, state.clone()))
        }
        _ => Err(format!("unknown native id: {id}")),
    }
}
