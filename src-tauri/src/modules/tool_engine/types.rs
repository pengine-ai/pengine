use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Podman,
    Docker,
}

fn default_true() -> bool {
    true
}

/// Catalog command line shown in the Tool Engine UI (mirrors the MCP server’s tool list).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogCommand {
    pub name: String,
    pub description: String,
}

/// One entry in the tool catalog (`tools.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    /// Unique tool identifier, e.g. "pengine/file-manager".
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    /// Full OCI image reference, e.g. "file-manager:0.1.0".
    pub image: String,
    /// Expected image digest for verification after pull (empty = skip).
    #[serde(default)]
    pub digest: String,
    /// Relative to the `src-tauri` crate root: if pull fails and the image is missing, run
    /// `podman|docker build -t <image> -f Dockerfile .` in this directory (first install from dev tree).
    #[serde(default)]
    pub build_context: Option<String>,
    /// Extra argv after the image (before auto-appended root paths). Often empty when the image ENTRYPOINT runs MCP.
    #[serde(default)]
    pub mcp_server_cmd: Vec<String>,
    /// When true, add `--read-only` to the container run (rootfs).
    #[serde(default = "default_true")]
    pub container_read_only_rootfs: bool,
    /// When true, use `:ro` on volume binds.
    #[serde(default = "default_true")]
    pub mount_read_only: bool,
    /// When true, `podman|docker run` bind-mounts each allow-list folder under `/app/<basename>`.
    #[serde(default)]
    pub mount_workspace: bool,
    /// When true, append allowed container roots after `image` + `mcp_server_cmd` (for MCP servers
    /// like `@modelcontextprotocol/server-filesystem` that take roots as argv). Requires `mount_workspace`.
    #[serde(default)]
    pub append_workspace_roots: bool,
    /// Tool names for the dashboard (same surface as `@modelcontextprotocol/server-filesystem`).
    #[serde(default)]
    pub commands: Vec<CatalogCommand>,
    /// Resource limits applied to the container.
    #[serde(default)]
    pub limits: ResourceLimits,
    /// When true, tool results go directly to the user without model summarisation.
    #[serde(default)]
    pub direct_return: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    /// CPU quota, e.g. "0.5".
    #[serde(default = "default_cpus")]
    pub cpus: String,
    /// Memory limit, e.g. "256m".
    #[serde(default = "default_memory")]
    pub memory: String,
    /// Kill container after this many seconds.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_cpus() -> String {
    "1.0".into()
}
fn default_memory() -> String {
    "256m".into()
}
fn default_timeout() -> u64 {
    30
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpus: default_cpus(),
            memory: default_memory(),
            timeout_secs: default_timeout(),
        }
    }
}

/// Root of the embedded `tools.json` catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCatalog {
    pub version: u32,
    pub tools: Vec<ToolEntry>,
}
