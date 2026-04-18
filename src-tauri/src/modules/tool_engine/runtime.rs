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
    // Use `--version` instead of `version --format=…`: `podman version` talks to the machine
    // socket and fails with "Cannot connect to Podman" when the VM is stopped, even though the
    // CLI is installed. `docker version` can also exit non-zero when the daemon is down while
    // still printing the client version. `--version` is client-only and succeeds if the binary exists.
    let output = tokio::process::Command::new(path)
        .arg("--version")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .await
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let version = parse_dash_version(kind, &raw)?.to_string();
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

/// Parse `podman --version` / `docker --version` stdout into a semver-ish version token.
fn parse_dash_version(kind: RuntimeKind, stdout: &str) -> Option<&str> {
    let line = stdout.lines().next()?.trim();
    let mut parts = line.split_whitespace();
    match kind {
        RuntimeKind::Podman => {
            if parts.next()? != "podman" {
                return None;
            }
            if parts.next()? != "version" {
                return None;
            }
            parts.next()
        }
        RuntimeKind::Docker => {
            if !parts.next()?.eq_ignore_ascii_case("docker") {
                return None;
            }
            if !parts.next()?.eq_ignore_ascii_case("version") {
                return None;
            }
            let ver = parts.next()?;
            Some(ver.trim_end_matches(','))
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dash_version_podman() {
        assert_eq!(
            parse_dash_version(RuntimeKind::Podman, "podman version 5.8.1\n"),
            Some("5.8.1")
        );
        assert_eq!(
            parse_dash_version(RuntimeKind::Podman, "Docker version 29.0.0\n"),
            None
        );
    }

    #[test]
    fn parse_dash_version_docker() {
        assert_eq!(
            parse_dash_version(
                RuntimeKind::Docker,
                "Docker version 29.3.1, build c2be9ccfc3\n"
            ),
            Some("29.3.1")
        );
        assert_eq!(
            parse_dash_version(
                RuntimeKind::Docker,
                "docker version 24.0.6, build ed223bc\n"
            ),
            Some("24.0.6")
        );
    }
}
