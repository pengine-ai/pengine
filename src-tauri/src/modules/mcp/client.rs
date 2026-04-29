use super::http_transport::HttpTransport;
use super::transport::StdioTransport;
use super::types::ToolDef;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Duration;

/// Handshake (`initialize`, `tools/list`) including cold `podman run` / image pull — keep bounded so the UI does not stall for many minutes.
const MCP_CONNECT_CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// Default JSON-RPC deadline for most `tools/call` traffic (stdio/http transport defaults match).
const MCP_TOOLS_CALL_TIMEOUT_DEFAULT: Duration = Duration::from_secs(60);

/// Recursive tree / glob search can be slow; keep a hard cap so the agent does not sit 10+ minutes.
const MCP_TOOLS_CALL_TIMEOUT_SEARCH: Duration = Duration::from_secs(90);

/// Full-repo trees are cheaper once excludes trim `node_modules` / `target` / `.git` (see agent merge),
/// but source-heavy repos still need headroom below multi‑minute stalls.
const MCP_TOOLS_CALL_TIMEOUT_TREE: Duration = Duration::from_secs(180);

/// Shell MCP (`pengine/shell`, `shell_execute`, interactive terminals) — align with catalog
/// `limits.timeout_secs` (300); default 60s caused spurious audit errors on `cargo`/`npm`/review scripts.
const MCP_TOOLS_CALL_TIMEOUT_SHELL: Duration = Duration::from_secs(300);

fn tools_call_timeout(tool_name: &str) -> Duration {
    match tool_name {
        "directory_tree" => MCP_TOOLS_CALL_TIMEOUT_TREE,
        "search_files" => MCP_TOOLS_CALL_TIMEOUT_SEARCH,
        // Foreground shell runs are often builds/tests; keep under catalog container ceiling.
        "shell_execute" => MCP_TOOLS_CALL_TIMEOUT_SHELL,
        // PTY / monitoring can block until user interaction or process exit.
        name if name.starts_with("terminal_") || name == "process_monitor" => {
            MCP_TOOLS_CALL_TIMEOUT_SHELL
        }
        _ => MCP_TOOLS_CALL_TIMEOUT_DEFAULT,
    }
}

/// Underlying wire to one MCP server. Variants share the same `call`/`notify`
/// surface so [`McpClient`] doesn't care which one connected.
pub enum Transport {
    Stdio(StdioTransport),
    Http(HttpTransport),
}

impl Transport {
    pub async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        match self {
            Transport::Stdio(t) => t.call(method, params).await,
            Transport::Http(t) => t.call(method, params).await,
        }
    }

    pub async fn call_with_timeout(
        &self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
    ) -> Result<Value, String> {
        match self {
            Transport::Stdio(t) => t.call_with_timeout(method, params, timeout).await,
            Transport::Http(t) => t.call_with_timeout(method, params, timeout).await,
        }
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        match self {
            Transport::Stdio(t) => t.notify(method, params).await,
            Transport::Http(t) => t.notify(method, params).await,
        }
    }
}

pub struct McpClient {
    pub server_name: String,
    transport: Transport,
    tool_defs: RwLock<Vec<ToolDef>>,
}

impl McpClient {
    /// Connect over a child-process stdio MCP server.
    pub async fn connect(
        server_name: String,
        command: String,
        args: Vec<String>,
        env: HashMap<String, String>,
        direct_return: bool,
    ) -> Result<Self, String> {
        let transport = Transport::Stdio(StdioTransport::spawn(&command, &args, &env).await?);
        Self::initialize(server_name, transport, direct_return).await
    }

    /// Connect over an HTTP MCP server (Claude Code `"type": "http"` shape).
    pub async fn connect_http(
        server_name: String,
        url: String,
        headers: HashMap<String, String>,
        direct_return: bool,
    ) -> Result<Self, String> {
        let transport = Transport::Http(HttpTransport::new(url, headers)?);
        Self::initialize(server_name, transport, direct_return).await
    }

    async fn initialize(
        server_name: String,
        transport: Transport,
        direct_return: bool,
    ) -> Result<Self, String> {
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

    /// Update the `direct_return` flag on every tool for this server without reconnecting.
    pub fn set_all_direct_return(&self, direct_return: bool) {
        let mut tools = self.tool_defs.write().expect("tool_defs lock poisoned");
        for t in tools.iter_mut() {
            t.direct_return = direct_return;
        }
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        let result = self
            .transport
            .call_with_timeout(
                "tools/call",
                Some(json!({ "name": name, "arguments": args })),
                tools_call_timeout(name),
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
