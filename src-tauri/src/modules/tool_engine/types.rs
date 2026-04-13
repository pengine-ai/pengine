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

/// Catalog command line shown in the Tool Engine UI (mirrors the MCP server's tool list).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogCommand {
    pub name: String,
    pub description: String,
}

/// A single released version of a tool, referenced by digest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionEntry {
    pub version: String,
    /// Image digest, e.g. "sha256:abc123…".
    pub digest: String,
    pub released_at: String,
    #[serde(default)]
    pub yanked: bool,
    #[serde(default)]
    pub revoked: bool,
    /// True if this version is a security fix.
    #[serde(default)]
    pub security: bool,
}

/// Optional npm package pinned inside a container image (see `tools/<slug>/Dockerfile`).
/// CI passes these as `docker build` args from `mcp-tools.json` so the registry stays the
/// source of truth for upstream MCP server releases (separate from Pengine’s image `current`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamMcpNpm {
    pub package: String,
    pub version: String,
}

/// One entry in the tool catalog (`tools.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    /// Unique tool identifier, e.g. "pengine/file-manager".
    pub id: String,
    pub name: String,
    pub description: String,
    /// Full OCI image reference (without tag/digest), e.g. "ghcr.io/pengine-ai/tools/pengine-file-manager".
    pub image: String,
    /// The current (latest non-yanked, non-revoked) version string, e.g. "0.1.0".
    pub current: String,
    /// All released versions with their digests and status flags.
    pub versions: Vec<VersionEntry>,
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
    /// When set, image build (`tools-publish.yml`) installs this npm package at this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_mcp_npm: Option<UpstreamMcpNpm>,
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

/// Root of the versioned tool catalog (`tools.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCatalog {
    pub schema_version: u32,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub catalog_revision: u64,
    #[serde(default)]
    pub valid_until: String,
    #[serde(default)]
    pub minimum_pengine_version: String,
    pub tools: Vec<ToolEntry>,
}
