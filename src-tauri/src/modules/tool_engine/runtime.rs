use super::types::RuntimeKind;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    pub kind: RuntimeKind,
    pub binary: String,
    pub version: String,
    pub rootless: bool,
}

/// Detect a container runtime. Prefers Podman (rootless by default), falls back to Docker.
///
/// GUI apps on macOS often inherit a minimal `PATH` (no Homebrew), so we probe well-known
/// install locations in addition to the bare executable name.
pub async fn detect_runtime() -> Option<RuntimeInfo> {
    if let Some(info) = try_runtime("podman", RuntimeKind::Podman).await {
        return Some(info);
    }
    try_runtime("docker", RuntimeKind::Docker).await
}

fn push_candidate(out: &mut Vec<PathBuf>, p: PathBuf) {
    if p.as_os_str().is_empty() {
        return;
    }
    if !out.iter().any(|x| x == &p) {
        out.push(p);
    }
}

/// Ordered list of paths to try for `podman` / `docker`.
fn runtime_binary_candidates(name: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    push_candidate(&mut out, PathBuf::from(name));

    if let Ok(path_var) = std::env::var("PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for dir in path_var.split(sep) {
            if dir.is_empty() {
                continue;
            }
            push_candidate(&mut out, Path::new(dir).join(name));
        }
    }

    #[cfg(target_os = "macos")]
    {
        push_candidate(&mut out, PathBuf::from(format!("/opt/homebrew/bin/{name}")));
        push_candidate(&mut out, PathBuf::from(format!("/usr/local/bin/{name}")));
        push_candidate(&mut out, PathBuf::from(format!("/opt/podman/bin/{name}")));
    }

    #[cfg(target_os = "linux")]
    {
        push_candidate(&mut out, PathBuf::from(format!("/usr/bin/{name}")));
        push_candidate(&mut out, PathBuf::from(format!("/bin/{name}")));
    }

    if let Ok(home) = std::env::var("HOME") {
        push_candidate(&mut out, Path::new(&home).join(".local/bin").join(name));
    }

    out
}

async fn try_runtime(binary_name: &str, kind: RuntimeKind) -> Option<RuntimeInfo> {
    for path in runtime_binary_candidates(binary_name) {
        if let Some(info) = try_runtime_at(&path, kind).await {
            return Some(info);
        }
    }
    None
}

async fn try_runtime_at(path: &Path, kind: RuntimeKind) -> Option<RuntimeInfo> {
    let output = tokio::process::Command::new(path)
        .args(["version", "--format", "{{.Client.Version}}"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if version.is_empty() {
        return None;
    }

    let binary = path.to_string_lossy().into_owned();

    let rootless = match kind {
        RuntimeKind::Podman => true,
        RuntimeKind::Docker => check_docker_rootless(path).await,
    };

    Some(RuntimeInfo {
        kind,
        binary,
        version,
        rootless,
    })
}

async fn check_docker_rootless(binary: &Path) -> bool {
    let output = tokio::process::Command::new(binary)
        .args(["info", "--format", "{{.SecurityOptions}}"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok();

    output
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("rootless"))
        .unwrap_or(false)
}
