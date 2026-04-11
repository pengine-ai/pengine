use super::types::RuntimeKind;
use crate::infrastructure::executable_resolve;
use serde::Serialize;
use std::path::Path;

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

async fn try_runtime(binary_name: &str, kind: RuntimeKind) -> Option<RuntimeInfo> {
    for path in executable_resolve::runtime_binary_candidates(binary_name) {
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
