//! Generic, backend-agnostic memory capability.
//!
//! The agent talks to `MemoryProvider` without knowing which MCP server is behind it.
//! Detection uses **tool shape** (tool names + JSON Schema checks), or the official
//! catalog server key `te_pengine-memory` as fallback.
//!
//! ## Session policy lives here, not in the agent
//!
//! Keyword phrases and entity naming are defined in this module. Swapping the backing
//! MCP server never touches them.
//!
//! ## Adding a new memory backend
//!
//! 1. Add a [`Backend`] variant.
//! 2. Add a detection arm in [`MemoryProvider::detect`].
//! 3. Add match arms in [`MemoryProvider::start_session`] / [`MemoryProvider::append`].

use crate::modules::mcp::registry::{Provider, ToolRegistry};
use crate::modules::mcp::types::ToolDef;
use crate::shared::keywords::{normalize_exact, KeywordGroup, MatchMode};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

const SESSION_START_EN: &[&str] = &[
    "remember this session",
    "save this session",
    "captain's log",
    "captain\u{2019}s log",
    "captains log",
    "begin log",
];

const SESSION_END_EN: &[&str] = &[
    "close session",
    "leave session",
    "over and out",
    "quit",
    "exit",
    "end log",
];

const DIARY_START_EN: &[&str] = &["record"];
const DIARY_END_EN: &[&str] = &["record end"];

/// Opens a full-transcript session (user + assistant each turn).
pub const SESSION_START: KeywordGroup = KeywordGroup {
    id: "memory.session_start",
    description: "Open a full-transcript memory session.",
    mode: MatchMode::Exact,
    phrases_by_lang: &[
        ("en", SESSION_START_EN),
        ("de", &[]),
        ("fr", &[]),
        ("es", &[]),
        ("ja", &[]),
    ],
};

/// Ends any active recording (session or diary).
pub const SESSION_END: KeywordGroup = KeywordGroup {
    id: "memory.session_end",
    description: "Close any active memory recording.",
    mode: MatchMode::Exact,
    phrases_by_lang: &[
        ("en", SESSION_END_EN),
        ("de", &[]),
        ("fr", &[]),
        ("es", &[]),
        ("ja", &[]),
    ],
};

/// Starts diary-only recording (user lines only).
pub const DIARY_START: KeywordGroup = KeywordGroup {
    id: "memory.diary_start",
    description: "Start diary-only recording (user lines only).",
    mode: MatchMode::Exact,
    phrases_by_lang: &[
        ("en", DIARY_START_EN),
        ("de", &[]),
        ("fr", &[]),
        ("es", &[]),
        ("ja", &[]),
    ],
};

/// Stops diary-only recording.
pub const DIARY_END: KeywordGroup = KeywordGroup {
    id: "memory.diary_end",
    description: "Stop diary-only recording.",
    mode: MatchMode::Exact,
    phrases_by_lang: &[
        ("en", DIARY_END_EN),
        ("de", &[]),
        ("fr", &[]),
        ("es", &[]),
        ("ja", &[]),
    ],
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCommand {
    Start,
    End,
    DiaryStart,
    DiaryEnd,
}

/// `<rank> <name> out` with 3–4 tokens. Rank must be present.
fn is_starfleet_signoff(s: &str) -> bool {
    let toks: Vec<&str> = s.split_whitespace().collect();
    if !(3..=4).contains(&toks.len()) {
        return false;
    }
    matches!(toks[0], "commander" | "captain")
        && toks.last() == Some(&"out")
        && toks[1..toks.len() - 1]
            .iter()
            .all(|t| !t.is_empty() && t.chars().all(|c| c.is_alphabetic()))
}

/// Match a user message against keyword groups. Only exact (full-message) matches count.
pub fn detect_session_command(msg: &str) -> Option<SessionCommand> {
    if DIARY_END.matches(msg) {
        return Some(SessionCommand::DiaryEnd);
    }
    if DIARY_START.matches(msg) {
        return Some(SessionCommand::DiaryStart);
    }
    if SESSION_START.matches(msg) {
        return Some(SessionCommand::Start);
    }
    if SESSION_END.matches(msg) {
        return Some(SessionCommand::End);
    }
    if is_starfleet_signoff(&normalize_exact(msg)) {
        return Some(SessionCommand::End);
    }
    None
}

/// Sortable entity name: `<prefix>-YYYYMMDDThhmmssZ`.
pub fn entity_name(prefix: &str, at: DateTime<Utc>) -> String {
    format!("{prefix}-{}", at.format("%Y%m%dT%H%M%SZ"))
}

const PENGINE_MEMORY_SERVER_KEY: &str = "te_pengine-memory";

enum Backend {
    KnowledgeGraph,
}

pub struct MemoryProvider {
    backend: Backend,
    provider: Provider,
}

/// Check if a JSON Schema has `properties.<array_prop>.items.properties` containing all `keys`.
fn schema_has_array_item_keys(schema: &Value, array_prop: &str, keys: &[&str]) -> bool {
    schema
        .get("properties")
        .and_then(|p| p.get(array_prop))
        .and_then(|a| a.get("items"))
        .and_then(|i| i.get("properties"))
        .and_then(|p| p.as_object())
        .is_some_and(|props| keys.iter().all(|k| props.contains_key(*k)))
}

fn is_knowledge_graph_shape(tools: &[ToolDef]) -> bool {
    let create = tools.iter().find(|t| t.name == "create_entities");
    let add = tools.iter().find(|t| t.name == "add_observations");
    match (create, add) {
        (Some(c), Some(a)) => {
            schema_has_array_item_keys(
                &c.input_schema,
                "entities",
                &["name", "entityType", "observations"],
            ) && schema_has_array_item_keys(
                &a.input_schema,
                "observations",
                &["entityName", "contents"],
            )
        }
        _ => false,
    }
}

impl MemoryProvider {
    pub fn detect(reg: &ToolRegistry) -> Option<Self> {
        for p in reg.providers() {
            let tools = p.tools();
            let has_tools = tools.iter().any(|t| t.name == "create_entities")
                && tools.iter().any(|t| t.name == "add_observations");
            if !has_tools {
                continue;
            }
            if is_knowledge_graph_shape(&tools) || p.server_name() == PENGINE_MEMORY_SERVER_KEY {
                return Some(Self {
                    backend: Backend::KnowledgeGraph,
                    provider: p.clone(),
                });
            }
        }
        None
    }

    pub fn server_name(&self) -> &str {
        self.provider.server_name()
    }

    pub fn provider_clone(&self) -> Self {
        Self {
            backend: Backend::KnowledgeGraph,
            provider: self.provider.clone(),
        }
    }

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
                if let Err(e) = self.provider.call_tool("create_entities", args).await {
                    let el = e.to_lowercase();
                    if !el.contains("already exist") && !el.contains("duplicate") {
                        return Err(e);
                    }
                }
            }
        }
        Ok(())
    }

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
        for p in SESSION_START.all_phrases() {
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
        for p in SESSION_END.all_phrases() {
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

    #[test]
    fn casual_mentions_do_not_trigger() {
        assert_eq!(detect_session_command("I want to quit my job"), None);
        assert_eq!(detect_session_command("exit the building safely"), None);
        assert_eq!(
            detect_session_command("please remember this session later"),
            None
        );
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
        assert_eq!(
            detect_session_command("Captain Jean Luc out"),
            Some(SessionCommand::End)
        );
        assert_eq!(detect_session_command("Captain Jean Luc Picard out"), None);
        assert_eq!(detect_session_command("Captain Picard 2 out"), None);
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
        assert_eq!(
            detect_session_command("Captain\u{2019}s log"),
            Some(SessionCommand::Start)
        );
        assert_eq!(
            detect_session_command("captain\u{2019}s log\u{2026}"),
            Some(SessionCommand::Start)
        );
    }

    #[test]
    fn entity_names_are_sortable_and_prefixed() {
        let t1 = chrono::DateTime::parse_from_rfc3339("2026-04-16T10:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let t2 = chrono::DateTime::parse_from_rfc3339("2026-04-16T11:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(entity_name("session", t1) < entity_name("session", t2));
        assert!(entity_name("session", t1).starts_with("session-"));
        assert!(entity_name("diary", t1).starts_with("diary-"));
    }

    #[test]
    fn cjk_trailing_punctuation_stripped() {
        assert_eq!(
            detect_session_command("record\u{3002}"),
            Some(SessionCommand::DiaryStart)
        );
        assert_eq!(
            detect_session_command("quit\u{FF01}"),
            Some(SessionCommand::End)
        );
    }
}
