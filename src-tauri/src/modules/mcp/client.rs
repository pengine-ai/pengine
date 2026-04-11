use super::transport::StdioTransport;
use super::types::ToolDef;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

/// `podman run` + `npx -y` inside the container can exceed a minute on cold cache / slow networks.
const MCP_CONNECT_CALL_TIMEOUT: Duration = Duration::from_secs(300);

pub struct McpClient {
    pub server_name: String,
    transport: StdioTransport,
    pub tools: Vec<ToolDef>,
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
            tools,
        })
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
            Some(ToolDef {
                server_name: server_name.to_string(),
                name,
                description: t
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                input_schema: t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({"type": "object"})),
                direct_return: false,
            })
        })
        .collect()
}
