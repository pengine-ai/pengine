use super::client::McpClient;
use super::native::NativeProvider;
use super::types::{ToolDef, ToolRisk};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

/// Normalize File Manager paths: absolute container paths pass through; relative `pengine/README.md` → `/app/pengine/README.md`.
fn rewrite_file_manager_path(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return t.to_string();
    }
    // Models often confuse the image WORKDIR (`/mcp`) with an allowed root — it is not.
    if let Some(rest) = t.strip_prefix("/mcp/") {
        return format!("/app/{rest}");
    }
    if t == "/mcp" {
        log::warn!(
            "rewrite_file_manager_path: `/mcp` is the server working directory, not an MCP file root; leaving path unchanged"
        );
        return "/mcp".to_string();
    }
    if t.starts_with("/opt/mcp-filesystem") {
        return t.to_string();
    }
    if t.starts_with('/') {
        return t.to_string();
    }
    if t.contains(':') || t.starts_with("\\\\") {
        return t.to_string();
    }
    resolve_relative_under_app(t)
}

/// Resolve a relative path under `/app` with `..` handling; escaping above the root → `/app`.
fn resolve_relative_under_app(raw: &str) -> String {
    let u = raw.replace('\\', "/");
    let mut stack: Vec<&str> = Vec::new();
    let mut escaped = false;
    for seg in u.split('/').filter(|s| !s.is_empty() && *s != ".") {
        if seg == ".." {
            if stack.pop().is_none() {
                escaped = true;
            }
        } else {
            stack.push(seg);
        }
    }
    if escaped {
        return "/app".to_string();
    }
    if stack.is_empty() {
        "/app".to_string()
    } else {
        format!("/app/{}", stack.join("/"))
    }
}

fn normalize_file_manager_tool_args(v: Value) -> Value {
    match v {
        Value::Object(mut map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                let Some(val) = map.remove(&k) else {
                    continue;
                };
                let val = match k.as_str() {
                    "path" => match val {
                        Value::String(s) => Value::String(rewrite_file_manager_path(&s)),
                        other => normalize_file_manager_tool_args(other),
                    },
                    "paths" => match val {
                        Value::Array(arr) => Value::Array(
                            arr.into_iter()
                                .map(|item| match item {
                                    Value::String(s) => {
                                        Value::String(rewrite_file_manager_path(&s))
                                    }
                                    other => normalize_file_manager_tool_args(other),
                                })
                                .collect(),
                        ),
                        other => normalize_file_manager_tool_args(other),
                    },
                    _ => normalize_file_manager_tool_args(val),
                };
                map.insert(k, val);
            }
            Value::Object(map)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(normalize_file_manager_tool_args)
                .collect(),
        ),
        other => other,
    }
}

#[derive(Clone)]
pub enum Provider {
    Native(Arc<NativeProvider>),
    Mcp(Arc<McpClient>),
}

impl Provider {
    pub fn server_name(&self) -> &str {
        match self {
            Provider::Native(n) => &n.server_name,
            Provider::Mcp(c) => &c.server_name,
        }
    }

    pub fn tools(&self) -> Vec<ToolDef> {
        match self {
            Provider::Native(n) => n.tools.clone(),
            Provider::Mcp(c) => c.tools(),
        }
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        match self {
            Provider::Native(n) => n.call(name, &args).await,
            Provider::Mcp(c) => c.call_tool(name, args).await,
        }
    }
}

pub struct ToolRegistry {
    providers: Vec<Provider>,
    cached_ollama_tools: Value,
    cached_tool_names: Vec<String>,
}

/// Subset of tools passed to the model for one turn (routing + observability).
#[derive(Debug, Clone)]
pub struct ToolContextSelection {
    pub tools_json: Value,
    pub total_count: usize,
    pub active_count: usize,
    pub used_subset: bool,
    /// Why this shape was chosen: `full` = entire registry, `ranked` = keyword/recent top-K,
    /// `core_no_signal` = no scores (e.g. non-English) — always-on + memory only.
    pub routing: &'static str,
    pub select_ms: u64,
    pub high_risk_active: usize,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            cached_ollama_tools: Value::Array(Vec::new()),
            cached_tool_names: Vec::new(),
        }
    }
}

impl ToolRegistry {
    pub fn new(providers: Vec<Provider>) -> Self {
        let active_tools: Vec<ToolDef> = providers
            .iter()
            .flat_map(|p| p.tools())
            .filter(|t| !is_deprecated_mcp_tool(t))
            .collect();
        let cached_ollama_tools = build_ollama_tools(&active_tools);
        let cached_tool_names = active_tools.iter().map(|t| t.name.clone()).collect();
        Self {
            providers,
            cached_ollama_tools,
            cached_tool_names,
        }
    }

    pub fn all_tools(&self) -> Vec<ToolDef> {
        self.providers
            .iter()
            .flat_map(|p| p.tools())
            .filter(|t| !is_deprecated_mcp_tool(t))
            .collect()
    }

    pub fn ollama_tools(&self) -> Value {
        self.cached_ollama_tools.clone()
    }

    /// Tools for one model turn: keyword + **recent-use** ranking, always-on
    /// tools, and (when relevant) memory MCP tools — including while a **full chat
    /// session** is recording (`chat_session_recording`), so the model can call
    /// memory APIs on every turn, not only `fetch`/`time`.
    pub fn select_tools_for_turn(
        &self,
        user_message: &str,
        recent_tool_names: &[String],
        memory_server: Option<&str>,
        chat_session_recording: bool,
    ) -> ToolContextSelection {
        let t0 = Instant::now();
        let all = self.all_tools();
        let total = all.len();
        match route_tools(
            &all,
            user_message,
            recent_tool_names,
            memory_server,
            chat_session_recording,
        ) {
            ToolRoutePlan::Subset {
                tools: selected,
                routing,
            } => {
                let select_ms = t0.elapsed().as_millis() as u64;
                let high = selected.iter().filter(|t| t.risk == ToolRisk::High).count();
                let active = selected.len();
                ToolContextSelection {
                    tools_json: build_ollama_tools(&selected),
                    total_count: total,
                    active_count: active,
                    used_subset: true,
                    routing,
                    select_ms,
                    high_risk_active: high,
                }
            }
            ToolRoutePlan::FullCatalog => {
                let select_ms = t0.elapsed().as_millis() as u64;
                let high = all.iter().filter(|t| t.risk == ToolRisk::High).count();
                ToolContextSelection {
                    tools_json: self.cached_ollama_tools.clone(),
                    total_count: total,
                    active_count: total,
                    used_subset: false,
                    routing: "full",
                    select_ms,
                    high_risk_active: high,
                }
            }
        }
    }

    /// Full catalog (no subset). Used after routing escalation.
    pub fn full_tool_context(&self) -> ToolContextSelection {
        let all = self.all_tools();
        let total = self.cached_tool_names.len();
        let high = all.iter().filter(|t| t.risk == ToolRisk::High).count();
        ToolContextSelection {
            tools_json: self.cached_ollama_tools.clone(),
            total_count: total,
            active_count: total,
            used_subset: false,
            routing: "full_escalation",
            select_ms: 0,
            high_risk_active: high,
        }
    }

    /// Names of commands offered to the model (flattened across all MCP tools).
    pub fn tool_names(&self) -> &[String] {
        &self.cached_tool_names
    }

    /// `true` when there is no command to expose (e.g. nothing connected yet).
    pub fn is_empty(&self) -> bool {
        self.cached_tool_names.is_empty()
    }

    /// Read-only access to the registered providers. Used by capability detectors
    /// (e.g. `memory::MemoryProvider::detect`) that pick a server by its tool shape.
    pub fn providers(&self) -> &[Provider] {
        &self.providers
    }

    fn normalize_tool_args_for_provider(provider: &Provider, args: Value) -> Value {
        match provider {
            Provider::Mcp(c) if is_pengine_file_manager_server_key(c.server_name.as_str()) => {
                normalize_file_manager_tool_args(args)
            }
            _ => args,
        }
    }

    /// Resolve a tool and rewrite arguments (e.g. File Manager `/mcp/...` → `/app/...`).
    /// Call [`Provider::call_tool`] with the returned name and args **after** releasing any lock on this registry.
    pub fn prepare_tool_invocation(
        &self,
        name: &str,
        args: Value,
    ) -> Result<(Provider, String, bool, Value), String> {
        let (provider, tool, direct) = self.resolve_tool(name)?;
        let args = Self::normalize_tool_args_for_provider(&provider, args);
        Ok((provider, tool, direct, args))
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<(String, bool), String> {
        let (provider, tool, direct) = self.resolve_tool(name)?;
        let args = Self::normalize_tool_args_for_provider(&provider, args);
        let text = provider.call_tool(&tool, args).await?;
        Ok((text, direct))
    }

    pub fn resolve_tool(&self, name: &str) -> Result<(Provider, String, bool), String> {
        let (server, tool) = match name.split_once('.') {
            Some((s, t)) => (Some(s), t),
            None => (None, name),
        };

        if server.is_none() {
            let mut found: Vec<(Provider, ToolDef)> = Vec::new();
            for provider in &self.providers {
                if let Some(def) = provider.tools().into_iter().find(|t| t.name == tool) {
                    found.push((provider.clone(), def));
                }
            }
            return match found.len() {
                0 => Err(format!("tool not found: {name}")),
                1 => {
                    let (p, d) = found.into_iter().next().expect("len 1");
                    Ok((p, tool.to_string(), d.direct_return))
                }
                _ => {
                    let servers: Vec<_> = found.iter().map(|(p, _)| p.server_name()).collect();
                    Err(format!(
                        "ambiguous tool `{tool}`: matches servers {}",
                        servers.join(", ")
                    ))
                }
            };
        }

        if let Some(s) = server {
            let key = s.trim();
            for provider in &self.providers {
                if !provider.server_name().eq_ignore_ascii_case(key) {
                    continue;
                }
                if let Some(def) = provider.tools().into_iter().find(|t| t.name == tool) {
                    return Ok((provider.clone(), tool.to_string(), def.direct_return));
                }
            }
        }
        Err(format!("tool not found: {name}"))
    }
}

/// `mcp.json` key for the catalog tool `pengine/file-manager` (same formula as `tool_engine::server_key`).
fn is_pengine_file_manager_server_key(key: &str) -> bool {
    key.eq_ignore_ascii_case("te_pengine-file-manager")
}

/// Hide tools the server marks as deprecated (e.g. filesystem `read_file` → use `read_text_file`).
fn is_deprecated_mcp_tool(tool: &ToolDef) -> bool {
    tool.description
        .as_deref()
        .unwrap_or("")
        .to_ascii_uppercase()
        .contains("DEPRECATED")
}

/// Strip `"description"` keys from nested property objects inside a JSON Schema to reduce
/// token count. Keeps `type`, `properties`, `required`, `enum`, `items`, `default`, etc.
fn compact_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut out = serde_json::Map::with_capacity(map.len());
            for (k, v) in map {
                if k == "description" {
                    continue;
                }
                out.insert(k.clone(), compact_schema(v));
            }
            Value::Object(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(compact_schema).collect()),
        other => other.clone(),
    }
}

/// Max characters kept from a tool's top-level description when advertising
/// tools to the model. The first clause is enough for routing; longer prose
/// balloons the step-0 prompt by ~80 tokens per tool (40 tools → ~3k tokens).
const TOOL_DESCRIPTION_CHAR_CAP: usize = 80;

fn build_ollama_tools(tools: &[ToolDef]) -> Value {
    let arr: Vec<Value> = tools
        .iter()
        .map(|t| {
            let description = shorten_tool_description(
                t.description.as_deref().unwrap_or(""),
                TOOL_DESCRIPTION_CHAR_CAP,
            );
            json!({
                "type": "function",
                "function": {
                    "name": &t.name,
                    "description": description,
                    "parameters": compact_schema(&t.input_schema),
                }
            })
        })
        .collect();
    Value::Array(arr)
}

/// Keyword-routing tuning. `ROUTED_TOOL_BUDGET` caps the top-K tools kept per
/// request before the always-on core set is appended. `MIN_TOKEN_CHARS` and
/// `ROUTING_STOPWORDS` prevent short function words from inflating every
/// tool's score — "the" / "you" would otherwise match nearly any description.
const ROUTED_TOOL_BUDGET: usize = 8;
const MIN_TOKEN_CHARS: usize = 3;
const ROUTING_STOPWORDS: &[&str] = &[
    "the", "and", "you", "for", "but", "not", "was", "are", "has", "have", "this", "that", "with",
    "from", "into", "what", "when", "where", "how", "why", "who", "does", "did", "will", "can",
    "would", "could", "should", "need", "some", "any", "all", "your", "its", "them", "they",
    "about",
];

/// Tools exposed on every turn regardless of routing. Keeps the model from
/// getting stuck when keyword matching misses (short queries, non-English
/// intent words, typos) — a tiny token cost for a big correctness win.
const ALWAYS_ON_TOOL_NAMES: &[&str] = &["fetch", "time"];

#[derive(Debug)]
enum ToolRoutePlan {
    FullCatalog,
    Subset {
        tools: Vec<ToolDef>,
        routing: &'static str,
    },
}

fn registry_routing_threshold(tools: &[ToolDef], memory_server: Option<&str>) -> usize {
    let memory_tool_count = memory_server
        .map(|m| {
            tools
                .iter()
                .filter(|t| t.server_name.eq_ignore_ascii_case(m))
                .count()
        })
        .unwrap_or(0);
    ROUTED_TOOL_BUDGET + ALWAYS_ON_TOOL_NAMES.len() + memory_tool_count
}

fn push_always_on_tools(
    tools: &[ToolDef],
    selected: &mut Vec<ToolDef>,
    seen: &mut HashSet<String>,
) {
    for tool in tools {
        if ALWAYS_ON_TOOL_NAMES
            .iter()
            .any(|n| tool.name.eq_ignore_ascii_case(n))
            && seen.insert(tool.name.clone())
        {
            selected.push(tool.clone());
        }
    }
}

/// Memory MCP tools are large; only attach them when the user likely needs memory
/// (Captain's log / session recording, recent use, or phrasing), not for unrelated
/// weather-only turns when nothing is being recorded.
fn memory_tools_relevant(
    user_message: &str,
    recent_tool_names: &[String],
    tools: &[ToolDef],
    memory_server: Option<&str>,
    chat_session_recording: bool,
) -> bool {
    let Some(mem_srv) = memory_server else {
        return false;
    };
    if chat_session_recording {
        return true;
    }
    if recent_tool_names.iter().any(|r| {
        tools
            .iter()
            .any(|t| t.server_name.eq_ignore_ascii_case(mem_srv) && tool_name_matches_recent(t, r))
    }) {
        return true;
    }
    let lower = user_message.to_lowercase();
    const HINTS: &[&str] = &[
        "remember",
        "session",
        "diary",
        "captain's log",
        "captain\u{2019}s log",
        "captains log",
        "tagebuch",
        "notizbuch",
        "gedächtnis",
        "gedachtnis",
        "memory",
        "read the log",
        "open the log",
        "merk dir",
        "speicher",
        "transcript",
    ];
    HINTS.iter().any(|h| lower.contains(h))
}

fn push_memory_server_tools(
    tools: &[ToolDef],
    memory_server: Option<&str>,
    selected: &mut Vec<ToolDef>,
    seen: &mut HashSet<String>,
) {
    if let Some(m) = memory_server {
        for tool in tools {
            if tool.server_name.eq_ignore_ascii_case(m) && seen.insert(tool.name.clone()) {
                selected.push(tool.clone());
            }
        }
    }
}

/// Large registries: return a subset. Small registries (`len` ≤ threshold): pass the full list.
/// When every tool scores 0 (common for non-English queries before any tool was used recently),
/// use **always-on** tools only (`fetch`, `time`) — not the full catalog.
fn route_tools(
    tools: &[ToolDef],
    user_message: &str,
    recent_tool_names: &[String],
    memory_server: Option<&str>,
    chat_session_recording: bool,
) -> ToolRoutePlan {
    let threshold = registry_routing_threshold(tools, memory_server);
    if tools.len() <= threshold {
        return ToolRoutePlan::FullCatalog;
    }

    let query_tokens = route_tokens(user_message);
    let mut scored: Vec<(usize, &ToolDef)> = tools
        .iter()
        .map(|t| {
            let s = score_tool_combined(t, &query_tokens, recent_tool_names, user_message);
            (s, t)
        })
        .collect();

    if scored.iter().all(|(s, _)| *s == 0) {
        let mut selected = Vec::new();
        let mut seen = HashSet::new();
        push_always_on_tools(tools, &mut selected, &mut seen);
        if memory_tools_relevant(
            user_message,
            recent_tool_names,
            tools,
            memory_server,
            chat_session_recording,
        ) {
            push_memory_server_tools(tools, memory_server, &mut selected, &mut seen);
        }
        selected.sort_by(|a, b| a.name.cmp(&b.name));
        return ToolRoutePlan::Subset {
            tools: selected,
            routing: "core_no_signal",
        };
    }

    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.name.cmp(&b.1.name)));

    let mut selected: Vec<ToolDef> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for (score, tool) in &scored {
        if *score == 0 || selected.len() >= ROUTED_TOOL_BUDGET {
            break;
        }
        if seen.insert(tool.name.clone()) {
            selected.push((*tool).clone());
        }
    }

    push_always_on_tools(tools, &mut selected, &mut seen);
    if memory_tools_relevant(
        user_message,
        recent_tool_names,
        tools,
        memory_server,
        chat_session_recording,
    ) {
        push_memory_server_tools(tools, memory_server, &mut selected, &mut seen);
    }

    ToolRoutePlan::Subset {
        tools: selected,
        routing: "ranked",
    }
}

fn message_suggests_url_fetch(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    const HINTS: &[&str] = &[
        "wetter",
        "weather",
        "forecast",
        "vorhersage",
        "regenwahrscheinlichkeit",
        "temperatur",
        "gewitter",
        "schnee",
        "hagel",
        "wind",
        "niederschlag",
        "wttr",
        "http",
        "https",
        "curl",
        "api",
        "download",
        "abruf",
    ];
    HINTS.iter().any(|h| lower.contains(h))
}

fn score_tool_combined(
    tool: &ToolDef,
    query_tokens: &HashSet<String>,
    recent_tool_names: &[String],
    user_message: &str,
) -> usize {
    let mut s = if query_tokens.is_empty() {
        0
    } else {
        score_tool(tool, query_tokens)
    };
    s += recent_tool_score(tool, recent_tool_names);
    if tool.name.eq_ignore_ascii_case("fetch") && message_suggests_url_fetch(user_message) {
        s += 14;
    }
    s
}

/// Weight recent invocations: newest names in the deque score highest.
fn recent_tool_score(tool: &ToolDef, recent: &[String]) -> usize {
    let mut score = 0usize;
    for (i, r) in recent.iter().enumerate() {
        if tool_name_matches_recent(tool, r) {
            let weight = recent.len().saturating_sub(i);
            score += 8 + weight * 4;
        }
    }
    score
}

fn tool_name_matches_recent(tool: &ToolDef, recent: &str) -> bool {
    let r = recent.trim();
    if r.is_empty() {
        return false;
    }
    if tool.name.eq_ignore_ascii_case(r) {
        return true;
    }
    if let Some((_, short)) = r.rsplit_once('.') {
        if tool.name.eq_ignore_ascii_case(short) {
            return true;
        }
    }
    false
}

fn route_tokens(s: &str) -> HashSet<String> {
    let lower = s.to_lowercase();
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= MIN_TOKEN_CHARS && !ROUTING_STOPWORDS.contains(t))
        .map(|t| t.to_string())
        .collect()
}

fn score_tool(tool: &ToolDef, query_tokens: &HashSet<String>) -> usize {
    let mut haystack = tool.name.clone();
    if let Some(desc) = tool.description.as_deref() {
        haystack.push(' ');
        haystack.push_str(desc);
    }
    let tool_tokens = route_tokens(&haystack);
    query_tokens
        .iter()
        .filter(|q| tool_tokens.contains(q.as_str()))
        .count()
}

/// Keep the first sentence or up to `cap` chars — whichever comes first — and
/// trim trailing whitespace/punctuation fragments. Falls back to a hard cut
/// on a char boundary (with ellipsis) when no early sentence break exists.
fn shorten_tool_description(desc: &str, cap: usize) -> String {
    let trimmed = desc.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // Prefer the first sentence-ending period within budget.
    if let Some(end) = first_sentence_end(trimmed, cap) {
        return trimmed[..end].trim_end().to_string();
    }

    if trimmed.chars().count() <= cap {
        return trimmed.to_string();
    }

    let mut cut = 0;
    for (i, _) in trimmed.char_indices().take(cap) {
        cut = i;
    }
    // Advance one more char so `cut` is an *exclusive* end-index.
    if let Some((i, _)) = trimmed.char_indices().nth(cap) {
        cut = i;
    }
    let base = trimmed[..cut].trim_end();
    format!("{base}…")
}

/// Return the byte offset *after* the first `. ` / `.\n` / end-of-string
/// terminator within the first `cap` chars, or `None` if the first sentence
/// extends past the budget.
fn first_sentence_end(s: &str, cap: usize) -> Option<usize> {
    let mut chars = s.char_indices();
    for _ in 0..cap {
        let (i, c) = chars.next()?;
        if c == '.' {
            let next = s[i + c.len_utf8()..].chars().next();
            if matches!(next, None | Some(' ') | Some('\n') | Some('\t')) {
                return Some(i + c.len_utf8());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::mcp::types::ToolRisk;

    #[test]
    fn rewrite_container_and_relative() {
        assert_eq!(
            rewrite_file_manager_path("/app/pengine/README.md"),
            "/app/pengine/README.md"
        );
        assert_eq!(
            rewrite_file_manager_path("/mcp/pengine/readme.md"),
            "/app/pengine/readme.md"
        );
        assert_eq!(rewrite_file_manager_path("/mcp"), "/mcp");
        assert_eq!(
            rewrite_file_manager_path("pengine/README.md"),
            "/app/pengine/README.md"
        );
        assert_eq!(rewrite_file_manager_path("README.md"), "/app/README.md");
    }

    #[test]
    fn relative_path_traversal_collapses_to_app_root() {
        assert_eq!(
            rewrite_file_manager_path("pengine/../../etc/passwd"),
            "/app"
        );
    }

    #[test]
    fn normalize_paths_in_arguments() {
        let raw = json!({ "path": "pengine/readme.md" });
        let out = normalize_file_manager_tool_args(raw);
        assert_eq!(out["path"], "/app/pengine/readme.md");
    }

    #[test]
    fn compact_schema_strips_descriptions() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to read"
                },
                "encoding": {
                    "type": "string",
                    "description": "Character encoding",
                    "enum": ["utf-8", "ascii"]
                }
            },
            "required": ["path"]
        });
        let compact = compact_schema(&schema);
        assert_eq!(compact["type"], "object");
        assert_eq!(compact["required"], json!(["path"]));
        assert!(compact["properties"]["path"].get("description").is_none());
        assert_eq!(compact["properties"]["path"]["type"], "string");
        assert!(compact["properties"]["encoding"]
            .get("description")
            .is_none());
        assert_eq!(
            compact["properties"]["encoding"]["enum"],
            json!(["utf-8", "ascii"])
        );
    }

    #[test]
    fn shorten_tool_description_keeps_short_input_unchanged() {
        assert_eq!(shorten_tool_description("Read a file", 80), "Read a file");
    }

    #[test]
    fn shorten_tool_description_stops_at_first_sentence() {
        let desc = "Read a file. Returns UTF-8 content; fails if missing or binary.";
        assert_eq!(shorten_tool_description(desc, 80), "Read a file.");
    }

    #[test]
    fn shorten_tool_description_hard_cuts_when_no_sentence_break() {
        // No '. ' in the first 20 chars, so fall back to hard cut with ellipsis.
        let desc = "Read a file and return content eventually.";
        let out = shorten_tool_description(desc, 20);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 21, "got: {out:?}");
    }

    #[test]
    fn shorten_tool_description_empty_input() {
        assert_eq!(shorten_tool_description("", 80), "");
        assert_eq!(shorten_tool_description("   ", 80), "");
    }

    #[test]
    fn shorten_tool_description_period_inside_word_not_treated_as_sentence() {
        // `v1.0` should not end the sentence.
        let desc = "Use v1.0 of the API for this endpoint.";
        assert_eq!(
            shorten_tool_description(desc, 80),
            "Use v1.0 of the API for this endpoint."
        );
    }

    #[test]
    fn tool_name_matches_recent_qualified() {
        let t = ToolDef {
            server_name: "srv".into(),
            name: "fetch".into(),
            description: None,
            input_schema: json!({}),
            direct_return: false,
            category: None,
            risk: ToolRisk::Low,
        };
        assert!(super::tool_name_matches_recent(&t, "fetch"));
        assert!(super::tool_name_matches_recent(&t, "srv.fetch"));
        assert!(!super::tool_name_matches_recent(&t, "other"));
    }

    #[test]
    fn recent_tool_score_boosts_latest() {
        let t = ToolDef {
            server_name: "x".into(),
            name: "alpha".into(),
            description: None,
            input_schema: json!({}),
            direct_return: false,
            category: None,
            risk: ToolRisk::Low,
        };
        let recent = vec!["beta".into(), "alpha".into()];
        let s = super::recent_tool_score(&t, &recent);
        assert!(s > 0);
    }

    #[test]
    fn routing_all_zero_scores_uses_core_tools_not_full_catalog() {
        let mut tools: Vec<ToolDef> = (0..12)
            .map(|i| ToolDef {
                server_name: "srv".into(),
                name: format!("misc_{i}"),
                description: None,
                input_schema: json!({}),
                direct_return: false,
                category: None,
                risk: ToolRisk::Low,
            })
            .collect();
        for name in ["fetch", "time"] {
            tools.push(ToolDef {
                server_name: "srv".into(),
                name: name.into(),
                description: None,
                input_schema: json!({}),
                direct_return: false,
                category: None,
                risk: ToolRisk::Low,
            });
        }
        // 14 tools > threshold (8+2+0) without memory server. Message must not hit the
        // weather/fetch URL hint or every tool still scores 0.
        let plan = super::route_tools(&tools, "qqq zzz unrelated", &[], None, false);
        match plan {
            super::ToolRoutePlan::Subset {
                tools: sel,
                routing,
            } => {
                assert_eq!(routing, "core_no_signal");
                assert_eq!(sel.len(), 2);
                assert!(sel.iter().any(|t| t.name == "fetch"));
                assert!(sel.iter().any(|t| t.name == "time"));
            }
            super::ToolRoutePlan::FullCatalog => panic!("expected core subset"),
        }
    }

    #[test]
    fn routing_german_weather_does_not_auto_attach_memory_mcp() {
        let mut tools: Vec<ToolDef> = (0..12)
            .map(|i| ToolDef {
                server_name: "srv".into(),
                name: format!("misc_{i}"),
                description: None,
                input_schema: json!({}),
                direct_return: false,
                category: None,
                risk: ToolRisk::Low,
            })
            .collect();
        for name in ["fetch", "time"] {
            tools.push(ToolDef {
                server_name: "srv".into(),
                name: name.into(),
                description: None,
                input_schema: json!({}),
                direct_return: false,
                category: None,
                risk: ToolRisk::Low,
            });
        }
        for i in 0..4 {
            tools.push(ToolDef {
                server_name: "memsrv".into(),
                name: format!("mem_{i}"),
                description: None,
                input_schema: json!({}),
                direct_return: false,
                category: None,
                risk: ToolRisk::Low,
            });
        }
        let plan = super::route_tools(
            &tools,
            "wie wird morgen das wetter in berlin",
            &[],
            Some("memsrv"),
            false,
        );
        match plan {
            super::ToolRoutePlan::Subset {
                tools: sel,
                routing,
            } => {
                assert_eq!(routing, "ranked");
                assert_eq!(sel.len(), 2, "{sel:?}");
                assert!(sel.iter().any(|t| t.name == "fetch"));
                assert!(sel.iter().any(|t| t.name == "time"));
                assert!(!sel.iter().any(|t| t.server_name == "memsrv"));
            }
            super::ToolRoutePlan::FullCatalog => panic!("expected ranked subset"),
        }
    }

    #[test]
    fn routing_chat_session_recording_includes_memory_mcp_with_weather() {
        let mut tools: Vec<ToolDef> = (0..12)
            .map(|i| ToolDef {
                server_name: "srv".into(),
                name: format!("misc_{i}"),
                description: None,
                input_schema: json!({}),
                direct_return: false,
                category: None,
                risk: ToolRisk::Low,
            })
            .collect();
        for name in ["fetch", "time"] {
            tools.push(ToolDef {
                server_name: "srv".into(),
                name: name.into(),
                description: None,
                input_schema: json!({}),
                direct_return: false,
                category: None,
                risk: ToolRisk::Low,
            });
        }
        for i in 0..4 {
            tools.push(ToolDef {
                server_name: "memsrv".into(),
                name: format!("mem_{i}"),
                description: None,
                input_schema: json!({}),
                direct_return: false,
                category: None,
                risk: ToolRisk::Low,
            });
        }
        let plan = super::route_tools(
            &tools,
            "wie wird morgen das wetter in berlin",
            &[],
            Some("memsrv"),
            true,
        );
        match plan {
            super::ToolRoutePlan::Subset {
                tools: sel,
                routing,
            } => {
                assert_eq!(routing, "ranked");
                assert_eq!(sel.len(), 6, "{sel:?}");
                assert!(sel.iter().any(|t| t.name == "fetch"));
                assert!(sel.iter().any(|t| t.name == "time"));
                assert_eq!(sel.iter().filter(|t| t.server_name == "memsrv").count(), 4);
            }
            super::ToolRoutePlan::FullCatalog => panic!("expected ranked subset"),
        }
    }
}
