//! Spawns the real `pengine` binary to guard CLI short-circuit + exit codes.
//!
//! Run: `cargo test --manifest-path src-tauri/Cargo.toml --test cli_oneshot`
//! (or `cd src-tauri && cargo test --test cli_oneshot`).

use std::path::PathBuf;
use std::process::Command;

fn pengine_exe() -> PathBuf {
    if let Some(p) = std::env::var_os("CARGO_BIN_EXE_pengine") {
        return PathBuf::from(p);
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    #[cfg(target_os = "windows")]
    {
        dir.join("target").join("debug").join("pengine.exe")
    }
    #[cfg(not(target_os = "windows"))]
    {
        dir.join("target").join("debug").join("pengine")
    }
}

fn pengine() -> Command {
    let exe = pengine_exe();
    assert!(
        exe.exists(),
        "pengine test binary missing at {} — run `cargo build --manifest-path src-tauri/Cargo.toml` first",
        exe.display()
    );
    Command::new(exe)
}

#[test]
fn version_exits_zero_with_stdout() {
    let out = pengine()
        .arg("version")
        .output()
        .expect("spawn pengine version");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("pengine") && stdout.contains('('),
        "unexpected stdout: {stdout:?}"
    );
}

#[test]
fn help_exits_zero() {
    let out = pengine().arg("help").output().expect("spawn pengine help");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Pengine CLI"),
        "unexpected stdout: {stdout:?}"
    );
}

#[test]
fn json_status_exits_zero_when_global_json_first() {
    let out = pengine()
        .args(["--json", "status"])
        .output()
        .expect("spawn pengine --json status");
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().next().unwrap_or("");
    assert!(
        line.starts_with("{\"v\":1,") && line.contains("\"reply\""),
        "expected one JSON envelope line, got: {line:?}"
    );
}
