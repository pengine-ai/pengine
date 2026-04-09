use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Root config: `src-tauri/mcp.json` in dev or `mcp.json` next to app data (`connection.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    #[serde(default)]
    pub servers: BTreeMap<String, ServerEntry>,
}

/// One logical MCP server. Same top-level shape for every backend: `type` picks the loader.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEntry {
    /// In-process tool pack; `id` selects a built-in (e.g. `dice`).
    Native { id: String },
    /// Child process speaking MCP over stdio (`docker run … -i` is just command + args).
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
    },
}

/// Definition of a single tool, regardless of where it runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    #[serde(skip)]
    pub server_name: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: serde_json::Value,
    #[serde(skip)]
    pub direct_return: bool,
}
