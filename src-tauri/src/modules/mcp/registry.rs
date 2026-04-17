use super::client::McpClient;
use super::native::NativeProvider;
use super::types::ToolDef;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::sync::Arc;

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

    /// Tools scoped to `user_message`: keeps the top-K that share lowercase
    /// tokens with the query (plus the always-on core set). Falls back to the
    /// full cached list when the registry is small, the query is tokenless, or
    /// no tool matches — so needed tools are never silently hidden.
    pub fn ollama_tools_for(&self, user_message: &str) -> Value {
        let all = self.all_tools();
        match route_tools(&all, user_message) {
            Some(selected) => build_ollama_tools(&selected),
            None => self.cached_ollama_tools.clone(),
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

/// Returns the routed tool subset, or `None` to signal that the caller should
/// keep the full cached list (small registry, tokenless query, or ambiguous
/// scoring where no tool matches).
fn route_tools(tools: &[ToolDef], user_message: &str) -> Option<Vec<ToolDef>> {
    if tools.len() <= ROUTED_TOOL_BUDGET + ALWAYS_ON_TOOL_NAMES.len() {
        return None;
    }

    let query_tokens = route_tokens(user_message);
    if query_tokens.is_empty() {
        return None;
    }

    let mut scored: Vec<(usize, &ToolDef)> = tools
        .iter()
        .map(|t| (score_tool(t, &query_tokens), t))
        .collect();

    if scored.iter().all(|(s, _)| *s == 0) {
        return None;
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

    for tool in tools {
        if ALWAYS_ON_TOOL_NAMES
            .iter()
            .any(|n| tool.name.eq_ignore_ascii_case(n))
            && seen.insert(tool.name.clone())
        {
            selected.push(tool.clone());
        }
    }

    Some(selected)
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
}
