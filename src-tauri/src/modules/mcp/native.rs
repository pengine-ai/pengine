use super::types::ToolDef;
use serde_json::{json, Value};

const MAX_SIDES: u64 = 1_000_000;

pub struct NativeProvider {
    pub server_name: String,
    pub tools: Vec<ToolDef>,
    handler: fn(&str, &Value) -> Result<String, String>,
}

impl NativeProvider {
    pub fn call(&self, tool_name: &str, args: &Value) -> Result<String, String> {
        if !self.tools.iter().any(|t| t.name == tool_name) {
            return Err(format!("unknown native tool: {tool_name}"));
        }
        (self.handler)(tool_name, args)
    }
}

/// Built-in dice tools under the given server key (must match `mcp.json` server name).
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
        handler: handle_dice,
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

/// Native tool that lets the agent manage Tool Engine catalog tools via messages.
fn tool_manager_named(server_key: &str) -> NativeProvider {
    NativeProvider {
        server_name: server_key.to_string(),
        tools: vec![ToolDef {
            server_name: server_key.to_string(),
            name: "tool_engine_help".to_string(),
            description: Some(
                "Show available Tool Engine catalog tools and how to install them from the dashboard.".into(),
            ),
            input_schema: json!({"type": "object", "properties": {}}),
            direct_return: true,
        }],
        handler: handle_tool_manager,
    }
}

fn handle_tool_manager(tool_name: &str, _args: &Value) -> Result<String, String> {
    match tool_name {
        "tool_engine_help" => Ok(
            "Open the Tool Engine panel in the dashboard to browse and install catalog tools. \
             Each tool runs as an MCP server inside a container. \
             Use the MCP Tools panel to manage workspace folders shared with installed tools."
                .into(),
        ),
        _ => Err(format!("unknown native tool: {tool_name}")),
    }
}

/// Resolve `id` from `mcp.json` (`type: native`) into a provider under `server_key`.
pub fn native_for(server_key: &str, id: &str) -> Result<NativeProvider, String> {
    match id {
        "dice" => Ok(dice_named(server_key)),
        "tool_manager" => Ok(tool_manager_named(server_key)),
        _ => Err(format!("unknown native id: {id}")),
    }
}
