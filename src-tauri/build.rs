fn main() {
    let manifest_dir = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"),
    );

    let app_version = package_json_version(&manifest_dir);
    println!("cargo:rustc-env=PENGINE_APP_VERSION={app_version}");

    let commit = git_head_commit(&manifest_dir);
    println!("cargo:rustc-env=PENGINE_GIT_COMMIT={commit}");

    let package_json = manifest_dir.join("../package.json");
    if let Ok(canonical) = package_json.canonicalize() {
        println!("cargo:rerun-if-changed={}", canonical.display());
    }

    let git_head = manifest_dir.join("../.git/HEAD");
    if let Ok(canonical) = git_head.canonicalize() {
        println!("cargo:rerun-if-changed={}", canonical.display());
    }

    tauri_build::build();
}

/// User-facing release version: root `package.json` `"version"` (same as Vite/npm).
fn package_json_version(manifest_dir: &std::path::Path) -> String {
    let path = manifest_dir
        .parent()
        .unwrap_or(manifest_dir)
        .join("package.json");
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into()),
    };
    for line in raw.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("\"version\"") {
            let rest = rest.trim_start();
            let rest = match rest.strip_prefix(':') {
                Some(r) => r.trim_start(),
                None => continue,
            };
            let rest = match rest.strip_prefix('"') {
                Some(r) => r,
                None => continue,
            };
            if let Some(end) = rest.find('"') {
                let v = rest[..end].trim();
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
    }
    std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into())
}

/// Resolved `HEAD` at build time (tip of the checked-out branch).
fn git_head_commit(manifest_dir: &std::path::Path) -> String {
    let repo_root = manifest_dir.parent().unwrap_or(manifest_dir);
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_root)
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}
