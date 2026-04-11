//! Resolve CLI executable paths for subprocess spawn.
//!
//! Tauri GUI apps on macOS (and some Linux desktop sessions) start with a minimal `PATH` that
//! omits Homebrew and other common install locations. [`resolve_command_for_spawn`] expands bare
//! names like `podman` / `docker` to an absolute path when possible so MCP stdio servers can start.

use std::path::{Path, PathBuf};

fn push_candidate(out: &mut Vec<PathBuf>, p: PathBuf) {
    if p.as_os_str().is_empty() {
        return;
    }
    if !out.iter().any(|x| x == &p) {
        out.push(p);
    }
}

/// Ordered list of paths to try for a CLI basename (e.g. `podman`, `docker`).
pub fn runtime_binary_candidates(name: &str) -> Vec<PathBuf> {
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

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    match fs::metadata(path) {
        Ok(m) => m.is_file() && m.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg(windows)]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

/// If `command` is a bare executable name, return the first candidate that exists and looks
/// runnable; otherwise return `command` unchanged (absolute paths, relative paths with dirs, etc.).
pub fn resolve_command_for_spawn(command: &str) -> PathBuf {
    let c = command.trim();
    if c.is_empty() {
        return PathBuf::from(c);
    }

    let path = Path::new(c);
    if path.is_absolute() || c.contains(std::path::MAIN_SEPARATOR) {
        return path.to_path_buf();
    }

    for candidate in runtime_binary_candidates(c) {
        if candidate.as_os_str().is_empty() {
            continue;
        }
        // Skip bare `docker` / `podman`: `Path::is_file()` would mean CWD, not PATH (GUI apps
        // often lack PATH entries, so we only want absolute / PATH-derived candidates here).
        if !candidate.is_absolute() && candidate.parent().is_none() {
            continue;
        }
        if candidate.is_file() && is_executable_file(&candidate) {
            if candidate != Path::new(c) {
                log::debug!("resolved MCP stdio command `{c}` → {}", candidate.display());
            }
            return candidate;
        }
    }

    PathBuf::from(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_keeps_absolute_paths() {
        let p = if cfg!(windows) {
            r"C:\Program Files\Docker\Docker\resources\bin\docker.exe"
        } else {
            "/opt/homebrew/bin/podman"
        };
        assert_eq!(resolve_command_for_spawn(p), PathBuf::from(p));
    }
}
