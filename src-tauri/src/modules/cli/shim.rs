//! Install a **`pengine-cli`** entry on the user PATH (no admin on macOS/Linux when using `~/.local/bin`).
//!
//! The dashboard writes a small launcher script that sets `PENGINE_LAUNCH_MODE=cli`
//! and `exec`s the real app binary. That keeps terminal use aligned with `bun run cli`
//! (REPL + one-shots) and avoids falling through to the GUI when stdin is not a TTY.

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliShimStatus {
    pub shim_path: String,
    pub installed: bool,
    /// App binary path embedded in the launcher (for display / debugging).
    pub resolves_to: Option<String>,
    /// Whether a typical `PATH` already includes the launcher directory.
    pub local_bin_on_path: bool,
    /// One-line shell hint if `local_bin_on_path` is false.
    pub path_export_hint: String,
}

fn path_env_contains_dir(dir: &Path) -> bool {
    let Ok(path_var) = std::env::var("PATH") else {
        return false;
    };
    let dir_norm = normalize_path_for_compare(dir);
    path_var
        .split(if cfg!(windows) { ';' } else { ':' })
        .any(|entry| {
            let p = Path::new(entry.trim());
            !p.as_os_str().is_empty() && normalize_path_for_compare(p) == dir_norm
        })
}

fn normalize_path_for_compare(p: &Path) -> PathBuf {
    let mut b = p.to_path_buf();
    while b.as_os_str().len() > 1 && b.file_name().is_none() {
        b.pop();
    }
    b
}

pub fn shim_path() -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        let home =
            std::env::var("HOME").map_err(|_| "HOME is not set; cannot install pengine-cli")?;
        Ok(PathBuf::from(home).join(".local/bin/pengine-cli"))
    }
    #[cfg(windows)]
    {
        let base = std::env::var("LOCALAPPDATA")
            .map_err(|_| "LOCALAPPDATA is not set; cannot install pengine-cli")?;
        Ok(PathBuf::from(base)
            .join("Pengine")
            .join("bin")
            .join("pengine-cli.cmd"))
    }
}

fn shim_parent() -> Result<PathBuf, String> {
    shim_path().and_then(|p| {
        p.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| "launcher path has no parent directory".to_string())
    })
}

fn path_export_hint(shim_dir: &Path) -> String {
    #[cfg(unix)]
    {
        format!(
            "export PATH=\"{}:$PATH\" · add once to ~/.zshrc or ~/.bashrc",
            shim_dir.display()
        )
    }
    #[cfg(windows)]
    {
        format!("Add to user PATH: {}", shim_dir.display())
    }
}

/// Shell-safe single-quoted string for `/bin/sh` `exec … "$@"`.
fn sh_single_quoted(path: &str) -> String {
    let mut out = String::from("'");
    for ch in path.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

fn parse_unix_launcher_target(body: &str) -> Option<String> {
    body.lines().find_map(|l| {
        l.strip_prefix("# pengine-exe:")
            .map(|s| s.trim().to_string())
    })
}

/// Inspect launcher file and PATH.
pub fn status() -> Result<CliShimStatus, String> {
    let shim = shim_path()?;
    let shim_dir = shim_parent()?;
    let local_bin_on_path = path_env_contains_dir(&shim_dir);

    let meta = fs::symlink_metadata(&shim).ok();
    let installed = meta.is_some();
    let resolves_to = if let Some(m) = &meta {
        if m.file_type().is_symlink() {
            fs::read_link(&shim).ok().map(|p| p.display().to_string())
        } else {
            let body = fs::read_to_string(&shim).unwrap_or_default();
            #[cfg(windows)]
            {
                let line = body
                    .lines()
                    .find(|l| l.trim_start().starts_with("set \"PENGINE_EXE="));
                line.and_then(|l| {
                    l.trim()
                        .strip_prefix("set \"PENGINE_EXE=")
                        .and_then(|s| s.strip_suffix('"'))
                        .map(str::to_string)
                })
            }
            #[cfg(not(windows))]
            {
                parse_unix_launcher_target(&body)
            }
        }
    } else {
        None
    };

    Ok(CliShimStatus {
        shim_path: shim.display().to_string(),
        installed,
        resolves_to,
        local_bin_on_path,
        path_export_hint: path_export_hint(&shim_dir),
    })
}

/// Create `~/.local/bin/pengine-cli` (Unix shell script) or `%LOCALAPPDATA%\Pengine\bin\pengine-cli.cmd` (Windows).
pub fn install_shim() -> Result<CliShimStatus, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let exe_display = exe.to_string_lossy();
    let shim = shim_path()?;
    let parent = shim_parent()?;
    fs::create_dir_all(&parent).map_err(|e| format!("create_dir_all {}: {e}", parent.display()))?;

    if fs::symlink_metadata(&shim).is_ok() {
        fs::remove_file(&shim).map_err(|e| format!("remove old launcher: {e}"))?;
    }

    #[cfg(unix)]
    {
        let body = format!(
            "#!/bin/sh\n# pengine-exe:{}\nexport PENGINE_LAUNCH_MODE=cli\nexec {} \"$@\"\n",
            exe_display,
            sh_single_quoted(&exe_display)
        );
        fs::write(&shim, body).map_err(|e| format!("write {}: {e}", shim.display()))?;
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&shim)
            .map_err(|e| format!("metadata {}: {e}", shim.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&shim, perms).map_err(|e| format!("chmod {}: {e}", shim.display()))?;
    }
    #[cfg(windows)]
    {
        write_windows_cmd_launcher(&shim, &exe)?;
    }

    status()
}

#[cfg(windows)]
fn write_windows_cmd_launcher(shim: &Path, exe: &Path) -> Result<(), String> {
    let exe_str = exe.to_str().ok_or("exe path is not valid UTF-8")?;
    let body = format!(
        "@echo off\r\nset PENGINE_LAUNCH_MODE=cli\r\nset \"PENGINE_EXE={exe_str}\"\r\n\"%PENGINE_EXE%\" %*\r\n"
    );
    fs::write(shim, body).map_err(|e| format!("write {}: {e}", shim.display()))
}

/// Remove the launcher file only (never deletes the real app binary).
pub fn remove_shim() -> Result<(), String> {
    let shim = shim_path()?;
    if fs::symlink_metadata(&shim).is_ok() {
        fs::remove_file(&shim).map_err(|e| format!("remove {}: {e}", shim.display()))?;
    }
    Ok(())
}
