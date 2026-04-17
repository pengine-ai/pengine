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

/// PyPI package pinned inside a container image (Python MCP servers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamMcpPypi {
    pub package: String,
    pub version: String,
}

/// Declares that a tool keeps mutable state on disk. The app bind-mounts a host
/// directory to `container_path` and sets `file_env_var` on the container to
/// `<container_path>/<bot_id>.<file_extension>` so state is scoped per connected bot.
/// Host directory defaults to `$APP_DATA/tool-data/<slug>/` and can be overridden by
/// `PUT /v1/toolengine/private-folder` (`{ "tool_id", "path" }`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivateFolderConfig {
    pub container_path: String,
    pub file_env_var: String,
    pub file_extension: String,
}

/// One entry in the tool catalog (`tools.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolEntry {
    /// Unique tool identifier, e.g. "pengine/file-manager".
    pub id: String,
    pub name: String,
    pub description: String,
    /// Full OCI image reference (without tag/digest), e.g. "ghcr.io/pengine-ai/pengine-file-manager".
    pub image: String,
    /// The current (latest non-yanked, non-revoked) version string, e.g. "0.1.0".
    pub current: String,
    /// All released versions with their digests and status flags.
    pub versions: Vec<VersionEntry>,
    /// Extra argv after the image (before auto-appended root paths). Often empty when the image ENTRYPOINT runs MCP.
    #[serde(default)]
    pub mcp_server_cmd: Vec<String>,
    /// When true, append `--ignore-robots-txt` after `mcp_server_cmd` (Fetch MCP). Default false — robots.txt is honored unless opted in here or via `mcp_server_cmd`.
    #[serde(default)]
    pub ignore_robots_txt: bool,
    /// Reserved for future host-scoped robots / fetch policy (not enforced by the container today). Documented in the catalog for visibility.
    #[serde(default)]
    pub robots_ignore_allowlist: Vec<String>,
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
    /// When true, tool results may go straight to the user without a model pass (host may still
    /// override for specific tools). Default false; `pengine/fetch` ships with false so replies stay human-readable.
    #[serde(default)]
    pub direct_return: bool,
    /// When set, image build (`tools-publish.yml`) installs this npm package at this version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_mcp_npm: Option<UpstreamMcpNpm>,
    /// When set, image build installs this PyPI package at this version (Python MCP servers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_mcp_pypi: Option<UpstreamMcpPypi>,
    /// When true (default), run the tool container with `--network=none`. Set false for servers
    /// that need outbound network (e.g. web fetch).
    #[serde(default = "default_true")]
    pub network_isolated: bool,
    /// When set, the app bind-mounts a host folder into the container and passes a per-bot file
    /// path via env so the tool can persist state (e.g. the Memory server's knowledge-graph JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_folder: Option<PrivateFolderConfig>,
    /// Host env var names to forward into the container via `--env=KEY=VALUE`. Missing host
    /// vars are silently skipped (the container is expected to surface its own error). Used
    /// for required secrets like `BRAVE_API_KEY` that must reach the MCP server at runtime.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub passthrough_env: Vec<String>,
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
