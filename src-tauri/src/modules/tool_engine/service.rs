use super::runtime::RuntimeInfo;
use super::types::{ToolCatalog, ToolEntry};
use crate::modules::mcp::service as mcp_service;
use crate::modules::mcp::types::{McpConfig, ServerEntry};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

const EMBEDDED_CATALOG: &str = include_str!("tools.json");

/// Server key prefix for tool-engine entries in `mcp.json`.
const TE_PREFIX: &str = "te_";

/// Sole MCP root when no shared folders are set yet (standard path in Linux images; no extra image dirs).
pub const EMPTY_WORKSPACE_CONTAINER_ROOT: &str = "/tmp";

pub fn load_catalog() -> Result<ToolCatalog, String> {
    serde_json::from_str(EMBEDDED_CATALOG).map_err(|e| format!("parse embedded tools.json: {e}"))
}

/// Derive the `mcp.json` server key for a tool ID (e.g. `pengine/file-manager` -> `te_pengine-file-manager`).
fn server_key(tool_id: &str) -> String {
    format!("{TE_PREFIX}{}", tool_id.replace('/', "-"))
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
pub fn podman_run_argv_for_tool(
    entry: &ToolEntry,
    host_paths: &[String],
) -> Result<Vec<String>, String> {
    if entry.append_workspace_roots && !entry.mount_workspace {
        return Err("catalog: append_workspace_roots requires mount_workspace".into());
    }

    let mut args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "-i".into(),
        "--network=none".into(),
        format!("--cpus={}", entry.limits.cpus),
        format!("--memory={}", entry.limits.memory),
    ];

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

    args.push(entry.image.clone());
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

/// Resolve Dockerfile directory: env override, else path relative to the `src-tauri` crate.
fn resolve_build_context_dir(rel: &str) -> PathBuf {
    if let Ok(p) = std::env::var("PENGINE_FILE_MANAGER_BUILD_CTX") {
        PathBuf::from(p)
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)
    }
}

/// Pull from registry, or use a local image, or build from `build_context` when configured.
async fn ensure_tool_image(runtime: &RuntimeInfo, entry: &ToolEntry) -> Result<(), String> {
    if image_present(runtime, &entry.image).await {
        return Ok(());
    }

    let pull_output = tokio::process::Command::new(&runtime.binary)
        .args(["pull", &entry.image])
        .output()
        .await
        .map_err(|e| format!("failed to pull image: {e}"))?;

    if pull_output.status.success() {
        return Ok(());
    }

    if image_present(runtime, &entry.image).await {
        return Ok(());
    }

    let Some(rel) = entry
        .build_context
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    else {
        let stderr = String::from_utf8_lossy(&pull_output.stderr);
        return Err(format!(
            "image `{}` not available — {}. Install from a Pengine source tree (auto-build), or run ./build in src-tauri/src/modules/tool_engine/container/file-manager/, or publish the image to a registry.",
            entry.image,
            stderr.trim()
        ));
    };

    let ctx = resolve_build_context_dir(rel);
    if !ctx.join("Dockerfile").is_file() {
        let stderr = String::from_utf8_lossy(&pull_output.stderr);
        return Err(format!(
            "image `{}` missing and no Dockerfile at {} (pull: {}). Set PENGINE_FILE_MANAGER_BUILD_CTX or build the image manually.",
            entry.image,
            ctx.display(),
            stderr.trim()
        ));
    }

    let build_fut = tokio::process::Command::new(&runtime.binary)
        .current_dir(&ctx)
        .arg("build")
        .arg("-t")
        .arg(&entry.image)
        .arg("-f")
        .arg("Dockerfile")
        .arg(".")
        .output();

    let build_output = tokio::time::timeout(Duration::from_secs(900), build_fut)
        .await
        .map_err(|_| "container image build timed out after 15 minutes".to_string())?
        .map_err(|e| format!("container build failed to start: {e}"))?;

    if !build_output.status.success() {
        let mut msg = String::from_utf8_lossy(&build_output.stderr).to_string();
        if msg.trim().is_empty() {
            msg = String::from_utf8_lossy(&build_output.stdout).to_string();
        }
        const MAX: usize = 6000;
        let tail = if msg.len() > MAX {
            format!("…{}", &msg[msg.len() - MAX..])
        } else {
            msg
        };
        return Err(format!(
            "building `{}` failed: {}",
            entry.image,
            tail.trim()
        ));
    }

    if !image_present(runtime, &entry.image).await {
        return Err(format!(
            "build finished but `{}` is not visible to `{}`",
            entry.image, runtime.binary
        ));
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

/// Pull a whitelisted container image and register it as an MCP stdio server in `mcp.json`.
pub async fn install_tool(
    tool_id: &str,
    runtime: &RuntimeInfo,
    mcp_config_path: &Path,
    mcp_cfg_lock: &tokio::sync::Mutex<()>,
) -> Result<(), String> {
    let catalog = load_catalog()?;
    let entry = catalog
        .tools
        .iter()
        .find(|t| t.id == tool_id)
        .ok_or_else(|| format!("tool '{tool_id}' not in catalog (whitelist)"))?;

    ensure_tool_image(runtime, entry).await?;

    // Verify digest (skip if catalog entry has no pinned digest).
    if !entry.digest.is_empty() {
        let inspect_output = tokio::process::Command::new(&runtime.binary)
            .args(["image", "inspect", "--format", "{{.Digest}}", &entry.image])
            .output()
            .await
            .map_err(|e| format!("failed to inspect image: {e}"))?;

        if inspect_output.status.success() {
            let actual = String::from_utf8_lossy(&inspect_output.stdout)
                .trim()
                .to_string();
            if !actual.is_empty() && actual != entry.digest {
                let _ = tokio::process::Command::new(&runtime.binary)
                    .args(["rmi", &entry.image])
                    .output()
                    .await;
                return Err(format!(
                    "digest mismatch: expected {}, got {actual}",
                    entry.digest
                ));
            }
        }
    }

    let _cfg_guard = mcp_cfg_lock.lock().await;
    let mut cfg = mcp_service::load_or_init_config(mcp_config_path)?;
    let host_paths = mcp_service::filesystem_allowed_paths(&cfg);
    let args = podman_run_argv_for_tool(entry, &host_paths)?;

    let server_entry = ServerEntry::Stdio {
        command: runtime.binary.clone(),
        args,
        env: HashMap::new(),
        direct_return: entry.direct_return,
    };

    cfg.servers.insert(server_key(tool_id), server_entry);
    mcp_service::save_config(mcp_config_path, &cfg)?;

    Ok(())
}

/// Rewrite every **installed** catalog tool that uses `mount_workspace` so argv matches `host_paths`
/// (empty list → in-image stub root only). Returns whether `mcp.json` should be saved.
pub fn sync_workspace_mounted_tools_if_installed(
    cfg: &mut McpConfig,
    host_paths: &[String],
) -> Result<bool, String> {
    let catalog = load_catalog()?;
    let mut changed = false;
    for entry in catalog.tools.iter().filter(|t| t.mount_workspace) {
        let key = server_key(&entry.id);
        let Some(ServerEntry::Stdio {
            command,
            args,
            env,
            direct_return,
        }) = cfg.servers.get(&key)
        else {
            continue;
        };

        let new_args = podman_run_argv_for_tool(entry, host_paths)?;
        if args == &new_args {
            continue;
        }

        let new_entry = ServerEntry::Stdio {
            command: command.clone(),
            args: new_args,
            env: env.clone(),
            direct_return: *direct_return,
        };
        cfg.servers.insert(key, new_entry);
        changed = true;
    }
    Ok(changed)
}

/// Remove an MCP stdio server entry from `mcp.json` and remove the container image.
pub async fn uninstall_tool(
    tool_id: &str,
    runtime: &RuntimeInfo,
    mcp_config_path: &Path,
    mcp_cfg_lock: &tokio::sync::Mutex<()>,
) -> Result<(), String> {
    // Remove from mcp.json.
    let key = server_key(tool_id);
    if mcp_config_path.exists() {
        let _cfg_guard = mcp_cfg_lock.lock().await;
        let mut cfg = mcp_service::read_config(mcp_config_path)?;
        cfg.servers.remove(&key);
        mcp_service::save_config(mcp_config_path, &cfg)?;
    }

    // Remove the container image.
    let catalog = load_catalog()?;
    if let Some(entry) = catalog.tools.iter().find(|t| t.id == tool_id) {
        let _ = tokio::process::Command::new(&runtime.binary)
            .args(["rmi", &entry.image])
            .output()
            .await;
    }

    Ok(())
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
    fn podman_argv_with_paths_emits_ro_binds_and_roots() {
        let catalog = load_catalog().unwrap();
        let entry = catalog
            .tools
            .iter()
            .find(|t| t.id == "pengine/file-manager")
            .unwrap();
        let hosts = vec!["/Users/x/pengine".into(), "/opt/other".into()];
        let argv = podman_run_argv_for_tool(entry, &hosts).unwrap();
        let suffix = if entry.mount_read_only { "ro" } else { "rw" };
        assert!(argv
            .iter()
            .any(|a| a == &format!("-v=/Users/x/pengine:/app/pengine:{suffix}")));
        assert!(argv
            .iter()
            .any(|a| a == &format!("-v=/opt/other:/app/other:{suffix}")));
        // Roots are appended after the image + mcp_server_cmd.
        assert_eq!(
            &argv[argv.len() - 2..],
            &["/app/pengine".to_string(), "/app/other".to_string()]
        );
    }

    #[test]
    fn duplicate_basenames_get_suffix() {
        let hosts = vec!["/a/foo".into(), "/b/foo".into()];
        let pairs = workspace_app_bind_pairs(&hosts);
        assert_eq!(pairs[0].1, "/app/foo");
        assert_eq!(pairs[1].1, "/app/foo_1");
    }

    #[test]
    fn podman_argv_empty_paths_uses_tmp_root() {
        let catalog = load_catalog().unwrap();
        let entry = catalog
            .tools
            .iter()
            .find(|t| t.id == "pengine/file-manager")
            .unwrap();
        let argv = podman_run_argv_for_tool(entry, &[]).unwrap();
        assert!(
            !argv.iter().any(|a| a.starts_with("-v=")),
            "no bind mounts until folders are set"
        );
        assert_eq!(
            argv.last().map(String::as_str),
            Some(EMPTY_WORKSPACE_CONTAINER_ROOT)
        );
    }
}
