//! CLI/REPL session — minimal turn history + persistence for `/compact`,
//! `/resume`, `/cost`, and `--continue`.
//!
//! Pengine's `agent::run_turn` is single-shot. To give the REPL a
//! Claude-Code-like continuity we keep a session here and prepend prior
//! context to each new user message before handing it to the agent.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const SESSIONS_DIRNAME: &str = "cli_sessions";
const LAST_POINTER: &str = "cli_session_last.json";

/// Cap applied when building the context prefix for a new turn.
/// Keeps the prompt size predictable across long sessions.
const HISTORY_TURN_BUDGET: usize = 6;
const HISTORY_BYTES_BUDGET: usize = 12_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTurn {
    pub at: DateTime<Utc>,
    pub user: String,
    pub assistant: String,
    pub prompt_tokens: u64,
    pub eval_tokens: u64,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliSession {
    pub id: String,
    pub started_at: DateTime<Utc>,
    pub turns: Vec<SessionTurn>,
    /// Set by `/compact`. When present, replaces the older turns when
    /// building the context prefix.
    pub summary: Option<String>,
    pub prompt_tokens_total: u64,
    pub eval_tokens_total: u64,
}

impl CliSession {
    pub fn fresh() -> Self {
        let now = Utc::now();
        Self {
            id: now.format("%Y%m%dT%H%M%S").to_string(),
            started_at: now,
            turns: Vec::new(),
            summary: None,
            prompt_tokens_total: 0,
            eval_tokens_total: 0,
        }
    }

    pub fn record_turn(
        &mut self,
        user: &str,
        assistant: &str,
        prompt_tokens: u64,
        eval_tokens: u64,
        model: &str,
    ) {
        self.turns.push(SessionTurn {
            at: Utc::now(),
            user: user.to_string(),
            assistant: assistant.to_string(),
            prompt_tokens,
            eval_tokens,
            model: model.to_string(),
        });
        self.prompt_tokens_total = self.prompt_tokens_total.saturating_add(prompt_tokens);
        self.eval_tokens_total = self.eval_tokens_total.saturating_add(eval_tokens);
    }

    /// Build the prior-context prefix that gets prepended to a fresh user
    /// message. Empty when the session is empty.
    pub fn context_prefix(&self) -> String {
        let mut out = String::new();
        if let Some(s) = self.summary.as_deref() {
            if !s.trim().is_empty() {
                out.push_str("## Prior session summary\n");
                out.push_str(s.trim());
                out.push_str("\n\n");
            }
        }
        let take_from = self.turns.len().saturating_sub(HISTORY_TURN_BUDGET);
        let mut bytes_used = 0usize;
        let mut pieces: Vec<String> = Vec::new();
        for t in &self.turns[take_from..] {
            let piece = format!(
                "[user] {}\n[assistant] {}\n",
                t.user.trim(),
                t.assistant.trim()
            );
            bytes_used = bytes_used.saturating_add(piece.len());
            if bytes_used > HISTORY_BYTES_BUDGET && !pieces.is_empty() {
                break;
            }
            pieces.push(piece);
        }
        if !pieces.is_empty() {
            out.push_str("## Prior turns (most recent last)\n");
            for p in &pieces {
                out.push_str(p);
            }
            out.push('\n');
        }
        out
    }
}

fn sessions_dir(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .map(|p| p.join(SESSIONS_DIRNAME))
        .unwrap_or_else(|| PathBuf::from(SESSIONS_DIRNAME))
}

fn last_pointer(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .map(|p| p.join(LAST_POINTER))
        .unwrap_or_else(|| PathBuf::from(LAST_POINTER))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LastPointer {
    last_session_id: String,
}

pub fn save(store_path: &Path, session: &CliSession) -> Result<(), String> {
    let dir = sessions_dir(store_path);
    fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join(format!("{}.json", session.id));
    let body = serde_json::to_string_pretty(session).map_err(|e| format!("encode: {e}"))?;
    fs::write(&path, body).map_err(|e| format!("write {}: {e}", path.display()))?;
    let pointer = LastPointer {
        last_session_id: session.id.clone(),
    };
    let pointer_body =
        serde_json::to_string_pretty(&pointer).map_err(|e| format!("encode pointer: {e}"))?;
    fs::write(last_pointer(store_path), pointer_body).map_err(|e| format!("write pointer: {e}"))?;
    Ok(())
}

pub fn load_last(store_path: &Path) -> Result<Option<CliSession>, String> {
    let pointer_path = last_pointer(store_path);
    let body = match fs::read_to_string(&pointer_path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read pointer: {e}")),
    };
    let p: LastPointer = serde_json::from_str(&body).map_err(|e| format!("parse pointer: {e}"))?;
    let dir = sessions_dir(store_path);
    let path = dir.join(format!("{}.json", p.last_session_id));
    let body = match fs::read_to_string(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("read {}: {e}", path.display())),
    };
    let s: CliSession = serde_json::from_str(&body).map_err(|e| format!("parse session: {e}"))?;
    Ok(Some(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn round_trip_save_and_load_last() {
        let dir = tempdir().unwrap();
        let store = dir.path().join("connection.json");
        fs::write(&store, "{}").unwrap();
        let mut s = CliSession::fresh();
        s.record_turn("hi", "hello", 10, 5, "qwen3:0.5b");
        save(&store, &s).unwrap();
        let loaded = load_last(&store).unwrap().unwrap();
        assert_eq!(loaded.id, s.id);
        assert_eq!(loaded.turns.len(), 1);
        assert_eq!(loaded.prompt_tokens_total, 10);
        assert_eq!(loaded.eval_tokens_total, 5);
    }

    #[test]
    fn context_prefix_includes_summary_and_turns() {
        let mut s = CliSession::fresh();
        s.summary = Some("we discussed cats".into());
        s.record_turn("hello", "hi there", 5, 3, "m");
        let prefix = s.context_prefix();
        assert!(prefix.contains("we discussed cats"));
        assert!(prefix.contains("[user] hello"));
        assert!(prefix.contains("[assistant] hi there"));
    }

    #[test]
    fn context_prefix_empty_for_fresh_session() {
        let s = CliSession::fresh();
        assert!(s.context_prefix().is_empty());
    }
}
