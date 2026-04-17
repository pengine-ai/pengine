use super::transport::StdioTransport;
use super::types::ToolDef;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;

/// `podman run` + `npx -y` inside the container can exceed a minute on cold cache / slow networks.
const MCP_CONNECT_CALL_TIMEOUT: Duration = Duration::from_secs(300);

pub struct McpClient {
    pub server_name: String,
    transport: StdioTransport,
    tool_defs: RwLock<Vec<ToolDef>>,
}

impl McpClient {
    pub async fn connect(
        server_name: String,
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
        direct_return: bool,
    ) -> Result<Self, String> {
        let transport = StdioTransport::spawn(&command, &args, &env).await?;

        let init_params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "pengine", "version": "0.1.0" },
        });
        transport
            .call_with_timeout("initialize", Some(init_params), MCP_CONNECT_CALL_TIMEOUT)
            .await?;
        let _ = transport.notify("notifications/initialized", None).await;

        let result = transport
            .call_with_timeout("tools/list", None, MCP_CONNECT_CALL_TIMEOUT)
            .await?;
        let mut tools = parse_tools(&server_name, &result);

        if direct_return {
            for tool in &mut tools {
                tool.direct_return = true;
            }
        }

        Ok(Self {
            server_name,
            transport,
            tool_defs: RwLock::new(tools),
        })
    }

    /// Snapshot of tool definitions (names, schemas, `direct_return`, …).
    pub fn tools(&self) -> Vec<ToolDef> {
        self.tool_defs
            .read()
            .expect("tool_defs lock poisoned")
            .clone()
    }

    /// Update the `direct_return` flag on every tool for this server without reconnecting stdio.
    pub fn set_all_direct_return(&self, direct_return: bool) {
        let mut tools = self.tool_defs.write().expect("tool_defs lock poisoned");
        for t in tools.iter_mut() {
            t.direct_return = direct_return;
        }
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        let result = self
            .transport
            .call(
                "tools/call",
                Some(json!({ "name": name, "arguments": args })),
            )
            .await?;

        let mut out = String::new();
        if let Some(items) = result.get("content").and_then(|v| v.as_array()) {
            for item in items {
                if let Some(t) = item.get("text").and_then(|v| v.as_str()) {
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    out.push_str(t);
                }
            }
        }
        if out.is_empty() {
            out = result.to_string();
        }
        Ok(out)
    }
}

fn parse_tools(server_name: &str, result: &Value) -> Vec<ToolDef> {
    let Some(arr) = result.get("tools").and_then(|v| v.as_array()) else {
        return vec![];
    };
    arr.iter()
        .filter_map(|t| {
            let name = t.get("name")?.as_str()?.to_string();
            Some({
                let mut def = ToolDef {
                    server_name: server_name.to_string(),
                    name,
                    description: t
                        .get("description")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    input_schema: t
                        .get("inputSchema")
                        .or_else(|| t.get("input_schema"))
                        .cloned()
                        .unwrap_or_else(|| json!({"type": "object"})),
                    direct_return: false,
                    category: None,
                    risk: super::types::ToolRisk::Low,
                };
                super::tool_metadata::apply(&mut def);
                def
            })
        })
        .collect()
}
