use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Root config: `$APP_DATA/mcp.json` next to `connection.json` for release/native builds; debug
/// builds may use crate `mcp.json` (see `service::resolve_mcp_config_path`). Override with
/// `PENGINE_MCP_CONFIG`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Host folders shared with the File Manager container (`/app/<basename>`). Replaces legacy
    /// `npx @modelcontextprotocol/server-filesystem` entries under `servers.filesystem`.
    #[serde(default)]
    pub workspace_roots: Vec<String>,
    #[serde(default)]
    pub servers: BTreeMap<String, ServerEntry>,
}

/// One logical MCP server. Same top-level shape for every backend: `type` picks the loader.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEntry {
    /// In-process tool pack; `id` selects a built-in (e.g. `dice`).
    Native { id: String },
    /// Child process speaking MCP over stdio (`command` + `args`; Tool Engine uses this for `te_*`).
    Stdio {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
        /// When true, tool results are returned directly to the user without
        /// sending them back to the model for summarisation.
        #[serde(default)]
        direct_return: bool,
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
