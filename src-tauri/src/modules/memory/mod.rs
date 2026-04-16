//! Generic, backend-agnostic memory capability.
//!
//! The agent talks to `MemoryProvider` without knowing which MCP server is behind it.
//! Detection is by **tool shape** (what commands the server exposes), not by catalog id,
//! so any memory MCP server that speaks a known shape is picked up automatically.
//!
//! ## Session policy lives here, not in the agent
//!
//! Keyword phrases ([`SESSION_START_PHRASES`], [`SESSION_END_PHRASES`], [`DIARY_START_PHRASES`],
//! [`DIARY_END_PHRASES`]) and the `session-<timestamp>` naming are defined at the top of this
//! module. Swapping the backing MCP server never touches them.
//!
//! ## Adding a new memory backend
//!
//! 1. Add a [`Backend`] variant.
//! 2. Add a detection arm in [`MemoryProvider::detect`] (match on the new tool shape).
//! 3. Add match arms in [`MemoryProvider::start_session`] / [`MemoryProvider::append`]
//!    translating the generic op into that backend's tool calls.
//!
//! The agent, session keywords, and entity-naming scheme do not change.

use crate::modules::mcp::registry::{Provider, ToolRegistry};
use chrono::{DateTime, Utc};
use serde_json::json;
use std::collections::HashSet;

/// Exact (case-insensitive, trimmed, trailing punctuation stripped) phrases that open a
/// recording session. Stable across backends.
pub const SESSION_START_PHRASES: &[&str] = &[
    "remember this session",
    "save this session",
    // Star Trek flavor — starting the captain's log opens the session.
    "captain's log",
    "captains log",
    "begin log",
];

/// Phrases that end the recording session (full chat log or diary — host always closes).
pub const SESSION_END_PHRASES: &[&str] = &[
    "close session",
    "leave session",
    "over and out",
    "quit",
    "exit",
    "end log",
];

/// Start **diary-only** recording: user lines only, no assistant reply (exact message `record`).
pub const DIARY_START_PHRASES: &[&str] = &["record"];

/// Stop diary-only recording without necessarily using a full session end phrase.
pub const DIARY_END_PHRASES: &[&str] = &["record end"];

/// Starfleet sign-off: `<rank> <name...> out`. Matches e.g. `Commander Worf out`,
/// `Captain Picard out`. Rank must be present so casual phrases like "logging out"
/// never trigger.
fn is_starfleet_signoff(normalized: &str) -> bool {
    let toks: Vec<&str> = normalized.split_whitespace().collect();
    if toks.len() < 3 {
        return false;
    }
    let first = toks.first().copied().unwrap_or("");
    let last = toks.last().copied().unwrap_or("");
    matches!(first, "commander" | "captain") && last == "out"
}

/// Abstract command a user message can request of the memory subsystem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCommand {
    /// Captain's log / remember this session — saves user + assistant each turn.
    Start,
    /// Close any active memory session (including diary). `over and out` maps here.
    End,
    /// Diary mode — only user messages persisted, no Telegram reply on each line.
    DiaryStart,
    /// Leave diary mode (must be in diary session).
    DiaryEnd,
}

/// Match a user message against the session keyword lists. Case-insensitive,
/// whitespace-trimmed, trailing `.!?,;` stripped. Only exact matches count — casual
/// substrings like "I want to quit my job" do **not** end the session.
pub fn detect_session_command(msg: &str) -> Option<SessionCommand> {
    let normalized = msg
        .trim()
        .trim_end_matches(['.', '!', '?', ',', ';'])
        .to_ascii_lowercase();
    // More specific phrases first (`record end` vs `record`).
    if DIARY_END_PHRASES.iter().any(|p| normalized == *p) {
        return Some(SessionCommand::DiaryEnd);
    }
    if DIARY_START_PHRASES.iter().any(|p| normalized == *p) {
        return Some(SessionCommand::DiaryStart);
    }
    if SESSION_START_PHRASES.iter().any(|p| normalized == *p) {
        return Some(SessionCommand::Start);
    }
    if SESSION_END_PHRASES.iter().any(|p| normalized == *p) {
        return Some(SessionCommand::End);
    }
    if is_starfleet_signoff(&normalized) {
        return Some(SessionCommand::End);
    }
    None
}

/// Build a session entity name: `session-YYYYMMDDThhmmssZ`. Deterministic format so
/// humans can sort sessions chronologically in the knowledge graph.
pub fn session_entity_name(at: DateTime<Utc>) -> String {
    format!("session-{}", at.format("%Y%m%dT%H%M%SZ"))
}

/// Which MCP tool shape backs this provider. One variant per supported memory style.
/// The agent never inspects this — it's private to the provider impl.
enum Backend {
    /// Official `@modelcontextprotocol/server-memory` shape: entities + observations.
    KnowledgeGraph,
}

/// Memory capability bound to a concrete MCP provider in the registry.
///
/// Each generic op (`start_session`, `append`) dispatches on the backend to the right
/// tool names — so the agent stays decoupled from any specific MCP server.
pub struct MemoryProvider {
    backend: Backend,
    provider: Provider,
}

impl MemoryProvider {
    /// Find a memory-capable server in the registry. Picks the first match. Returns
    /// `None` if no connected MCP server exposes a known memory tool shape.
    pub fn detect(reg: &ToolRegistry) -> Option<Self> {
        for p in reg.providers() {
            let tools: HashSet<&str> = p.tools().iter().map(|t| t.name.as_str()).collect();
            if tools.contains("create_entities") && tools.contains("add_observations") {
                return Some(Self {
                    backend: Backend::KnowledgeGraph,
                    provider: p.clone(),
                });
            }
        }
        None
    }

    /// MCP server key hosting this memory (e.g. `te_pengine-memory`). Useful for logs.
    pub fn server_name(&self) -> &str {
        self.provider.server_name()
    }

    /// Create the root session entity. `description` becomes the first observation.
    pub async fn start_session(&self, name: &str, description: &str) -> Result<(), String> {
        match self.backend {
            Backend::KnowledgeGraph => {
                let args = json!({
                    "entities": [{
                        "name": name,
                        "entityType": "ChatSession",
                        "observations": [description],
                    }]
                });
                self.provider.call_tool("create_entities", args).await?;
            }
        }
        Ok(())
    }

    /// Append a free-form observation to an existing session entity.
    pub async fn append(&self, entity_name: &str, content: &str) -> Result<(), String> {
        match self.backend {
            Backend::KnowledgeGraph => {
                let args = json!({
                    "observations": [{
                        "entityName": entity_name,
                        "contents": [content],
                    }]
                });
                self.provider.call_tool("add_observations", args).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_phrases_match_exactly_ignoring_case_and_punctuation() {
        for p in SESSION_START_PHRASES {
            assert_eq!(detect_session_command(p), Some(SessionCommand::Start));
            assert_eq!(
                detect_session_command(&p.to_uppercase()),
                Some(SessionCommand::Start)
            );
            assert_eq!(
                detect_session_command(&format!("  {p}.")),
                Some(SessionCommand::Start)
            );
        }
    }

    #[test]
    fn end_phrases_match_exactly() {
        for p in SESSION_END_PHRASES {
            assert_eq!(detect_session_command(p), Some(SessionCommand::End));
        }
    }

    #[test]
    fn diary_phrases() {
        assert_eq!(
            detect_session_command("record"),
            Some(SessionCommand::DiaryStart)
        );
        assert_eq!(
            detect_session_command("record end"),
            Some(SessionCommand::DiaryEnd)
        );
        assert_eq!(
            detect_session_command("record."),
            Some(SessionCommand::DiaryStart)
        );
    }

    /// Casual use of single-word end phrases must not trigger a session close — keywords
    /// are full-message-only to avoid accidental terminations.
    #[test]
    fn casual_mentions_do_not_trigger() {
        assert_eq!(detect_session_command("I want to quit my job"), None);
        assert_eq!(detect_session_command("exit the building safely"), None);
        assert_eq!(
            detect_session_command("please remember this session later"),
            None
        );
        // "logging out" should not fire the Starfleet matcher — it requires a rank prefix.
        assert_eq!(detect_session_command("logging out"), None);
        assert_eq!(detect_session_command("I need to head out"), None);
    }

    #[test]
    fn starfleet_signoff_closes_session() {
        assert_eq!(
            detect_session_command("Commander Worf out"),
            Some(SessionCommand::End)
        );
        assert_eq!(
            detect_session_command("Captain Picard out."),
            Some(SessionCommand::End)
        );
        assert_eq!(
            detect_session_command("commander data out"),
            Some(SessionCommand::End)
        );
        // No rank — don't fire.
        assert_eq!(detect_session_command("Kirk out"), None);
    }

    #[test]
    fn captains_log_opens_session() {
        assert_eq!(
            detect_session_command("Captain's Log"),
            Some(SessionCommand::Start)
        );
        assert_eq!(
            detect_session_command("captains log"),
            Some(SessionCommand::Start)
        );
    }

    #[test]
    fn session_entity_name_is_sortable() {
        let a = session_entity_name(
            chrono::DateTime::parse_from_rfc3339("2026-04-16T10:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );
        let b = session_entity_name(
            chrono::DateTime::parse_from_rfc3339("2026-04-16T11:30:00Z")
                .unwrap()
                .with_timezone(&Utc),
        );
        assert!(a < b);
        assert!(a.starts_with("session-"));
    }
}
