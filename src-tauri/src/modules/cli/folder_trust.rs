use crate::modules::mcp::service as mcp_service;
use crate::shared::state::AppState;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

const TRUST_FILE: &str = "folder_trust.json";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct FolderTrust {
    #[serde(default)]
    pub trusted: Vec<PathBuf>,
    #[serde(default)]
    pub denied: Vec<PathBuf>,
}

impl FolderTrust {
    pub fn is_decided(&self, path: &Path) -> bool {
        self.trusted.iter().any(|p| p == path) || self.denied.iter().any(|p| p == path)
    }

    pub fn is_under_trusted(&self, path: &Path) -> bool {
        self.trusted.iter().any(|t| path.starts_with(t))
    }
}

fn trust_path(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .map(|p| p.join(TRUST_FILE))
        .unwrap_or_else(|| PathBuf::from(TRUST_FILE))
}

pub fn load(store_path: &Path) -> FolderTrust {
    let path = trust_path(store_path);
    let body = match fs::read_to_string(&path) {
        Ok(b) => b,
        Err(_) => return FolderTrust::default(),
    };
    serde_json::from_str(&body).unwrap_or_default()
}

pub fn save(store_path: &Path, trust: &FolderTrust) -> Result<(), String> {
    let path = trust_path(store_path);
    let body = serde_json::to_string_pretty(trust).map_err(|e| format!("encode: {e}"))?;
    fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptDecision {
    Yes,
    No,
    Skip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptOutcome {
    Added,
    Declined,
    AlreadyCovered,
    NoTty,
    NotPrompted,
}

fn ask(prompt: &str) -> PromptDecision {
    if !std::io::stdin().is_terminal() {
        return PromptDecision::Skip;
    }
    {
        let mut out = std::io::stdout().lock();
        let _ = out.write_all(prompt.as_bytes());
        let _ = out.flush();
    }
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).is_err() {
        return PromptDecision::Skip;
    }
    parse_answer(&line)
}

fn parse_answer(line: &str) -> PromptDecision {
    match line.trim().to_lowercase().as_str() {
        "y" | "yes" => PromptDecision::Yes,
        "n" | "no" => PromptDecision::No,
        _ => PromptDecision::Skip,
    }
}

pub async fn maybe_prompt_for_cwd(state: &AppState, cwd: &Path) -> Result<PromptOutcome, String> {
    let cwd = match fs::canonicalize(cwd) {
        Ok(p) => p,
        Err(_) => return Ok(PromptOutcome::NotPrompted),
    };
    if cwd.parent().is_none() {
        return Ok(PromptOutcome::NotPrompted);
    }

    let mut trust = load(&state.store_path);
    if trust.is_decided(&cwd) || trust.is_under_trusted(&cwd) {
        return Ok(PromptOutcome::NotPrompted);
    }

    if cwd_already_covered_by_mcp(state, &cwd)? {
        // Treat as implicit trust so we don't ask again next launch.
        if !trust.trusted.iter().any(|p| p == &cwd) {
            trust.trusted.push(cwd);
            let _ = save(&state.store_path, &trust);
        }
        return Ok(PromptOutcome::AlreadyCovered);
    }

    if !std::io::stdin().is_terminal() {
        return Ok(PromptOutcome::NoTty);
    }

    let prompt = format!(
        "\n  ⎿  {}\n     Add this folder to Pengine's MCP filesystem roots? [y/n] ",
        cwd.display()
    );
    match ask(&prompt) {
        PromptDecision::Yes => {
            add_to_mcp(state, &cwd).await?;
            trust.trusted.push(cwd);
            save(&state.store_path, &trust)?;
            Ok(PromptOutcome::Added)
        }
        PromptDecision::No => {
            trust.denied.push(cwd);
            save(&state.store_path, &trust)?;
            Ok(PromptOutcome::Declined)
        }
        PromptDecision::Skip => Ok(PromptOutcome::NotPrompted),
    }
}

fn cwd_already_covered_by_mcp(state: &AppState, cwd: &Path) -> Result<bool, String> {
    let cfg = mcp_service::load_or_init_config(&state.mcp_config_path)
        .map_err(|e| format!("load mcp config: {e}"))?;
    let existing = mcp_service::filesystem_allowed_paths(&cfg);
    Ok(existing.iter().any(|p| {
        let pb = PathBuf::from(p);
        let canon = fs::canonicalize(&pb).unwrap_or(pb);
        cwd.starts_with(&canon)
    }))
}

async fn add_to_mcp(state: &AppState, cwd: &Path) -> Result<(), String> {
    let _guard = state.mcp_config_mutex.lock().await;
    let mut cfg = mcp_service::load_or_init_config(&state.mcp_config_path)
        .map_err(|e| format!("load mcp config: {e}"))?;
    let mut paths = mcp_service::filesystem_allowed_paths(&cfg);
    let cwd_str = cwd.display().to_string();
    if !paths.iter().any(|p| p == &cwd_str) {
        paths.push(cwd_str);
        mcp_service::set_filesystem_allowed_paths(&mut cfg, &paths);
        mcp_service::save_config(&state.mcp_config_path, &cfg)
            .map_err(|e| format!("save mcp config: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parse_answer_accepts_yes_no() {
        assert_eq!(parse_answer("y\n"), PromptDecision::Yes);
        assert_eq!(parse_answer("Yes"), PromptDecision::Yes);
        assert_eq!(parse_answer("YES"), PromptDecision::Yes);
        assert_eq!(parse_answer("n"), PromptDecision::No);
        assert_eq!(parse_answer("No"), PromptDecision::No);
        assert_eq!(parse_answer(""), PromptDecision::Skip);
        assert_eq!(parse_answer("maybe"), PromptDecision::Skip);
    }

    #[test]
    fn round_trip_save_and_load() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("connection.json");
        fs::write(&store, "{}").unwrap();
        let mut trust = FolderTrust::default();
        trust.trusted.push(PathBuf::from("/a"));
        trust.denied.push(PathBuf::from("/b"));
        save(&store, &trust).unwrap();
        let loaded = load(&store);
        assert_eq!(loaded.trusted, vec![PathBuf::from("/a")]);
        assert_eq!(loaded.denied, vec![PathBuf::from("/b")]);
    }

    #[test]
    fn is_decided_matches_exact_paths() {
        let mut t = FolderTrust::default();
        t.trusted.push(PathBuf::from("/a"));
        t.denied.push(PathBuf::from("/b"));
        assert!(t.is_decided(Path::new("/a")));
        assert!(t.is_decided(Path::new("/b")));
        assert!(!t.is_decided(Path::new("/c")));
    }

    #[test]
    fn is_under_trusted_walks_subtree() {
        let mut t = FolderTrust::default();
        t.trusted.push(PathBuf::from("/work"));
        assert!(t.is_under_trusted(Path::new("/work")));
        assert!(t.is_under_trusted(Path::new("/work/src")));
        assert!(!t.is_under_trusted(Path::new("/elsewhere")));
    }

    #[test]
    fn load_returns_default_when_file_missing() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("connection.json");
        let trust = load(&store);
        assert!(trust.trusted.is_empty());
        assert!(trust.denied.is_empty());
    }
}
