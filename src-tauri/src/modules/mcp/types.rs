use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};

/// Root config: `$APP_DATA/mcp.json` next to `connection.json` (see `service::resolve_mcp_config_path`).
/// Override with `PENGINE_MCP_CONFIG`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    /// Host folders shared with the File Manager container (`/app/<basename>`). Replaces legacy
    /// `npx @modelcontextprotocol/server-filesystem` entries under `servers.filesystem`.
    #[serde(default)]
    pub workspace_roots: Vec<String>,
    #[serde(default)]
    pub servers: BTreeMap<String, ServerEntry>,
    /// Developer-added Docker images not in the remote registry. Local only — never pushed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub custom_tools: Vec<CustomToolEntry>,
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
        /// For catalog tools that declare `private_folder`: the host directory currently mounted
        /// into the container. Defaults to `$APP_DATA/tool-data/<slug>/`; user overrides land here.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        private_host_path: Option<String>,
    },
}

/// A developer-added custom Docker image registered as an MCP tool.
/// Stored locally in `mcp.json` — never pushed to the remote registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomToolEntry {
    /// Unique local key, e.g. "my-tool". Used as the server key prefix `te_custom_<key>`.
    pub key: String,
    /// Human-readable name shown in the dashboard.
    pub name: String,
    /// Full Docker/OCI image reference, e.g. "ghcr.io/user/my-mcp:latest" or "localhost/my-mcp:dev".
    pub image: String,
    /// Extra argv after the image (before workspace roots). Empty when ENTRYPOINT is the MCP server.
    #[serde(default)]
    pub mcp_server_cmd: Vec<String>,
    /// Bind-mount workspace folders into the container.
    #[serde(default)]
    pub mount_workspace: bool,
    /// Use `:ro` on bind mounts (ignored when mount_workspace is false).
    #[serde(default = "super_default_true")]
    pub mount_read_only: bool,
    /// Append container mount paths as argv (for MCP servers that take roots as args).
    #[serde(default)]
    pub append_workspace_roots: bool,
    /// Return tool results directly to user without model summarisation.
    #[serde(default)]
    pub direct_return: bool,
}

fn super_default_true() -> bool {
    true
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
