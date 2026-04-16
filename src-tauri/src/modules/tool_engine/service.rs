use super::runtime::RuntimeInfo;
use super::types::{PrivateFolderConfig, ToolCatalog, ToolEntry, VersionEntry};
use crate::modules::mcp::service as mcp_service;
use crate::modules::mcp::types::{CustomToolEntry, McpConfig, ServerEntry};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Sentinel used in `<bot_id>.<ext>` when no bot is connected yet. The file gets rewritten to
/// `<bot_id>.<ext>` on the next sync after connect, so this name is only ever live when unused.
const BOT_ID_FALLBACK: &str = "default";

const EMBEDDED_CATALOG: &str = include_str!("../../../../tools/mcp-tools.json");

/// Remote registry URL — raw GitHub content. The app fetches this at runtime so
/// users get new tools / version bumps without waiting for a Pengine app update.
const REMOTE_CATALOG_URL: &str =
    "https://raw.githubusercontent.com/pengine-ai/pengine/main/tools/mcp-tools.json";

/// How long to wait for the remote catalog before falling back to embedded.
const REMOTE_CATALOG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Server key prefix for catalog tool-engine entries in `mcp.json`.
const TE_PREFIX: &str = "te_";

/// Server key prefix for custom (developer-added) tool entries.
const TE_CUSTOM_PREFIX: &str = "te_custom_";

/// Sole MCP root when no shared folders are set yet (standard path in Linux images; no extra image dirs).
pub const EMPTY_WORKSPACE_CONTAINER_ROOT: &str = "/tmp";

/// Parse and validate a catalog JSON string. Returns `None` if parsing fails
/// or schema_version is unsupported.
fn parse_catalog(json: &str) -> Option<ToolCatalog> {
    let cat: ToolCatalog = serde_json::from_str(json).ok()?;
    if cat.schema_version != 1 {
        return None;
    }
    Some(cat)
}

/// Load the embedded (compile-time) catalog. Always succeeds on a valid build.
pub fn load_embedded_catalog() -> Result<ToolCatalog, String> {
    serde_json::from_str(EMBEDDED_CATALOG)
        .map_err(|e| format!("parse embedded mcp-tools.json: {e}"))
}

/// Try repo `tools/mcp-tools.json` before the remote catalog (used by `bun run tauri dev` and
/// any run where the file exists next to the workspace). Release builds from CI point at paths
/// that do not exist on end-user machines, so this safely no-ops there.
fn try_load_local_tools_catalog() -> Option<ToolCatalog> {
    let mut paths: Vec<PathBuf> = Vec::new();
    paths.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/mcp-tools.json"));
    if let Ok(mut cwd) = std::env::current_dir() {
        for _ in 0..8 {
            paths.push(cwd.join("tools/mcp-tools.json"));
            if !cwd.pop() {
                break;
            }
        }
    }
    for p in paths {
        if let Ok(json) = std::fs::read_to_string(&p) {
            if let Some(cat) = parse_catalog(&json) {
                log::info!("loaded tool catalog from {}", p.display());
                return Some(cat);
            }
            log::warn!(
                "found {} but it did not parse as catalog schema v1",
                p.display()
            );
        }
    }
    None
}

/// Resolve the tool catalog: prefer repo `tools/mcp-tools.json` when present, then remote,
/// then embedded fallback.
pub async fn load_catalog() -> Result<ToolCatalog, String> {
    if let Some(cat) = try_load_local_tools_catalog() {
        log::info!(
            "using local tools/mcp-tools.json (revision {}); remote fetch skipped",
            cat.catalog_revision
        );
        return Ok(cat);
    }

    match fetch_remote_catalog().await {
        Ok(cat) => {
            log::info!("using remote catalog (revision {})", cat.catalog_revision);
            Ok(cat)
        }
        Err(e) => {
            log::warn!("remote catalog unavailable ({e}), using embedded fallback");
            load_embedded_catalog()
        }
    }
}

/// Try to fetch and parse the remote catalog.
async fn fetch_remote_catalog() -> Result<ToolCatalog, String> {
    let client = reqwest::Client::builder()
        .timeout(REMOTE_CATALOG_TIMEOUT)
        .build()
        .map_err(|e| format!("http client: {e}"))?;

    let resp = client
        .get(REMOTE_CATALOG_URL)
        .send()
        .await
        .map_err(|e| format!("fetch: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let body = resp.text().await.map_err(|e| format!("body: {e}"))?;
    parse_catalog(&body).ok_or_else(|| "invalid or unsupported catalog schema".into())
}

/// Derive the `mcp.json` server key for a tool ID (e.g. `pengine/file-manager` -> `te_pengine-file-manager`).
pub fn server_key(tool_id: &str) -> String {
    format!("{TE_PREFIX}{}", tool_id.replace('/', "-"))
}

/// Default host directory for a catalog tool's `private_folder` (`<mcp.json-parent>/tool-data/<id-with-hyphens>/`).
pub fn default_private_data_dir(mcp_config_path: &Path, tool_id: &str) -> PathBuf {
    let base = mcp_config_path.parent().unwrap_or_else(|| Path::new("."));
    base.join("tool-data").join(tool_id.replace('/', "-"))
}

/// Resolve the host path for private tool data: explicit `mcp.json` override, else [`default_private_data_dir`].
pub fn resolve_private_host_path(
    mcp_config_path: &Path,
    tool_id: &str,
    stored: Option<&str>,
) -> PathBuf {
    if let Some(s) = stored.map(str::trim).filter(|s| !s.is_empty()) {
        PathBuf::from(s)
    } else {
        default_private_data_dir(mcp_config_path, tool_id)
    }
}

fn ensure_private_data_dir(path: &Path) -> Result<(), String> {
    std::fs::create_dir_all(path)
        .map_err(|e| format!("create private tool data dir {}: {e}", path.display()))
}

/// Per-container env entry that points the MCP server at its bot-scoped state file
/// inside the bind-mounted private folder.
fn private_folder_container_env(pf: &PrivateFolderConfig, bot_id: &str) -> (String, String) {
    let root = pf.container_path.trim_end_matches('/');
    let value = format!("{root}/{bot_id}.{}", pf.file_extension);
    (pf.file_env_var.clone(), value)
}

/// Everything the container needs to mount and address the private folder in one bundle.
pub struct PrivateBind<'a> {
    pub host_path: &'a Path,
    pub config: &'a PrivateFolderConfig,
    pub bot_id: &'a str,
}

fn catalog_tool_stdio_eq(a: &ServerEntry, b: &ServerEntry) -> bool {
    match (a, b) {
        (
            ServerEntry::Stdio {
                command: c1,
                args: a1,
                env: e1,
                direct_return: d1,
                private_host_path: p1,
            },
            ServerEntry::Stdio {
                command: c2,
                args: a2,
                env: e2,
                direct_return: d2,
                private_host_path: p2,
            },
        ) => c1 == c2 && a1 == a2 && e1 == e2 && d1 == d2 && p1 == p2,
        _ => false,
    }
}

/// Rebuild argv for one installed catalog tool from `mcp.json` + catalog entry.
/// The container env is baked into argv via `-e` flags, so `ServerEntry.env` stays empty
/// (host-process env does not propagate into the container).
fn rebuild_installed_catalog_tool_stdio(
    entry: &ToolEntry,
    host_paths: &[String],
    mcp_config_path: &Path,
    prev: &ServerEntry,
    bot_id: Option<&str>,
) -> Result<ServerEntry, String> {
    let ServerEntry::Stdio {
        command,
        direct_return,
        private_host_path,
        ..
    } = prev
    else {
        return Err("internal: expected stdio server entry for tool engine catalog tool".into());
    };

    let pb_buf = if entry.private_folder.is_some() {
        let pb =
            resolve_private_host_path(mcp_config_path, &entry.id, private_host_path.as_deref());
        ensure_private_data_dir(&pb)?;
        Some(pb)
    } else {
        None
    };
    let bid = bot_id.unwrap_or(BOT_ID_FALLBACK);
    let private_bind: Option<PrivateBind> = match (&pb_buf, &entry.private_folder) {
        (Some(pb), Some(pf)) => Some(PrivateBind {
            host_path: pb.as_path(),
            config: pf,
            bot_id: bid,
        }),
        _ => None,
    };

    let args = podman_run_argv_for_tool(entry, host_paths, private_bind.as_ref())?;

    Ok(ServerEntry::Stdio {
        command: command.clone(),
        args,
        env: HashMap::new(),
        direct_return: *direct_return,
        private_host_path: private_host_path.clone(),
    })
}

fn sanitize_mount_label(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() || s.chars().all(|c| c == '_') {
        "folder".into()
    } else {
        s
    }
}

/// Each host folder → `/app/<basename>` (basename from the path; duplicates become `name_1`, `name_2`, …).
/// Same order as the MCP allow-list. Used for bind mounts and MCP root argv.
pub fn workspace_app_bind_pairs(host_paths: &[String]) -> Vec<(String, String)> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out = Vec::with_capacity(host_paths.len());
    for h in host_paths {
        let base = Path::new(h.trim())
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("folder");
        let label = sanitize_mount_label(base);
        let mut key = label.clone();
        let mut n = 0u32;
        while seen.contains(&key) {
            n += 1;
            key = format!("{label}_{n}");
        }
        seen.insert(key.clone());
        out.push((h.clone(), format!("/app/{key}")));
    }
    out
}

/// Full `podman|docker run …` argv (excluding the runtime binary) for a catalog tool entry.
/// The image reference is `image@digest` (digest-pinned).
pub fn podman_run_argv_for_tool(
    entry: &ToolEntry,
    host_paths: &[String],
    private_bind: Option<&PrivateBind<'_>>,
) -> Result<Vec<String>, String> {
    if entry.append_workspace_roots && !entry.mount_workspace {
        return Err("catalog: append_workspace_roots requires mount_workspace".into());
    }

    let image_ref = image_reference(entry)?;

    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "-i".into(),
        format!("--cpus={}", entry.limits.cpus),
        format!("--memory={}", entry.limits.memory),
    ];

    if entry.network_isolated {
        args.insert(3, "--network=none".into());
    }

    if entry.container_read_only_rootfs {
        args.push("--read-only".into());
    }

    // Compute the host→container layout once and reuse it for both bind mounts and root args.
    let bind_pairs = if entry.mount_workspace {
        workspace_app_bind_pairs(host_paths)
    } else {
        Vec::new()
    };

    if entry.mount_workspace && !bind_pairs.is_empty() {
        let suffix = if entry.mount_read_only { "ro" } else { "rw" };
        args.extend(
            bind_pairs
                .iter()
                .map(|(host, cpath)| format!("-v={host}:{cpath}:{suffix}")),
        );
    }

    if let Some(pb) = private_bind {
        let host_s = pb.host_path.to_str().ok_or_else(|| {
            format!(
                "private data path must be valid UTF-8: {}",
                pb.host_path.display()
            )
        })?;
        args.push(format!(
            "-v={host_s}:{}:rw",
            pb.config.container_path.trim_end_matches('/')
        ));
        let (k, v) = private_folder_container_env(pb.config, pb.bot_id);
        args.push(format!("--env={k}={v}"));
    }

    args.push(image_ref);
    args.extend(entry.mcp_server_cmd.iter().cloned());

    if entry.append_workspace_roots {
        if bind_pairs.is_empty() {
            args.push(EMPTY_WORKSPACE_CONTAINER_ROOT.to_string());
        } else {
            args.extend(bind_pairs.into_iter().map(|(_, cpath)| cpath));
        }
    }

    Ok(args)
}

/// Resolve the digest for the current (non-yanked, non-revoked) version of a tool.
fn resolve_current_version(entry: &ToolEntry) -> Result<&VersionEntry, String> {
    entry
        .versions
        .iter()
        .find(|v| v.version == entry.current && !v.yanked && !v.revoked)
        .ok_or_else(|| {
            format!(
                "no valid version '{}' found for tool '{}'",
                entry.current, entry.id
            )
        })
}

/// Returns `true` when the current version has a real (non-placeholder) digest.
fn has_pinned_digest(entry: &ToolEntry) -> bool {
    resolve_current_version(entry)
        .map(|v| !v.digest.is_empty() && v.digest != "sha256:placeholder")
        .unwrap_or(false)
}

/// Resolve the digest string for the current version.
/// Returns `None` for placeholder/empty digests (dev builds without a registry image).
fn resolve_current_digest(entry: &ToolEntry) -> Result<Option<String>, String> {
    let ver = resolve_current_version(entry)?;
    if ver.digest.is_empty() || ver.digest == "sha256:placeholder" {
        return Ok(None);
    }
    Ok(Some(ver.digest.clone()))
}

/// The OCI image reference for a tool entry.
///
/// - **Production** (real digest): `ghcr.io/pengine-ai/pengine-file-manager@sha256:abc123…`
/// - **Dev** (placeholder digest): `ghcr.io/pengine-ai/pengine-file-manager:0.1.0` (tagged)
fn image_reference(entry: &ToolEntry) -> Result<String, String> {
    match resolve_current_digest(entry)? {
        Some(digest) => Ok(format!("{}@{}", entry.image, digest)),
        None => Ok(format!("{}:{}", entry.image, entry.current)),
    }
}

async fn image_present(runtime: &RuntimeInfo, image: &str) -> bool {
    tokio::process::Command::new(&runtime.binary)
        .args(["image", "inspect", image])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// A callback for streaming log lines during long-running operations.
pub type LogFn = Box<dyn Fn(&str) + Send + Sync>;

/// Ensure the tool image is available locally. Tries to pull from the registry first;
/// if the image is already present (e.g. from a local `podman build`), uses it directly.
/// All log lines are prefixed with `[tool_id]` so the frontend can filter by tool.
async fn ensure_tool_image(
    runtime: &RuntimeInfo,
    entry: &ToolEntry,
    log: &LogFn,
) -> Result<(), String> {
    let image_ref = image_reference(entry)?;
    let pinned = has_pinned_digest(entry);
    let tag = format!("[{}]", entry.id);

    if image_present(runtime, &image_ref).await {
        log(&format!("{tag} image already present"));
        return Ok(());
    }

    log(&format!("{tag} pulling {}…", image_ref));

    let mut cmd = tokio::process::Command::new(&runtime.binary);
    cmd.args(["pull", &image_ref]);

    match run_streaming_tagged(cmd, log, &tag).await {
        Ok(()) => {}
        Err(e) => {
            // If using a tag (dev mode, no pinned digest), the pull failure is expected
            // when the image hasn't been published yet. Check if it appeared locally
            // (e.g. concurrent build, or tag resolves to a local image).
            if !pinned && image_present(runtime, &image_ref).await {
                log(&format!("{tag} pull failed but image found locally"));
                return Ok(());
            }
            let hint = if pinned {
                "Ensure the image is published to the registry."
            } else {
                "No registry image yet. Build locally: podman build -t <image>:<version> tools/<slug>/"
            };
            return Err(format!("failed to pull image `{image_ref}` — {e}. {hint}"));
        }
    }

    // Verify image is now present after pull.
    if !image_present(runtime, &image_ref).await {
        return Err(format!(
            "pull completed but `{}` is not visible to `{}`",
            image_ref, runtime.binary
        ));
    }

    log(&format!("{tag} image pulled successfully"));
    Ok(())
}

/// Run a command, streaming stderr line-by-line through `log`, prefixed with `tag`.
async fn run_streaming_tagged(
    mut cmd: tokio::process::Command,
    log: &LogFn,
    tag: &str,
) -> Result<(), String> {
    // Pull progress goes to stderr when there is no TTY. Discard stdout to avoid a
    // deadlock if the child fills the pipe buffer writing to a piped-but-unread stdout.
    cmd.stdout(Stdio::null()).stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| format!("failed to spawn: {e}"))?;

    let stderr = child.stderr.take();
    let mut stderr_tail: Vec<String> = Vec::new();

    if let Some(se) = stderr {
        let mut lines = BufReader::new(se).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            log(&format!("{tag} {line}"));
            stderr_tail.push(line);
            if stderr_tail.len() > 50 {
                stderr_tail.remove(0);
            }
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("failed to wait on child: {e}"))?;

    if !status.success() {
        let tail = stderr_tail.join("\n");
        return Err(format!("command failed (exit {}): {}", status, tail.trim()));
    }

    Ok(())
}

pub fn installed_tool_ids(mcp_config_path: &Path) -> Vec<String> {
    let cfg = match mcp_config_path
        .exists()
        .then(|| mcp_service::read_config(mcp_config_path).ok())
        .flatten()
    {
        Some(c) => c,
        None => return Vec::new(),
    };

    cfg.servers
        .keys()
        .filter_map(|k| k.strip_prefix(TE_PREFIX))
        .map(|s| s.replacen('-', "/", 1))
        .collect()
}

/// Rewrite every **installed** catalog tool that uses `mount_workspace` so argv matches `host_paths`
/// (empty list → in-image stub root only). Returns whether `mcp.json` should be saved.
///
/// Pass the catalog from [`load_catalog`] (or tests) so callers can fetch **before** holding
/// `mcp_config_mutex`, avoiding network I/O under that lock.
///
/// `mcp_config_path` and `bot_id` refresh `private_folder` bind mounts and per-bot env paths.
pub fn sync_workspace_mounted_tools_for_catalog(
    cfg: &mut McpConfig,
    host_paths: &[String],
    catalog: &ToolCatalog,
    mcp_config_path: &Path,
    bot_id: Option<String>,
) -> Result<bool, String> {
    let bid = bot_id.as_deref();
    let mut changed = false;
    for entry in &catalog.tools {
        let key = server_key(&entry.id);
        let Some(prev) = cfg.servers.get(&key) else {
            continue;
        };

        let new_entry =
            rebuild_installed_catalog_tool_stdio(entry, host_paths, mcp_config_path, prev, bid)?;

        if !catalog_tool_stdio_eq(prev, &new_entry) {
            cfg.servers.insert(key, new_entry);
            changed = true;
        }
    }
    Ok(changed)
}

/// Pull a whitelisted container image by digest and register it as an MCP stdio server in `mcp.json`.
pub async fn install_tool(
    tool_id: &str,
    runtime: &RuntimeInfo,
    mcp_config_path: &Path,
    mcp_cfg_lock: &tokio::sync::Mutex<()>,
    log: &LogFn,
) -> Result<(), String> {
    let catalog = load_catalog().await?;
    let entry = catalog
        .tools
        .iter()
        .find(|t| t.id == tool_id)
        .ok_or_else(|| format!("tool '{tool_id}' not in catalog (allowlist)"))?;

    ensure_tool_image(runtime, entry, log).await?;

    let _cfg_guard = mcp_cfg_lock.lock().await;
    let mut cfg = mcp_service::load_or_init_config(mcp_config_path)?;
    let host_paths = mcp_service::filesystem_allowed_paths(&cfg);

    let pb_buf = if entry.private_folder.is_some() {
        let pb = resolve_private_host_path(mcp_config_path, tool_id, None);
        ensure_private_data_dir(&pb)?;
        Some(pb)
    } else {
        None
    };
    let private_bind: Option<PrivateBind> = match (&pb_buf, &entry.private_folder) {
        (Some(pb), Some(pf)) => Some(PrivateBind {
            host_path: pb.as_path(),
            config: pf,
            bot_id: BOT_ID_FALLBACK,
        }),
        _ => None,
    };

    let args = podman_run_argv_for_tool(entry, &host_paths, private_bind.as_ref())?;

    let server_entry = ServerEntry::Stdio {
        command: runtime.binary.clone(),
        args,
        env: HashMap::new(),
        direct_return: entry.direct_return,
        private_host_path: None,
    };

    cfg.servers.insert(server_key(tool_id), server_entry);
    mcp_service::save_config(mcp_config_path, &cfg)?;

    Ok(())
}

/// Remove an MCP Tool Engine entry from `mcp.json` and remove the container image.
pub async fn uninstall_tool(
    tool_id: &str,
    runtime: &RuntimeInfo,
    mcp_config_path: &Path,
    mcp_cfg_lock: &tokio::sync::Mutex<()>,
) -> Result<(), String> {
    // Read the installed image ref from mcp.json before removing the entry, so we
    // remove the image that was actually pulled — not whatever the catalog currently
    // resolves to (which may have been updated since install).
    let key = server_key(tool_id);
    let mut installed_image_ref: Option<String> = None;
    if mcp_config_path.exists() {
        let _cfg_guard = mcp_cfg_lock.lock().await;
        let mut cfg = mcp_service::read_config(mcp_config_path)?;
        if let Some(ServerEntry::Stdio { args, .. }) = cfg.servers.get(&key) {
            // In the podman run argv the image ref is the first non-flag token
            // after "run" (flags start with `-`; "run" itself is skipped).
            installed_image_ref = args
                .iter()
                .skip_while(|a| *a == "run")
                .find(|a| !a.starts_with('-'))
                .cloned();
        }
        cfg.servers.remove(&key);
        mcp_service::save_config(mcp_config_path, &cfg)?;
    }

    // Remove the container image — prefer the ref from the installed entry.
    let image_ref = match installed_image_ref {
        Some(r) => Some(r),
        None => load_catalog()
            .await
            .ok()
            .and_then(|cat| cat.tools.iter().find(|t| t.id == tool_id).cloned())
            .and_then(|entry| image_reference(&entry).ok()),
    };
    if let Some(ref img) = image_ref {
        let _ = tokio::process::Command::new(&runtime.binary)
            .args(["rmi", img])
            .output()
            .await;
    }

    Ok(())
}

// ── Custom tools (developer-added Docker images, local only) ──────────

/// Server key for a custom tool entry in `mcp.json`.
fn custom_server_key(key: &str) -> String {
    format!("{TE_CUSTOM_PREFIX}{key}")
}

/// Build `podman|docker run …` argv for a custom tool entry.
fn podman_run_argv_for_custom(entry: &CustomToolEntry, host_paths: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "-i".into(),
        "--network=none".into(),
    ];

    let bind_pairs = if entry.mount_workspace {
        workspace_app_bind_pairs(host_paths)
    } else {
        Vec::new()
    };

    if entry.mount_workspace && !bind_pairs.is_empty() {
        let suffix = if entry.mount_read_only { "ro" } else { "rw" };
        args.extend(
            bind_pairs
                .iter()
                .map(|(host, cpath)| format!("-v={host}:{cpath}:{suffix}")),
        );
    }

    args.push(entry.image.clone());
    args.extend(entry.mcp_server_cmd.iter().cloned());

    if entry.append_workspace_roots {
        if bind_pairs.is_empty() {
            args.push(EMPTY_WORKSPACE_CONTAINER_ROOT.to_string());
        } else {
            args.extend(bind_pairs.into_iter().map(|(_, cpath)| cpath));
        }
    }

    args
}

/// List custom tools from `mcp.json`.
pub fn list_custom_tools(mcp_config_path: &Path) -> Vec<CustomToolEntry> {
    mcp_config_path
        .exists()
        .then(|| mcp_service::read_config(mcp_config_path).ok())
        .flatten()
        .map(|cfg| cfg.custom_tools)
        .unwrap_or_default()
}

/// Add a custom Docker image as an MCP tool. Pulls the image, registers it in `mcp.json`,
/// and stores the entry in `custom_tools` so the dashboard can list it.
pub async fn add_custom_tool(
    entry: CustomToolEntry,
    runtime: &RuntimeInfo,
    mcp_config_path: &Path,
    mcp_cfg_lock: &tokio::sync::Mutex<()>,
    log: &LogFn,
) -> Result<(), String> {
    let tag = format!("[custom/{}]", entry.key);

    // Pull the image (no digest pinning for custom tools — developer controls the tag).
    log(&format!("{tag} pulling {}…", entry.image));
    let mut cmd = tokio::process::Command::new(&runtime.binary);
    cmd.args(["pull", &entry.image]);
    match run_streaming_tagged(cmd, log, &tag).await {
        Ok(()) => log(&format!("{tag} image pulled")),
        Err(e) => {
            // Check if the image is already present locally (e.g. local build).
            if image_present(runtime, &entry.image).await {
                log(&format!("{tag} pull failed but image found locally"));
            } else {
                return Err(format!("failed to pull `{}` — {e}", entry.image));
            }
        }
    }

    let _cfg_guard = mcp_cfg_lock.lock().await;
    let mut cfg = mcp_service::load_or_init_config(mcp_config_path)?;
    let host_paths = mcp_service::filesystem_allowed_paths(&cfg);

    // Prevent duplicate keys.
    if cfg.custom_tools.iter().any(|t| t.key == entry.key) {
        return Err(format!("custom tool '{}' already exists", entry.key));
    }

    let args = podman_run_argv_for_custom(&entry, &host_paths);
    let server_entry = ServerEntry::Stdio {
        command: runtime.binary.clone(),
        args,
        env: HashMap::new(),
        direct_return: entry.direct_return,
        private_host_path: None,
    };

    cfg.servers
        .insert(custom_server_key(&entry.key), server_entry);
    cfg.custom_tools.push(entry);
    mcp_service::save_config(mcp_config_path, &cfg)
}

/// Remove a custom tool from `mcp.json` and optionally remove the image.
pub async fn remove_custom_tool(
    key: &str,
    runtime: &RuntimeInfo,
    mcp_config_path: &Path,
    mcp_cfg_lock: &tokio::sync::Mutex<()>,
) -> Result<(), String> {
    let _cfg_guard = mcp_cfg_lock.lock().await;
    let mut cfg = mcp_service::load_or_init_config(mcp_config_path)?;

    let idx = cfg
        .custom_tools
        .iter()
        .position(|t| t.key == key)
        .ok_or_else(|| format!("custom tool '{key}' not found"))?;

    let removed = cfg.custom_tools.remove(idx);
    cfg.servers.remove(&custom_server_key(key));
    mcp_service::save_config(mcp_config_path, &cfg)?;

    // Best-effort image removal.
    let _ = tokio::process::Command::new(&runtime.binary)
        .args(["rmi", &removed.image])
        .output()
        .await;

    Ok(())
}

/// Rewrite custom tool server entries when workspace paths change (same as catalog tools).
pub fn sync_custom_tools_if_installed(cfg: &mut McpConfig, host_paths: &[String]) -> bool {
    let mut changed = false;
    for entry in &cfg.custom_tools {
        let key = custom_server_key(&entry.key);
        let Some(ServerEntry::Stdio {
            command,
            args,
            env,
            direct_return,
            private_host_path,
        }) = cfg.servers.get(&key)
        else {
            continue;
        };

        let new_args = podman_run_argv_for_custom(entry, host_paths);
        if args == &new_args {
            continue;
        }

        let new_entry = ServerEntry::Stdio {
            command: command.clone(),
            args: new_args,
            env: env.clone(),
            direct_return: *direct_return,
            private_host_path: private_host_path.clone(),
        };
        cfg.servers.insert(key, new_entry);
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_app_layout() {
        let hosts = vec!["/Users/x/pengine".into(), "/opt/other".into()];
        assert_eq!(
            workspace_app_bind_pairs(&hosts),
            vec![
                ("/Users/x/pengine".into(), "/app/pengine".into()),
                ("/opt/other".into(), "/app/other".into()),
            ]
        );
    }

    #[test]
    fn catalog_parses_new_schema() {
        let catalog = load_embedded_catalog().unwrap();
        assert_eq!(catalog.schema_version, 1);
        assert!(!catalog.tools.is_empty());
        let fm = catalog
            .tools
            .iter()
            .find(|t| t.id == "pengine/file-manager")
            .expect("file-manager must be in embedded catalog");
        assert_eq!(fm.current, "0.1.0");
        assert!(!fm.versions.is_empty());
        assert!(fm.image.contains("ghcr.io/pengine-ai/pengine-file-manager"));
        let u = fm
            .upstream_mcp_npm
            .as_ref()
            .expect("file-manager catalog pins upstream MCP npm");
        assert!(u.package.contains("server-filesystem"));
        assert!(!u.version.is_empty());
        let mem = catalog
            .tools
            .iter()
            .find(|t| t.id == "pengine/memory")
            .expect("memory in embedded catalog");
        let mp = mem
            .private_folder
            .as_ref()
            .expect("memory declares private_folder");
        assert_eq!(mp.file_env_var, "MEMORY_FILE_PATH");
    }

    #[test]
    fn duplicate_basenames_get_suffix() {
        let hosts = vec!["/a/foo".into(), "/b/foo".into()];
        let pairs = workspace_app_bind_pairs(&hosts);
        assert_eq!(pairs[0].1, "/app/foo");
        assert_eq!(pairs[1].1, "/app/foo_1");
    }

    #[test]
    fn placeholder_digest_uses_tagged_image_in_argv() {
        let catalog = load_embedded_catalog().unwrap();
        let fm = catalog
            .tools
            .iter()
            .find(|t| t.id == "pengine/file-manager")
            .expect("file-manager in catalog");
        let ver = fm
            .versions
            .iter()
            .find(|v| v.version == fm.current)
            .unwrap();
        assert_eq!(ver.digest, "sha256:placeholder");
        let argv = podman_run_argv_for_tool(fm, &[], None).expect("argv");
        let tagged = format!("{}:{}", fm.image, fm.current);
        let image_ref = argv
            .iter()
            .find(|a| *a == &tagged)
            .expect("tagged image ref in argv");
        assert!(
            !image_ref.contains('@'),
            "placeholder must not use @digest: {image_ref}"
        );
    }

    #[test]
    fn memory_catalog_has_private_folder_and_argv_includes_bind_and_env() {
        let catalog = load_embedded_catalog().unwrap();
        let mem = catalog
            .tools
            .iter()
            .find(|t| t.id == "pengine/memory")
            .expect("memory in catalog");
        let pf = mem
            .private_folder
            .as_ref()
            .expect("memory declares private_folder");
        assert_eq!(pf.container_path, "/mcp/data");
        assert_eq!(pf.file_env_var, "MEMORY_FILE_PATH");

        let tmp = std::env::temp_dir().join("pengine-mem-test-data");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let pb = PrivateBind {
            host_path: tmp.as_path(),
            config: pf,
            bot_id: "12345",
        };
        let argv = podman_run_argv_for_tool(mem, &[], Some(&pb)).expect("argv");

        let want_mount = format!("-v={}:/mcp/data:rw", tmp.to_str().expect("utf8 tmp path"));
        assert!(
            argv.iter().any(|a| a == &want_mount),
            "missing mount: argv={argv:?}"
        );

        // Container env must be passed as `-e` argv — not host env, which podman does not forward.
        let want_env = "--env=MEMORY_FILE_PATH=/mcp/data/12345.json".to_string();
        assert!(
            argv.iter().any(|a| a == &want_env),
            "missing -e flag: argv={argv:?}"
        );
    }
}
