//! Generic, backend-agnostic memory capability.
//!
//! The agent talks to `MemoryProvider` without knowing which MCP server is behind it.
//! Detection prefers **tool shape** plus JSON `inputSchema` checks for the Knowledge Graph memory
//! tools (or the official catalog server key `te_pengine-memory`).
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
use crate::modules::mcp::types::ToolDef;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use std::collections::HashSet;
use unicode_general_category::{get_general_category, GeneralCategory};
use unicode_normalization::UnicodeNormalization;

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

/// Starfleet sign-off: `<rank> <name…> out` with **3–4** tokens only (e.g. `Commander Worf out`,
/// `Captain Jean Luc out`). Middle name tokens must be alphabetic — no digits or punctuation —
/// so stray chatter does not match.
fn is_starfleet_signoff(normalized: &str) -> bool {
    let toks: Vec<&str> = normalized.split_whitespace().collect();
    if toks.len() < 3 || toks.len() > 4 {
        return false;
    }
    let first = toks.first().copied().unwrap_or("");
    let last = toks.last().copied().unwrap_or("");
    if !matches!(first, "commander" | "captain") || last != "out" {
        return false;
    }
    toks[1..toks.len() - 1]
        .iter()
        .all(|t| !t.is_empty() && t.chars().all(|c| c.is_alphabetic()))
}

/// Catalog `mcp.json` key for [`crate::modules::tool_engine::service::server_key`] `"pengine/memory"`.
const PENGINE_MEMORY_SERVER_KEY: &str = "te_pengine-memory";

fn normalize_curly_quotes_and_nfkc(msg: &str) -> String {
    let s: String = msg.chars().nfkc().collect();
    s.chars()
        .map(|c| match c {
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            c => c,
        })
        .collect()
}

fn is_unicode_or_ascii_trailing_junk(c: char) -> bool {
    c.is_whitespace()
        || c.is_ascii_punctuation()
        || matches!(
            c,
            '\u{2018}'
                | '\u{2019}'
                | '\u{201C}'
                | '\u{201D}'
                | '\u{2026}'
                | '\u{3002}'
                | '\u{FF01}'
                | '\u{FF0C}'
                | '\u{FF0E}'
                | '\u{FF1A}'
                | '\u{FF1B}'
                | '\u{FF1F}'
        )
        || matches!(
            get_general_category(c),
            GeneralCategory::DashPunctuation
                | GeneralCategory::OpenPunctuation
                | GeneralCategory::ClosePunctuation
                | GeneralCategory::ConnectorPunctuation
                | GeneralCategory::OtherPunctuation
                | GeneralCategory::InitialPunctuation
                | GeneralCategory::FinalPunctuation
        )
}

fn trim_session_message_end(s: &str) -> &str {
    s.trim_end_matches(is_unicode_or_ascii_trailing_junk)
}

/// `properties.<array>.items.properties` contains every key in `item_keys` (Knowledge Graph memory tools).
fn schema_array_items_have_keys(schema: &Value, array_prop: &str, item_keys: &[&str]) -> bool {
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return false;
    };
    let Some(arr) = props.get(array_prop) else {
        return false;
    };
    let Some(item_props) = arr
        .get("items")
        .and_then(|i| i.get("properties"))
        .and_then(|p| p.as_object())
    else {
        return false;
    };
    item_keys.iter().all(|k| item_props.contains_key(*k))
}

fn kg_create_entities_schema_ok(schema: &Value) -> bool {
    schema_array_items_have_keys(schema, "entities", &["name", "entityType", "observations"])
}

fn kg_add_observations_schema_ok(schema: &Value) -> bool {
    schema_array_items_have_keys(schema, "observations", &["entityName", "contents"])
}

fn knowledge_graph_memory_schemas_match(tools: &[ToolDef]) -> bool {
    let create = tools.iter().find(|t| t.name == "create_entities");
    let add_obs = tools.iter().find(|t| t.name == "add_observations");
    let (Some(c), Some(a)) = (create, add_obs) else {
        return false;
    };
    kg_create_entities_schema_ok(&c.input_schema) && kg_add_observations_schema_ok(&a.input_schema)
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

/// Match a user message against the session keyword lists. Applies Unicode NFKC, maps curly
/// quotes to ASCII, full Unicode-savvy trailing punctuation trim, then Unicode lowercase. Only
/// exact matches count — casual substrings like "I want to quit my job" do **not** end the session.
pub fn detect_session_command(msg: &str) -> Option<SessionCommand> {
    let normalized = normalize_curly_quotes_and_nfkc(msg);
    let normalized = normalized.trim();
    let normalized = trim_session_message_end(normalized);
    let normalized = normalized.to_lowercase();
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

/// Build a diary entity name: `diary-YYYYMMDDThhmmssZ`.
pub fn diary_entity_name(at: DateTime<Utc>) -> String {
    format!("diary-{}", at.format("%Y%m%dT%H%M%SZ"))
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
    /// Find a memory-capable server in the registry.
    ///
    /// **Tie-break:** providers are scanned in registry iteration order; the **first** server
    /// that passes validation is selected. Callers cannot set preference today.
    ///
    /// A server is accepted when it advertises `create_entities` and `add_observations` **and**
    /// either (a) both tools’ JSON `inputSchema`s match the Knowledge Graph memory shape, or
    /// (b) the server key is the official catalog entry (`te_pengine-memory`).
    pub fn detect(reg: &ToolRegistry) -> Option<Self> {
        for p in reg.providers() {
            let tools_vec = p.tools();
            let names: HashSet<String> = tools_vec.iter().map(|t| t.name.clone()).collect();
            if !names.contains("create_entities") || !names.contains("add_observations") {
                continue;
            }
            let schema_ok = knowledge_graph_memory_schemas_match(&tools_vec);
            let catalog_key = p.server_name() == PENGINE_MEMORY_SERVER_KEY;
            if schema_ok || catalog_key {
                log::info!(
                    "memory: selected MCP provider `{}` (knowledge_graph schema_ok={} catalog_key={})",
                    p.server_name(),
                    schema_ok,
                    catalog_key
                );
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
                match self.provider.call_tool("create_entities", args).await {
                    Ok(_) => {}
                    Err(e) => {
                        let el = e.to_lowercase();
                        if el.contains("already exists")
                            || el.contains("already exist")
                            || el.contains("duplicate")
                            || el.contains("entity already")
                            || el.contains("already known")
                        {
                            return Ok(());
                        }
                        return Err(e);
                    }
                }
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
        assert_eq!(
            detect_session_command("Captain Jean Luc out"),
            Some(SessionCommand::End)
        );
        // Too many tokens — don't fire.
        assert_eq!(detect_session_command("Captain Jean Luc Picard out"), None);
        // Middle token with digit — don't fire.
        assert_eq!(detect_session_command("Captain Picard 2 out"), None);
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

    #[test]
    fn diary_entity_name_has_diary_prefix() {
        let ts = chrono::DateTime::parse_from_rfc3339("2026-04-17T09:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let name = diary_entity_name(ts);
        assert!(name.starts_with("diary-"));
        assert!(!name.starts_with("session-"));
    }

    #[test]
    fn cjk_trailing_punctuation_stripped() {
        // Chinese period (U+3002) and fullwidth exclamation (U+FF01)
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
