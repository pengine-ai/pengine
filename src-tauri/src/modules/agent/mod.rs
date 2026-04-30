pub mod search_followup;

use crate::modules::memory::{self, MemoryProvider, SessionCommand};
use crate::modules::ollama::keywords::THINK_ON;
use crate::modules::ollama::service::{self as ollama, ChatOptions};
use crate::modules::skills::service as skills;
use crate::modules::tool_engine::service::workspace_app_bind_pairs;
use crate::shared::state::{AppState, MemorySession};
use crate::shared::text::{
    compact_tool_output, truncate_for_model, PENGINE_OUTPUT_CONTRACT_LEAD,
    PENGINE_POST_TOOL_REMINDER,
};
use chrono::Utc;
use serde_json::json;
use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, Instant};

/// Tool rounds + at least one completion-only step. Research flows (sitemap + several
/// `fetch` calls) otherwise exhaust the loop and fall through to summarize, which
/// used to drop URLs and paraphrase loosely.
const MAX_STEPS: usize = 6;
/// When the user asks to **apply** fixes (pre-commit, lint, edit files), allow more tool rounds:
/// read → write → verify (`git_diff`) without hitting the step cap early.
const MAX_STEPS_APPLY_FIX: usize = 10;

/// Brave Search web calls are billed; allow at most one `brave_web_search` per user message
/// (across all agent steps). Other Brave tools are unchanged.
const MAX_BRAVE_WEB_SEARCH_PER_USER_MESSAGE: u32 = 1;

const BRAVE_WEB_SEARCH_LIMIT_MSG: &str = "Pengine policy: at most one `brave_web_search` call per user message (cost control). \
Use the previous search result, answer without another search, or ask the user to narrow the query.";

const FETCH_DUPLICATE_URL_MSG: &str = "Pengine policy: this URL was already fetched successfully in this user message. \
Do not call `fetch` again for the same URL. Answer from the prior tool output (or use a different URL if the excerpt was insufficient).";

/// After tool results (agent step ≥1), cap completion tokens. The model should
/// put the user-visible answer in `<pengine_reply>` (see system prompt); this
/// cap bounds wall time if it drafts a long `<pengine_plan>`. ~1024 fits a
/// concise multilingual answer in most cases.
const POST_TOOL_NUM_PREDICT: u32 = 1024;
/// Larger cap when the user expects file mutations (edit/write + diff); avoids the model
/// exhausting tokens while planning the next tool call.
const POST_TOOL_NUM_PREDICT_APPLY_FIX: u32 = 2048;
const POST_TOOL_TEMPERATURE: f32 = 0.35;

/// Fallback summarize pass when the tool loop exits without a user-visible reply.
const SUMMARY_NUM_PREDICT: u32 = 768;
const SUMMARY_TEMPERATURE: f32 = 0.3;

const SUMMARY_SYSTEM_PROMPT: &str = "You synthesize tool results for the user. Rules:\n\
\n\
1) Use ONLY the text in the user message's Data section (tool outputs). Do not add facts, legal claims, or country-specific rules that are not clearly supported there.\n\
2) If the Data is insufficient, say so briefly and list what is missing — do not invent answers.\n\
3) Language: match the user's question where possible.\n\
4) After the substantive answer, add a final section **Quellen** with a bullet list of every relevant full URL you relied on:\n\
   - Include URLs from `brave_web_search` results and from every `fetch` block (including lines like `--- fetch (auto: https://…) ---`).\n\
   - Copy URLs exactly as they appear in the Data (fetch bodies, HTML links, or Location lines).\n\
   - If the Data shows only page text without URLs, write one bullet per tool block naming the fetch target if it appears in the `--- fetch ---` headers or quoted links in the excerpt.\n\
   - Never omit **Quellen** when the Data came from web search or fetches.\n\
5) Keep the body concise but do not drop **Quellen** to save space.\n\
6) No chain-of-thought, planning, or English meta: write only text that should appear in the user's chat bubble.";

const REPO_WRITE_CONTINUE_AFTER_PROSE: &str = "CONTINUE (repo files): This turn is **not** finished. Either you have not called **`edit_file`** / **`write_file`** / **`create_directory`** yet, or the **latest** tool output still shows a compile/clippy/lint/pre-commit failure. \
Your next step must be **tool calls** on the repo (absolute **`/app/...`** paths): create missing files/folders the user asked for, or patch what the log cites — then verify if appropriate. \
Do not reply with meta-commentary about output formats or a generic \"ready to assist\" offer.";

const REPO_WRITE_CONTINUE_AFTER_EMPTY: &str = "CONTINUE (repo files): You returned no assistant text and no tool calls after tool results. \
Use **`edit_file`** / **`write_file`** / **`create_directory`** as needed for the user's request or the errors in the latest tool output.";

/// When the MCP catalog is empty and the user did not enable `/think`, constrain the model to JSON
/// `{\"reply\":...}` so the host can take a single user-visible field (same schema as the summarize pass).
fn chat_options_for_agent_step(
    post_tool: bool,
    user_wants_think: bool,
    json_only_user_reply: bool,
    repo_write_followthrough: bool,
) -> ChatOptions {
    let format = (json_only_user_reply && !user_wants_think)
        .then_some(ollama::summarize_reply_json_schema());
    if !post_tool {
        ChatOptions {
            think: Some(user_wants_think),
            num_predict: None,
            temperature: None,
            format,
            ..ChatOptions::default()
        }
    } else {
        let cap = if repo_write_followthrough {
            POST_TOOL_NUM_PREDICT_APPLY_FIX
        } else {
            POST_TOOL_NUM_PREDICT
        };
        ChatOptions {
            think: Some(false),
            num_predict: Some(cap),
            temperature: Some(POST_TOOL_TEMPERATURE),
            format,
            ..ChatOptions::default()
        }
    }
}

/// Cap on tool output fed back to the model. Raw fetch bodies can be 5–10 kB
/// of HTML; the model only needs the first screen to answer, and larger
/// feedback balloons the step-1 prompt. Direct replies (non-fetch tools) are
/// not truncated before sending to the user.
const TOOL_OUTPUT_CHAR_CAP: usize = 4000;

async fn chat_with_cloud_fallback(
    state: &AppState,
    model: &mut String,
    messages: &serde_json::Value,
    tools: &serde_json::Value,
    options: &ChatOptions,
) -> Result<ollama::ChatResult, String> {
    match ollama::chat_with_tools(model, messages, tools, options).await {
        Ok(r) => Ok(r),
        Err(err) => {
            if ollama::classify_model(model) != ollama::ModelKind::Cloud {
                return Err(err);
            }

            let err_lower = err.to_ascii_lowercase();
            // Check for subscription error specifically
            let is_subscription_error = err_lower.contains("requires a subscription")
                || err_lower.contains("upgrade for access");

            // Check for rate limit error
            let is_rate_limit_error =
                err_lower.contains("rate limit") || err_lower.contains("quota exceeded");

            // Check for unavailable error
            let is_unavailable_error = ollama::is_cloud_unavailable_error(&err);

            let should_fallback =
                is_subscription_error || is_rate_limit_error || is_unavailable_error;

            if !should_fallback {
                return Err(err);
            }

            let reason = if is_subscription_error {
                "subscription required"
            } else if is_rate_limit_error {
                "rate-limited/quota exceeded"
            } else {
                "unavailable"
            };

            let last_local = state.last_local_model.read().await.clone();
            let catalog = ollama::model_catalog(3000).await.ok();
            let fallback = catalog
                .as_ref()
                .and_then(|c| ollama::pick_local_fallback(c, None, last_local.as_deref()));
            let Some(local) = fallback else {
                state
                    .emit_log(
                        "ollama",
                        &format!("cloud '{model}' {reason} ({err}); no local fallback"),
                    )
                    .await;
                return Err(err);
            };

            if local == *model {
                state
                    .emit_log(
                        "ollama",
                        &format!(
                            "cloud '{model}' {reason} ({err}); local fallback resolves to same model, cannot retry"
                        ),
                    )
                    .await;
                return Err(err);
            }

            state
                .emit_log(
                    "ollama",
                    &format!("cloud '{model}' {reason}, switching to local '{local}': {err}"),
                )
                .await;

            {
                let mut pref = state.preferred_ollama_model.write().await;
                *pref = Some(local.clone());
            }

            *model = local;
            ollama::chat_with_tools(model, messages, tools, options).await
        }
    }
}

fn tool_name_is_fetch(name: &str) -> bool {
    name.eq_ignore_ascii_case("fetch")
        || name
            .rsplit_once('.')
            .is_some_and(|(_, tail)| tail.eq_ignore_ascii_case("fetch"))
}

fn fetch_url_from_tool_args(args: &serde_json::Value) -> Option<String> {
    args.get("url")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::string::ToString::to_string)
}

/// Stable key so `https://Host/path/` and `https://host/path#x` count as one URL.
fn fetch_url_dedup_key(url: &str) -> String {
    let t = url.trim();
    let no_frag = t.split('#').next().unwrap_or(t).trim_end_matches('/');
    no_frag.to_lowercase()
}

/// Stdio MCP child exited or closed stdin — the existing [`crate::modules::mcp::client::McpClient`] is dead.
fn mcp_stdio_recoverable(err: &str) -> bool {
    let e = err.to_ascii_lowercase();
    e.contains("broken pipe") || e.contains("os error 32") || e.contains("connection reset")
}

/// Run a `task_spawn` tool call inline (not via [`crate::modules::mcp::registry::Provider::call_tool`]).
///
/// The recursive call into [`run_system_turn`] makes this future `!Send`, so it cannot be inserted
/// into the parallel `tokio::spawn` pool. The dispatcher detects task-spawner provider invocations
/// and routes them through this function instead.
async fn run_task_spawn_inline(
    state: &AppState,
    args: &serde_json::Value,
) -> Result<String, String> {
    use std::sync::atomic::Ordering;

    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("missing 'description'")?;
    let prompt = args
        .get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("missing 'prompt'")?
        .to_string();

    let depth_before = state.task_spawn_depth.load(Ordering::Acquire);
    if depth_before >= crate::modules::mcp::native::TASK_SPAWN_MAX_DEPTH {
        return Err(format!(
            "task_spawn refused: recursion depth {depth_before} >= cap {}",
            crate::modules::mcp::native::TASK_SPAWN_MAX_DEPTH
        ));
    }
    state.task_spawn_depth.fetch_add(1, Ordering::AcqRel);
    state
        .emit_log(
            "task",
            &format!("spawn[{}]: {description}", depth_before + 1),
        )
        .await;

    let started = std::time::Instant::now();
    // `Box::pin` breaks the cycle [run_model_turn → run_task_spawn_inline → run_system_turn → run_model_turn]
    // that would otherwise produce an infinitely-sized future.
    let result = Box::pin(run_system_turn(state, &prompt, None)).await;
    state.task_spawn_depth.fetch_sub(1, Ordering::AcqRel);

    match result {
        Ok(turn) => {
            state
                .emit_log(
                    "task",
                    &format!(
                        "done[{}]: {description} ({} ms, {} chars)",
                        depth_before + 1,
                        started.elapsed().as_millis(),
                        turn.text.chars().count()
                    ),
                )
                .await;
            Ok(turn.text)
        }
        Err(e) => {
            state
                .emit_log(
                    "task",
                    &format!("failed[{}]: {description}: {e}", depth_before + 1),
                )
                .await;
            Err(format!("sub-agent failed: {e}"))
        }
    }
}

/// One attempt to reconnect all MCP servers and retry the same tool after a transport failure.
async fn call_tool_with_mcp_recovery(
    state: &AppState,
    model_tool_name: &str,
    provider: crate::modules::mcp::registry::Provider,
    tool_name: String,
    args: serde_json::Value,
) -> Result<String, String> {
    match provider.call_tool(&tool_name, args.clone()).await {
        Ok(t) => Ok(t),
        Err(e) if mcp_stdio_recoverable(&e) => {
            state
                .emit_log(
                    "mcp",
                    &format!(
                        "tool `{model_tool_name}` transport error ({e}); rebuilding MCP registry"
                    ),
                )
                .await;
            crate::modules::mcp::service::rebuild_registry_into_state(state).await?;
            let (p2, tn2, _, a2) = {
                let reg = state.mcp.read().await;
                reg.prepare_tool_invocation(model_tool_name, args)
            }
            .map_err(|prep| format!("mcp reconnected but invocation failed: {prep}"))?;
            p2.call_tool(&tn2, a2).await
        }
        Err(e) => Err(e),
    }
}

/// After `brave_web_search`, prefetch this many distinct result URLs (one search per message; extra bandwidth here is `fetch` only).
const AUTO_FETCH_TOP_URLS: usize = search_followup::DEFAULT_AUTO_FETCH_CAP;

async fn append_host_prefetch_after_brave_search(
    state: &AppState,
    messages: &mut serde_json::Value,
    tool_results: &mut Vec<(String, String)>,
    search_blob: &str,
    fetch_urls_success: &mut HashSet<String>,
) {
    let urls = search_followup::extract_fetchable_urls(search_blob, AUTO_FETCH_TOP_URLS);
    for url in urls {
        let key = fetch_url_dedup_key(&url);
        if fetch_urls_success.contains(&key) {
            state
                .emit_log(
                    "tool",
                    &format!("[host] auto-fetch skip (already fetched): {url}"),
                )
                .await;
            continue;
        }
        state
            .emit_log("tool", &format!("[host] auto-fetch {url}"))
            .await;
        let prep = {
            let reg = state.mcp.read().await;
            reg.prepare_tool_invocation("fetch", json!({ "url": url.clone() }))
        };
        let Ok((provider, tool_name, _, args)) = prep else {
            continue;
        };
        let text = match provider.call_tool(&tool_name, args).await {
            Ok(t) => t,
            Err(e) => format!("ERROR: {e}"),
        };
        if !text.trim_start().starts_with("ERROR:") {
            fetch_urls_success.insert(key);
        }
        let compacted = compact_tool_output(&text);
        let for_model = truncate_for_model(&compacted, TOOL_OUTPUT_CHAR_CAP);
        let block_name = format!("fetch (auto: {url})");
        if let Some(arr) = messages.as_array_mut() {
            arr.push(json!({
                "role": "tool",
                "name": "fetch",
                "content": &for_model,
            }));
        }
        tool_results.push((block_name, for_model));
    }
}

fn push_ephemeral_post_tool_reminder(messages: &mut serde_json::Value) {
    if let Some(arr) = messages.as_array_mut() {
        arr.push(json!({
            "role": "system",
            "content": PENGINE_POST_TOOL_REMINDER,
        }));
    }
}

fn pop_ephemeral_post_tool_reminder(messages: &mut serde_json::Value) {
    let Some(arr) = messages.as_array_mut() else {
        return;
    };
    let pop = arr.last().is_some_and(|m| {
        m.get("role").and_then(|r| r.as_str()) == Some("system")
            && m.get("content").and_then(|c| c.as_str()) == Some(PENGINE_POST_TOOL_REMINDER)
    });
    if pop {
        arr.pop();
    }
}

/// Source of a think-mode decision for this turn, mainly for observability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ThinkSource {
    SlashOn,
    SlashOff,
    Keyword,
    Default,
}

impl ThinkSource {
    fn enabled(self) -> bool {
        matches!(self, Self::SlashOn | Self::Keyword)
    }

    fn label(self) -> &'static str {
        match self {
            Self::SlashOn => "on (/think)",
            Self::SlashOff => "off (/nothink)",
            Self::Keyword => "on (keyword)",
            Self::Default => "off (default)",
        }
    }
}

/// Strip a leading `/think` or `/nothink` slash command from `msg`. Returns
/// the override flag (if any) and the remaining message, borrowed from the
/// input. The command is only recognized when followed by whitespace or
/// end-of-input so `/thinker` and similar don't match.
fn parse_think_override(msg: &str) -> (Option<bool>, &str) {
    let trimmed = msg.trim_start();
    for (prefix, value) in [("/nothink", false), ("/think", true)] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let at_boundary = rest.chars().next().is_none_or(char::is_whitespace);
            if at_boundary {
                return (Some(value), rest.trim_start());
            }
        }
    }
    (None, msg)
}

/// Decide whether to enable Ollama thinking mode for this turn. Precedence:
/// explicit slash command wins; else the multilingual `THINK_ON` keyword
/// group; else off.
fn decide_think(override_flag: Option<bool>, cleaned_msg: &str) -> ThinkSource {
    match override_flag {
        Some(true) => ThinkSource::SlashOn,
        Some(false) => ThinkSource::SlashOff,
        None if THINK_ON.matches(cleaned_msg) => ThinkSource::Keyword,
        None => ThinkSource::Default,
    }
}

fn memory_hint(session_active: Option<&str>, diary_active: bool) -> String {
    let status = match session_active {
        Some(name) if diary_active => format!(" Diary ACTIVE (`{name}`)."),
        Some(name) => {
            format!(" Session ACTIVE (`{name}`); host saves each turn — no write tools.")
        }
        None => String::new(),
    };
    format!("\nMemory server ready. Use read tools for prior context.{status}")
}

fn tool_call_arguments(call: &serde_json::Value) -> serde_json::Value {
    let raw = call.get("function").and_then(|f| f.get("arguments"));
    match raw {
        None => json!({}),
        Some(serde_json::Value::String(s)) => {
            serde_json::from_str::<serde_json::Value>(s).unwrap_or_else(|_| json!({}))
        }
        Some(v) => v.clone(),
    }
}

/// Default container path for git (`repo_path`) and filesystem MCP tools (`path`): `/app/<label>`
/// from workspace roots + host cwd. Override with `PENGINE_MCP_CONTAINER_ROOT` or `PENGINE_GIT_REPO_PATH`.
async fn default_workspace_container_path(state: &AppState) -> Option<String> {
    for key in ["PENGINE_MCP_CONTAINER_ROOT", "PENGINE_GIT_REPO_PATH"] {
        if let Ok(p) = std::env::var(key) {
            let t = p.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }

    let roots = state.cached_filesystem_paths.read().await.clone();
    if roots.is_empty() {
        return None;
    }

    let pairs = workspace_app_bind_pairs(&roots);
    let cwd = std::env::current_dir().ok()?;
    let cwd_canon = std::fs::canonicalize(&cwd).unwrap_or(cwd);

    for (host, container) in &pairs {
        let hp = Path::new(host.trim());
        let hcanon = std::fs::canonicalize(hp).unwrap_or_else(|_| hp.to_path_buf());
        if cwd_canon.starts_with(&hcanon) {
            return Some(container.clone());
        }
    }

    pairs.first().map(|(_, c)| c.clone())
}

/// Ollama exposes tools as `server_key.tool_name`; merge helpers must match on the base name.
fn mcp_tool_base_name(full_name: &str) -> &str {
    full_name
        .rsplit_once('.')
        .map(|(_, tail)| tail)
        .unwrap_or(full_name)
}

/// User wants **real edits** (not just review prose): pre-commit, formatters, linters, "apply fix", etc.
fn message_implies_apply_repo_fix(msg: &str) -> bool {
    const HINTS: &[&str] = &[
        "pre-commit",
        "precommit",
        "lint-staged",
        "lint staged",
        "husky",
        "rustfmt",
        "cargo fmt",
        "clippy",
        "cargo clippy",
        "eslint",
        "prettier",
        "fix the issue",
        "fix pre-commit",
        "fix precommit",
        "apply the fix",
        "apply fixes",
        "edit the file",
        "edit the files",
        "make the change",
        "update the file",
        "patch the file",
    ];
    HINTS
        .iter()
        .any(|h| skills::user_message_needle_match(msg, h))
}

/// Lint/fix flows plus explicit create/write/scaffold requests (`.pengine`, new files, etc.).
fn user_expects_repo_write_followthrough(msg: &str) -> bool {
    message_implies_apply_repo_fix(msg)
        || crate::modules::mcp::registry::message_suggests_filesystem_mutation(msg)
}

fn tool_invocation_writes_files(model_tool_name: &str) -> bool {
    let b = mcp_tool_base_name(model_tool_name);
    b.eq_ignore_ascii_case("edit_file")
        || b.eq_ignore_ascii_case("write_file")
        || b.eq_ignore_ascii_case("create_directory")
        || b.eq_ignore_ascii_case("move_file")
}

/// True when tool output still looks like an unresolved Rust/clippy/pre-commit failure.
/// Used to avoid ending the apply-fix loop on prose-only or meta replies while the last
/// command output is still red.
fn tool_output_implies_unresolved_failure(body: &str) -> bool {
    let b = body;
    b.contains("could not compile")
        || b.contains("clippy::")
        || b.contains("exited with code 101")
        || b.contains("script \"rust:lint\" exited")
        || b.contains("pre-commit script failed")
        || b.contains("husky - pre-commit")
        || b.contains("error[E0")
        || (b.contains("error: ") && b.contains("-->") && b.contains(".rs:"))
}

/// Apply-fix turn is unfinished if we never wrote, or the **latest** tool payload still
/// looks like a failing check (so we do not keep looping after a successful `edit_file`
/// just because an older clippy blob remains earlier in `tool_results`).
fn apply_fix_turn_unfinished(tool_results: &[(String, String)], all_invoked: &[String]) -> bool {
    let wrote = all_invoked.iter().any(|n| tool_invocation_writes_files(n));
    let last_failed = tool_results
        .last()
        .is_some_and(|(_, body)| tool_output_implies_unresolved_failure(body));
    !wrote || last_failed
}

/// Merged into `directory_tree` / `search_files` when absent or empty — keeps scans inside host
/// RPC timeouts (a `search_files` over a `node_modules`-heavy repo otherwise hits the 90s ceiling).
/// Patterns follow `@modelcontextprotocol/server-filesystem` (minimatch on paths under `path`).
const DIRECTORY_TREE_DEFAULT_EXCLUDES: &[&str] = &[
    "**/node_modules/**",
    "**/.git/**",
    "**/target/**",
    "**/dist/**",
    "**/build/**",
    "**/.next/**",
    "**/__pycache__/**",
    "**/.cache/**",
    "**/coverage/**",
    "**/.turbo/**",
    "**/Pods/**",
    "**/.venv/**",
    "**/venv/**",
];

fn merge_directory_tree_exclude_patterns(map: &mut serde_json::Map<String, serde_json::Value>) {
    let user_nonempty: Option<Vec<String>> = match map.get("excludePatterns") {
        Some(serde_json::Value::Array(a)) if !a.is_empty() => {
            let v: Vec<String> = a
                .iter()
                .filter_map(|it| it.as_str().map(str::trim).filter(|s| !s.is_empty()))
                .map(std::string::ToString::to_string)
                .collect();
            if v.is_empty() {
                None
            } else {
                Some(v)
            }
        }
        _ => None,
    };

    let mut merged: HashSet<String> = DIRECTORY_TREE_DEFAULT_EXCLUDES
        .iter()
        .map(|s| (*s).to_string())
        .collect();
    if let Some(user) = user_nonempty {
        for s in user {
            merged.insert(s);
        }
    }
    map.insert(
        "excludePatterns".into(),
        serde_json::Value::Array(merged.into_iter().map(|s| json!(s)).collect()),
    );
}

/// Official MCP filesystem tools (`@modelcontextprotocol/server-filesystem`) require `path` under a
/// configured root. Models often call `directory_tree` with `{}`; fill `path` with the same default
/// mount as [`default_workspace_container_path`].
fn merge_filesystem_mcp_path_args(
    tool_full_name: &str,
    args: serde_json::Value,
    default_path: Option<&str>,
) -> serde_json::Value {
    let base = mcp_tool_base_name(tool_full_name);
    const DIR_TOOLS: &[&str] = &[
        "directory_tree",
        "list_directory",
        "list_directory_with_sizes",
        "search_files",
    ];
    if !DIR_TOOLS.iter().any(|t| base.eq_ignore_ascii_case(t)) {
        return args;
    }
    let Some(default) = default_path.map(str::trim).filter(|s| !s.is_empty()) else {
        return args;
    };

    let inject_excludes =
        base.eq_ignore_ascii_case("directory_tree") || base.eq_ignore_ascii_case("search_files");

    let mut map = match args {
        serde_json::Value::Object(m) => m,
        serde_json::Value::Null => serde_json::Map::new(),
        _ => {
            if inject_excludes {
                let mut m = serde_json::Map::new();
                m.insert("path".into(), json!(default));
                merge_directory_tree_exclude_patterns(&mut m);
                return serde_json::Value::Object(m);
            }
            return json!({ "path": default });
        }
    };

    let needs_fill = match map.get("path") {
        None => true,
        Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.trim().is_empty(),
        _ => false,
    };
    if needs_fill {
        map.insert("path".into(), json!(default));
    }
    if inject_excludes {
        merge_directory_tree_exclude_patterns(&mut map);
    }
    serde_json::Value::Object(map)
}

fn merge_git_repo_path_args(
    tool_full_name: &str,
    args: serde_json::Value,
    default_repo: Option<&str>,
) -> serde_json::Value {
    let base = mcp_tool_base_name(tool_full_name);
    if !base.starts_with("git_") {
        return args;
    }
    let Some(default) = default_repo.map(str::trim).filter(|s| !s.is_empty()) else {
        return args;
    };

    let mut map = match args {
        serde_json::Value::Object(m) => m,
        serde_json::Value::Null => serde_json::Map::new(),
        _ => {
            return json!({ "repo_path": default });
        }
    };

    let needs_fill = match map.get("repo_path") {
        None => true,
        Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.trim().is_empty(),
        _ => false,
    };
    if needs_fill {
        map.insert("repo_path".into(), json!(default));
    }
    serde_json::Value::Object(map)
}

fn fmt_duration(d: Duration) -> String {
    if d.as_millis() < 1000 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.1}s", d.as_secs_f64())
    }
}

fn fmt_tokens(prompt: Option<u64>, eval: Option<u64>) -> String {
    match (prompt, eval) {
        (None, None) => String::new(),
        (p, e) => {
            let p = p.map(|n| n.to_string()).unwrap_or_else(|| "?".into());
            let e = e.map(|n| n.to_string()).unwrap_or_else(|| "?".into());
            format!(" (in:{p} out:{e})")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplySource {
    Model,
    Tool,
}

pub struct TurnResult {
    pub text: String,
    pub source: ReplySource,
    pub suppress_telegram_reply: bool,
    pub prompt_tokens: u64,
    pub eval_tokens: u64,
    pub model: String,
}

impl TurnResult {
    fn reply(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
            prompt_tokens: 0,
            eval_tokens: 0,
            model: String::new(),
        }
    }

    fn suppressed() -> Self {
        Self {
            text: String::new(),
            source: ReplySource::Model,
            suppress_telegram_reply: true,
            prompt_tokens: 0,
            eval_tokens: 0,
            model: String::new(),
        }
    }
}

// ── Public entry point ─────────────────────────────────────────────

/// Run one model turn for a system-originated prompt (e.g. cron job).
/// Bypasses memory-session routing so scheduled tasks never land in diary/session
/// logs, and never trigger session start/stop keywords.
///
/// `skills_slug_filter`: when [`Some`] and non-empty, only those skills are included in the
/// system prompt; when [`None`] or empty slice, all enabled skills are included.
pub async fn run_system_turn(
    state: &AppState,
    prompt: &str,
    skills_slug_filter: Option<&[String]>,
) -> Result<TurnResult, String> {
    let think = decide_think(None, prompt).enabled();
    let result = run_model_turn(state, prompt, think, skills_slug_filter).await?;
    let body = result.text.trim();
    if !body.is_empty() {
        let tag = match result.source {
            ReplySource::Tool => "tool",
            ReplySource::Model => "model",
        };
        state
            .emit_log("reply", &format!("[cron:{tag}] {body}"))
            .await;
    } else {
        state.emit_log("reply", "[cron] (empty reply)").await;
    }
    Ok(result)
}

pub async fn run_turn(state: &AppState, user_message: &str) -> Result<TurnResult, String> {
    let (think_override, user_message) = parse_think_override(user_message);

    if let Some(cmd) = memory::detect_session_command(user_message) {
        return match cmd {
            SessionCommand::Start => handle_recording_start(state, false).await,
            SessionCommand::DiaryStart => handle_recording_start(state, true).await,
            SessionCommand::End => handle_recording_end(state, false).await,
            SessionCommand::DiaryEnd => handle_recording_end(state, true).await,
        };
    }

    if let Some(s) = state.memory_session.read().await.clone() {
        if s.diary_only {
            return handle_diary_line(state, user_message).await;
        }
    }

    let think = decide_think(think_override, user_message);
    state
        .emit_log("run", &format!("think:{}", think.label()))
        .await;

    let result = run_model_turn(state, user_message, think.enabled(), None).await?;
    spawn_memory_save(state, user_message, &result.text).await;
    Ok(result)
}

// ── Memory recording handlers ──────────────────────────────────────

async fn handle_recording_start(state: &AppState, diary: bool) -> Result<TurnResult, String> {
    let Some(memory) = MemoryProvider::detect(&*state.mcp.read().await) else {
        return Ok(TurnResult::reply(
            "No memory server is connected. Install a memory tool in Dashboard → MCP Tools first.",
        ));
    };

    if let Some(existing) = state.memory_session.read().await.clone() {
        let msg = if existing.diary_only == diary {
            format!(
                "Already recording as `{}`. Say \"over and out\" to end it.",
                existing.entity_name
            )
        } else if existing.diary_only {
            "Diary recording is active — say \"record end\" or \"over and out\" first.".into()
        } else {
            "A chat session is active — say \"close session\" or \"over and out\" first.".into()
        };
        return Ok(TurnResult::reply(msg));
    }

    let now = Utc::now();
    let prefix = if diary { "diary" } else { "session" };
    let entity_name = memory::entity_name(prefix, now);
    let description = if diary {
        format!(
            "Diary opened at {} UTC (user lines only).",
            now.format("%Y-%m-%d %H:%M:%S")
        )
    } else {
        format!(
            "Chat session opened at {} UTC.",
            now.format("%Y-%m-%d %H:%M:%S")
        )
    };

    if let Err(e) = memory.start_session(&entity_name, &description).await {
        state
            .emit_log("memory", &format!("failed to open {prefix}: {e}"))
            .await;
        return Ok(TurnResult::reply(format!("Could not open {prefix}: {e}")));
    }

    *state.memory_session.write().await = Some(MemorySession {
        entity_name: entity_name.clone(),
        turn_count: 0,
        diary_only: diary,
    });

    state
        .emit_log(
            "memory",
            &format!("{prefix} opened on {}: {entity_name}", memory.server_name()),
        )
        .await;

    let reply = if diary {
        format!(
            "Diary recording started (`{entity_name}`). Your lines are saved silently. \
             Say \"record end\" or \"over and out\" to stop."
        )
    } else {
        format!(
            "Captain's log opened as `{entity_name}`. Every message is saved to memory. \
             Say \"close session\", \"end log\", or \"Commander <name> out\" to close it."
        )
    };
    Ok(TurnResult::reply(reply))
}

async fn handle_recording_end(state: &AppState, diary_only: bool) -> Result<TurnResult, String> {
    let session = {
        let mut guard = state.memory_session.write().await;
        match guard.as_ref() {
            None => {
                return Ok(TurnResult::reply("No memory recording is active."));
            }
            Some(s) if diary_only && !s.diary_only => {
                return Ok(TurnResult::reply(
                    "Not in diary mode — say \"close session\" or \"over and out\" to end the chat session.",
                ));
            }
            _ => guard.take().unwrap(),
        }
    };

    if let Some(memory) = MemoryProvider::detect(&*state.mcp.read().await) {
        let kind = if session.diary_only {
            "Diary"
        } else {
            "Session"
        };
        let note = format!(
            "{kind} closed at {} UTC after {} turn(s).",
            Utc::now().format("%Y-%m-%d %H:%M:%S"),
            session.turn_count
        );
        if let Err(e) = memory.append(&session.entity_name, &note).await {
            state
                .emit_log("memory", &format!("close note not saved: {e}"))
                .await;
        }
    }

    let kind = if session.diary_only {
        "diary"
    } else {
        "session"
    };
    state
        .emit_log(
            "memory",
            &format!(
                "{kind} closed: {} ({} turn(s))",
                session.entity_name, session.turn_count
            ),
        )
        .await;

    Ok(TurnResult::reply(format!(
        "Memory {kind} `{}` closed after {} turn(s).",
        session.entity_name, session.turn_count
    )))
}

async fn handle_diary_line(state: &AppState, user_message: &str) -> Result<TurnResult, String> {
    let Some(session) = state.memory_session.read().await.clone() else {
        return Ok(TurnResult::reply(
            "Diary ended; send \"record\" to start a new one.",
        ));
    };
    let Some(mem) = MemoryProvider::detect(&*state.mcp.read().await) else {
        *state.memory_session.write().await = None;
        return Ok(TurnResult::reply(
            "Memory server disconnected — diary stopped.",
        ));
    };

    spawn_append(
        state,
        &mem,
        &session.entity_name,
        format!("[diary] {user_message}"),
    )
    .await;
    Ok(TurnResult::suppressed())
}

// ── Background memory persistence ──────────────────────────────────

async fn spawn_memory_save(state: &AppState, user_message: &str, reply: &str) {
    let Some(session) = state.memory_session.read().await.clone() else {
        return;
    };
    if session.diary_only {
        return;
    }
    let Some(mem) = MemoryProvider::detect(&*state.mcp.read().await) else {
        state
            .emit_log(
                "memory",
                &format!(
                    "session `{}` active but no memory server — turn dropped",
                    session.entity_name
                ),
            )
            .await;
        return;
    };

    let content = format!("[user] {user_message}\n[assistant] {reply}");
    spawn_append(state, &mem, &session.entity_name, content).await;
}

async fn spawn_append(state: &AppState, mem: &MemoryProvider, entity: &str, content: String) {
    let state_bg = state.clone();
    let entity = entity.to_string();
    let mem_server = mem.provider_clone();
    tokio::spawn(async move {
        let mem = mem_server;
        match mem.append(&entity, &content).await {
            Ok(()) => {
                if let Some(s) = state_bg.memory_session.write().await.as_mut() {
                    if s.entity_name == entity {
                        s.turn_count += 1;
                    }
                }
            }
            Err(e) => {
                state_bg
                    .emit_log("memory", &format!("append to `{entity}` failed: {e}"))
                    .await;
            }
        }
    });
}

// ── Model turn with tool loop ──────────────────────────────────────

/// Assemble the static assistant preamble plus fs/memory/skills fragments.
/// The order is stable turn-to-turn so Ollama can reuse its KV-cache prefix.
async fn build_system_prompt(
    state: &AppState,
    user_message: &str,
    has_tools: bool,
    has_memory: bool,
    skills_slug_filter: Option<&[String]>,
) -> String {
    if !has_tools {
        return format!("{PENGINE_OUTPUT_CONTRACT_LEAD}Answer concisely.");
    }

    let fs_hint = {
        // Gate on whether the filesystem MCP server is actually connected — checking
        // `workspace_roots` alone wrongly advertises `directory_tree`/`list_directory`
        // when the file-manager Tool Engine is not installed, leading the model to
        // hallucinate calls that resolve to `tool not found`.
        let has_fs_tool = {
            let reg = state.mcp.read().await;
            reg.tool_names().iter().any(|n| {
                let short = n.rsplit_once('.').map(|(_, t)| t).unwrap_or(n.as_str());
                matches!(
                    short,
                    "directory_tree"
                        | "list_directory"
                        | "list_directory_with_sizes"
                        | "read_text_file"
                        | "search_files"
                )
            })
        };
        if !has_fs_tool {
            String::new()
        } else {
            let paths = state.cached_filesystem_paths.read().await.clone();
            let mounts_line = if paths.is_empty() {
                String::new()
            } else {
                let mounts = workspace_app_bind_pairs(&paths)
                    .iter()
                    .map(|(_, cpath)| cpath.clone())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("\nFile tools: use /app/… paths only. Mounts: {mounts}.")
            };
            let discipline = if paths.is_empty() {
                "\nFilesystem MCP: tools are functions — include **every** required argument from the schema (never `{}` when fields are required). For `directory_tree`, pass mandatory `path` as an absolute `/app/<folder>` under a configured mount. Recursive trees over large repos time out: prefer **`list_directory`** / **`search_files`** on a narrow path, or pass **`excludePatterns`** (e.g. `**/node_modules/**`, `**/.git/**`, `**/target/**`)."
                    .to_string()
            } else {
                let example = workspace_app_bind_pairs(&paths)
                    .first()
                    .map(|(_, c)| c.clone())
                    .unwrap_or_else(|| "/app/<folder>".into());
                format!(
                    "\nFilesystem MCP: include every required argument. For **`directory_tree`**, set **`path`** to an absolute mount root (example: `{example}`); avoid scanning the whole repo at once — use **`excludePatterns`** (`**/node_modules/**`, `**/.git/**`, …) or **`list_directory`** / **`search_files`** on subpaths first. \
                     For **`git_*`** tools (git repo in container), set **`repo_path`** to that same mount root when the schema requires it (example: `{example}`)."
                )
            };
            format!("{mounts_line}{discipline}")
        }
    };

    // Code-change hint: when the user asks for a review, fix, refactor, or any change to
    // files in the repo, apply edits with `edit_file`/`write_file` and end with a `git_diff`
    // embedded inside `<pengine_reply>`. Without this, the model defaults to a static
    // markdown summary even when write+diff tools are in the catalog.
    let code_edit_hint = {
        let reg = state.mcp.read().await;
        let names = reg.tool_names();
        let has_short = |target: &str| {
            names.iter().any(|n| {
                let short = n.rsplit_once('.').map(|(_, t)| t).unwrap_or(n.as_str());
                short.eq_ignore_ascii_case(target)
            })
        };
        let has_edit = has_short("edit_file") || has_short("write_file");
        let has_diff = has_short("git_diff")
            || has_short("git_diff_unstaged")
            || has_short("git_diff_staged")
            || has_short("git_status");
        let base = if has_edit && has_diff {
            "\nCode changes: when the user asks for a review with changes, a fix, a refactor, or any \
             modification to repo files, **apply the change yourself** with **`edit_file`** / **`write_file`** \
             — do not write a markdown summary describing what to change. After editing, call **`git_diff`** \
             (unstaged) and embed the diff inside `<pengine_reply>` as a fenced ```diff block, followed by a \
             brief one-line rationale per file. If the user only asked for a review without changes, answer \
             with prose only and do not edit."
        } else if has_edit {
            "\nCode changes: when the user asks for changes/fixes/refactors, apply them yourself with \
             **`edit_file`** / **`write_file`** instead of describing them. End with a short bullet list of the \
             files you changed."
        } else {
            ""
        };
        let apply_fix = if message_implies_apply_repo_fix(user_message) && has_edit {
            "\n**Apply-fix order (pre-commit / lint / format):** (1) read or search if needed; (2) **you must \
             call `edit_file` or `write_file` at least once** using an absolute `/app/…` path; (3) then \
             **`git_diff`** (unstaged) to verify. Showing **`git_diff`** alone after **`cargo fmt`** or other \
             tools ran **outside** the model is not enough — if the user asked to fix pre-commit, **you** must \
             write the corrected file contents via tools before finishing. If **`cargo clippy`**, **`rust:lint`**, \
             or **husky/pre-commit** output shows errors, your **very next** step is always an **edit** tool on \
             the file and line cited — never stop with meta text about response formats."
        } else {
            ""
        };
        format!("{base}{apply_fix}")
    };

    let mem_hint = if has_memory {
        let snap = state.memory_session.read().await;
        memory_hint(
            snap.as_ref().map(|s| s.entity_name.as_str()),
            snap.as_ref().is_some_and(|s| s.diary_only),
        )
    } else {
        String::new()
    };

    let weather_directive = if skills::user_message_suggests_weather(user_message) {
        "\n\n**This turn is weather-related:** Follow **skill:weather** only (wttr.in / Open-Meteo via **`fetch`**). Do not cite or prioritize government-portal skills (e.g. oesterreich.gv.at) for forecasts or current conditions unless the user explicitly asked for public administration, forms, or law."
    } else {
        ""
    };

    let skills_raw = skills::skills_prompt_hint_for_turn(
        &state.store_path,
        Some(user_message),
        skills_slug_filter,
    );
    let skills_cap = *state.skills_hint_max_bytes.read().await as usize;
    let (skills_hint, skills_truncated) = skills::limit_skills_hint_bytes(skills_raw, skills_cap);
    if skills_truncated {
        state
            .emit_log(
                "run",
                &format!(
                    "skills hint truncated to {} bytes (cap {})",
                    skills_hint.len(),
                    skills_cap
                ),
            )
            .await;
    }

    let scaffold_hint = {
        let has_edit = {
            let reg = state.mcp.read().await;
            reg.tool_names().iter().any(|n| {
                let short = n.rsplit_once('.').map(|(_, t)| t).unwrap_or(n.as_str());
                short.eq_ignore_ascii_case("edit_file") || short.eq_ignore_ascii_case("write_file")
            })
        };
        if has_edit
            && crate::modules::mcp::registry::message_suggests_filesystem_mutation(user_message)
        {
            "\n**On-disk project metadata:** When the user asks for a **new file**, **folder**, or dot-directory (e.g. **`.pengine`**) for session or project context, **create it in this turn** with **`create_directory`** / **`write_file`** / **`edit_file`** under an absolute **`/app/…`** path. Do not answer with only a generic offer to help before those paths exist."
        } else {
            ""
        }
    };

    format!(
        "{PENGINE_OUTPUT_CONTRACT_LEAD}Assistant with tools. Use tools to fetch external data **or to act on the user's repository (read, edit, diff, commit)**; otherwise answer directly. \
         After tool results, answer immediately. Be concise. \
         `brave_web_search` is only in the tool list when the user asked to search the open web (e.g. \"search the internet\", \"suche im Internet\", \"suche nach ...\") or a skill's `requires` matches this turn — otherwise prefer **`fetch`** on any `http(s)` URL you have (including from the user). \
         At most one `brave_web_search` per user message when it is available. \
         After an allowed search, the host may auto-`fetch` several top result URLs — use those excerpts and end with **Quellen** listing every source URL.{fs_hint}{code_edit_hint}{scaffold_hint}{mem_hint}{weather_directive}{skills_hint}"
    )
}

async fn run_model_turn(
    state: &AppState,
    user_message: &str,
    think: bool,
    skills_slug_filter: Option<&[String]>,
) -> Result<TurnResult, String> {
    let plan_mode = *state.plan_mode.read().await;
    let repo_write_followthrough = user_expects_repo_write_followthrough(user_message);
    let max_steps = if repo_write_followthrough {
        MAX_STEPS_APPLY_FIX
    } else {
        MAX_STEPS
    };
    let mut model = match state.preferred_ollama_model.read().await.clone() {
        Some(m) => m,
        None => ollama::active_model().await?,
    };
    let mut tokens_in: u64 = 0;
    let mut tokens_out: u64 = 0;

    let registry_has_write_tool = {
        let reg = state.mcp.read().await;
        reg.tool_names().iter().any(|n| {
            let short = mcp_tool_base_name(n);
            short.eq_ignore_ascii_case("edit_file")
                || short.eq_ignore_ascii_case("write_file")
                || short.eq_ignore_ascii_case("create_directory")
        })
    };

    let recent_tools = state.recent_tools_snapshot().await;
    let (has_tools, has_memory, memory_server_key) = {
        let reg = state.mcp.read().await;
        let mem = MemoryProvider::detect(&reg);
        (
            !reg.is_empty(),
            mem.is_some(),
            mem.map(|m| m.server_name().to_string()),
        )
    };

    let chat_session_recording = state
        .memory_session
        .read()
        .await
        .as_ref()
        .is_some_and(|s| !s.diary_only);
    // CLI sessions should consistently have memory tools available when a
    // memory provider exists, not only after explicit memory keywords.
    let cli_session_active = state.cli_session.read().await.is_some();

    let allow_brave_web_search =
        skills::allow_brave_web_search_for_message(&state.store_path, user_message);

    let mut tool_ctx = {
        let reg = state.mcp.read().await;
        reg.select_tools_for_turn(
            user_message,
            &recent_tools,
            memory_server_key.as_deref(),
            chat_session_recording || cli_session_active,
            allow_brave_web_search,
        )
    };
    if plan_mode {
        plan_mode_filter_writes(&mut tool_ctx.tools_json);
    }
    state
        .emit_log(
            "tool_ctx",
            &format!(
                "select_ms={} active={}/{} subset={} routing={} recording={} high_risk={} recent_n={} brave_web={}",
                tool_ctx.select_ms,
                tool_ctx.active_count,
                tool_ctx.total_count,
                tool_ctx.used_subset,
                tool_ctx.routing,
                chat_session_recording,
                tool_ctx.high_risk_active,
                recent_tools.len(),
                allow_brave_web_search
            ),
        )
        .await;
    state.record_tool_selection_ms(tool_ctx.select_ms).await;

    let mut system = build_system_prompt(
        state,
        user_message,
        has_tools,
        has_memory,
        skills_slug_filter,
    )
    .await;
    if plan_mode {
        system.push_str(
            "\n\nPLAN MODE: You are in read-only planning mode. Do NOT call tools that modify state \
             (memory writes, fs writes, append, edit, create). Produce a numbered, markdown plan that the \
             user can review and apply. End with a one-line summary of expected impact.",
        );
    }

    // Order matters for Ollama KV-cache reuse across turns: system message
    // first, user second. Changing fragment order would invalidate the cached
    // prefix between turns even when the content is identical.
    let mut messages = json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user_message }
    ]);

    let mut tool_results: Vec<(String, String)> = Vec::new();
    let mut tools_supported = true;
    let empty_tools = json!([]);
    let mut routing_escalated = false;
    let mut brave_web_search_calls_this_message: u32 = 0;
    // Counts actual tool-result rounds, not loop iterations. A routing escalation
    // re-enters step 0 with a fresh catalog, so it must not be treated as a
    // post-tool continuation (no reminder, keep user's think/num_predict).
    let mut tool_rounds: usize = 0;
    // URLs already fetched successfully this user message (model + host auto-fetch).
    let mut fetch_urls_success: HashSet<String> = HashSet::new();
    let mut all_invoked_tools: Vec<String> = Vec::new();
    let mut fix_write_nudge_sent = false;

    for step in 0..max_steps {
        let t0 = Instant::now();
        let effective_tools = if tools_supported {
            &tool_ctx.tools_json
        } else {
            &empty_tools
        };
        let post_tool = tool_rounds > 0;
        let json_only_user_reply = !has_tools;
        let chat_opts = chat_options_for_agent_step(
            post_tool,
            think,
            json_only_user_reply,
            repo_write_followthrough,
        );

        let inject_post_tool = post_tool;
        if inject_post_tool {
            push_ephemeral_post_tool_reminder(&mut messages);
        }

        let result =
            chat_with_cloud_fallback(state, &mut model, &messages, effective_tools, &chat_opts)
                .await;
        if inject_post_tool {
            pop_ephemeral_post_tool_reminder(&mut messages);
        }
        let result = result?;
        let tokens = fmt_tokens(result.prompt_tokens, result.eval_tokens);
        tokens_in = tokens_in.saturating_add(result.prompt_tokens.unwrap_or(0));
        tokens_out = tokens_out.saturating_add(result.eval_tokens.unwrap_or(0));
        let msg = result.message;

        if !result.tools_sent && tools_supported {
            tools_supported = false;
            state
                .emit_log("tool", &format!("{model} does not support tools"))
                .await;
        }
        state
            .emit_log(
                "time",
                &format!("model step {step} {}{tokens}", fmt_duration(t0.elapsed())),
            )
            .await;

        let tool_calls = msg
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let content = msg
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if step == 0
            && !routing_escalated
            && tool_ctx.used_subset
            && tool_calls.is_empty()
            && content.is_empty()
        {
            routing_escalated = true;
            tool_ctx = {
                let reg = state.mcp.read().await;
                reg.full_tool_context(allow_brave_web_search)
            };
            state
                .emit_log(
                    "tool_ctx",
                    &format!("escalate full catalog ({} tools)", tool_ctx.active_count),
                )
                .await;
            continue;
        }

        if let Some(arr) = messages.as_array_mut() {
            arr.push(msg);
        }

        if tool_calls.is_empty() {
            if !content.is_empty() {
                if repo_write_followthrough
                    && !plan_mode
                    && registry_has_write_tool
                    && step + 1 < max_steps
                    && apply_fix_turn_unfinished(&tool_results, &all_invoked_tools)
                {
                    if let Some(arr) = messages.as_array_mut() {
                        arr.push(json!({
                            "role": "system",
                            "content": REPO_WRITE_CONTINUE_AFTER_PROSE,
                        }));
                    }
                    state
                        .emit_log(
                            "run",
                            "agent: repo-write continuation (prose-only while incomplete)",
                        )
                        .await;
                    continue;
                }
                let mut r = TurnResult::reply(content);
                r.prompt_tokens = tokens_in;
                r.eval_tokens = tokens_out;
                r.model = model.clone();
                return Ok(r);
            }
            if tool_results.is_empty() {
                let mut r = TurnResult::reply("");
                r.prompt_tokens = tokens_in;
                r.eval_tokens = tokens_out;
                r.model = model.clone();
                return Ok(r);
            }
            if repo_write_followthrough
                && !plan_mode
                && registry_has_write_tool
                && step + 1 < max_steps
                && apply_fix_turn_unfinished(&tool_results, &all_invoked_tools)
            {
                if let Some(arr) = messages.as_array_mut() {
                    arr.push(json!({
                        "role": "system",
                        "content": REPO_WRITE_CONTINUE_AFTER_EMPTY,
                    }));
                }
                state
                    .emit_log(
                        "run",
                        "agent: repo-write continuation (silent model step after tools)",
                    )
                    .await;
                continue;
            }
            break;
        }

        state
            .emit_log("tool", &format!("{} tool call(s)", tool_calls.len()))
            .await;

        let workspace_mount_default = default_workspace_container_path(state).await;

        // Resolve under one lock, then execute in parallel (per-server stdio is serialized by
        // [`crate::modules::mcp::transport::StdioTransport::rpc_mutex`]).
        let mut prepared = {
            let reg = state.mcp.read().await;
            tool_calls
                .iter()
                .map(|call| {
                    let name = call
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let raw = tool_call_arguments(call);
                    let args = merge_filesystem_mcp_path_args(
                        &name,
                        merge_git_repo_path_args(&name, raw, workspace_mount_default.as_deref()),
                        workspace_mount_default.as_deref(),
                    );
                    let resolved = reg.prepare_tool_invocation(&name, args);
                    (name, resolved)
                })
                .collect::<Vec<_>>()
        };

        for (name, res) in &mut prepared {
            if !name.eq_ignore_ascii_case("brave_web_search") {
                continue;
            }
            if res.is_err() {
                continue;
            }
            if brave_web_search_calls_this_message >= MAX_BRAVE_WEB_SEARCH_PER_USER_MESSAGE {
                *res = Err(BRAVE_WEB_SEARCH_LIMIT_MSG.to_string());
            } else {
                brave_web_search_calls_this_message += 1;
            }
        }

        {
            let mut batch_fetch_keys = HashSet::<String>::new();
            for (name, res) in prepared.iter_mut() {
                if !tool_name_is_fetch(name) {
                    continue;
                }
                let Ok((_, _, _, ref args)) = res else {
                    continue;
                };
                let Some(raw) = fetch_url_from_tool_args(args) else {
                    continue;
                };
                let key = fetch_url_dedup_key(&raw);
                if fetch_urls_success.contains(&key) || !batch_fetch_keys.insert(key) {
                    *res = Err(FETCH_DUPLICATE_URL_MSG.to_string());
                }
            }
        }

        let invoked_names: Vec<String> = prepared.iter().map(|(n, _)| n.clone()).collect();
        all_invoked_tools.extend(invoked_names.iter().cloned());
        state.note_tools_used(&invoked_names).await;

        let t0 = Instant::now();
        // `task_spawn` recursively calls into [`run_model_turn`], whose future is `!Send`.
        // Mixing it into the parallel `tokio::spawn` pool would fail to compile, so we run
        // task-spawner calls serially inline before joining the spawned handles.
        let mut results: Vec<Result<String, String>> =
            (0..prepared.len()).map(|_| Err(String::new())).collect();
        let mut spawned: Vec<(usize, tokio::task::JoinHandle<Result<String, String>>)> =
            Vec::with_capacity(prepared.len());
        for (i, (name, resolved)) in prepared.iter().enumerate() {
            state.emit_log("tool", &format!("[{step}] {name}")).await;
            match resolved {
                Ok((provider, tool_name, _, args)) => {
                    if matches!(
                        provider,
                        crate::modules::mcp::registry::Provider::Native(np)
                            if np.server_name == crate::modules::mcp::native::TASK_SPAWNER_ID
                    ) {
                        results[i] = run_task_spawn_inline(state, args).await;
                    } else {
                        let state_bg = state.clone();
                        let display_name = name.clone();
                        let (p, tn, a) = (provider.clone(), tool_name.clone(), args.clone());
                        spawned.push((
                            i,
                            tokio::spawn(async move {
                                call_tool_with_mcp_recovery(&state_bg, &display_name, p, tn, a)
                                    .await
                            }),
                        ));
                    }
                }
                Err(e) => {
                    results[i] = Err(e.clone());
                }
            }
        }
        for (i, handle) in spawned {
            results[i] = match handle.await {
                Ok(r) => r,
                Err(e) => Err(format!("task panicked: {e}")),
            };
        }

        let mut direct_replies: Vec<String> = Vec::new();
        let mut last_brave_search_blob: Option<String> = None;
        for (i, result) in results.into_iter().enumerate() {
            let (name, resolved) = &prepared[i];
            let (text, is_direct) = match result {
                Ok(text) => {
                    let direct = resolved.as_ref().map(|(_, _, d, _)| *d).unwrap_or(false);
                    state
                        .emit_log("tool", &format!("{name}: {} bytes", text.len()))
                        .await;
                    (text, direct)
                }
                Err(e) => {
                    state.emit_log("tool", &format!("{name} error: {e}")).await;
                    (format!("ERROR: {e}"), false)
                }
            };

            // Fetch output is often XML/HTML snippets; never bypass the model for the user bubble
            // (even if `mcp.json` still has `direct_return: true` from an older catalog default).
            if is_direct && !tool_name_is_fetch(name) {
                direct_replies.push(text.clone());
            }
            let compacted = compact_tool_output(&text);
            let for_model = truncate_for_model(&compacted, TOOL_OUTPUT_CHAR_CAP);
            if tool_name_is_fetch(name) {
                if let Ok((_, _, _, args)) = resolved {
                    if let Some(raw) = fetch_url_from_tool_args(args) {
                        if !text.trim_start().starts_with("ERROR:") {
                            fetch_urls_success.insert(fetch_url_dedup_key(&raw));
                        }
                    }
                }
            }
            if name.eq_ignore_ascii_case("brave_web_search")
                && !for_model.trim_start().starts_with("ERROR:")
            {
                last_brave_search_blob = Some(for_model.clone());
            }
            if let Some(arr) = messages.as_array_mut() {
                arr.push(json!({ "role": "tool", "name": name, "content": &for_model }));
            }
            tool_results.push((name.clone(), for_model));
        }
        state
            .emit_log(
                "time",
                &format!("{} tool(s) {}", prepared.len(), fmt_duration(t0.elapsed())),
            )
            .await;
        tool_rounds += 1;

        if let Some(blob) = last_brave_search_blob {
            append_host_prefetch_after_brave_search(
                state,
                &mut messages,
                &mut tool_results,
                &blob,
                &mut fetch_urls_success,
            )
            .await;
        }

        if repo_write_followthrough
            && !plan_mode
            && registry_has_write_tool
            && !fix_write_nudge_sent
            && tool_rounds >= 1
            && step + 1 < max_steps
            && !all_invoked_tools
                .iter()
                .any(|n| tool_invocation_writes_files(n))
        {
            if let Some(arr) = messages.as_array_mut() {
                arr.push(json!({
                    "role": "system",
                    "content": "FIX REQUIRED: The user asked for **repository file or folder changes** (fixes, lint, pre-commit, **new files**, dot-directories like **`.pengine`**, etc.). \
                     You have not called **`edit_file`**, **`write_file`**, or **`create_directory`** yet. Call one now with the \
                     correct `/app/…` path. Do not finish with only **`git_diff`** / **`git_status`** / read or memory tools — create or patch paths on disk first."
                }));
            }
            fix_write_nudge_sent = true;
            state
                .emit_log(
                    "run",
                    "agent: injected repo-write nudge (no fs write tool yet)",
                )
                .await;
        }

        if !direct_replies.is_empty() {
            return Ok(TurnResult {
                text: direct_replies.join("\n\n"),
                source: ReplySource::Tool,
                suppress_telegram_reply: false,
                prompt_tokens: tokens_in,
                eval_tokens: tokens_out,
                model: model.clone(),
            });
        }
    }

    // Phase 2: summarize tool results if model didn't answer inline.
    if !tool_results.is_empty() {
        state
            .emit_log(
                "run",
                "agent: summarizing tool results (follow-up model step)",
            )
            .await;
        let mut data = String::new();
        for (name, content) in &tool_results {
            data.push_str(&format!("--- {name} ---\n{content}\n"));
        }

        let summary_messages = json!([
            { "role": "system", "content": SUMMARY_SYSTEM_PROMPT },
            { "role": "user", "content": format!("{user_message}\n\nData:\n{data}") }
        ]);

        let summary_opts = ChatOptions {
            think: Some(false),
            num_predict: Some(SUMMARY_NUM_PREDICT),
            temperature: Some(SUMMARY_TEMPERATURE),
            format: Some(ollama::summarize_reply_json_schema()),
            ..ChatOptions::default()
        };
        let t0 = Instant::now();
        let result = chat_with_cloud_fallback(
            state,
            &mut model,
            &summary_messages,
            &json!([]),
            &summary_opts,
        )
        .await?;
        let tokens = fmt_tokens(result.prompt_tokens, result.eval_tokens);
        tokens_in = tokens_in.saturating_add(result.prompt_tokens.unwrap_or(0));
        tokens_out = tokens_out.saturating_add(result.eval_tokens.unwrap_or(0));
        state
            .emit_log(
                "time",
                &format!("summarize {}{tokens}", fmt_duration(t0.elapsed())),
            )
            .await;

        let text = result
            .message
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if !text.trim().is_empty() {
            return Ok(TurnResult {
                text,
                source: ReplySource::Model,
                suppress_telegram_reply: false,
                prompt_tokens: tokens_in,
                eval_tokens: tokens_out,
                model: model.clone(),
            });
        }

        let fallback = tool_results.into_iter().last().unwrap().1;
        return Ok(TurnResult {
            text: fallback,
            source: ReplySource::Tool,
            suppress_telegram_reply: false,
            prompt_tokens: tokens_in,
            eval_tokens: tokens_out,
            model: model.clone(),
        });
    }

    Err(format!(
        "agent exceeded {max_steps} steps without finishing"
    ))
}

/// Strip tool entries whose name suggests state mutation (memory writes, fs
/// writes, edits, appends, deletes) so the plan-mode catalog stays read-only.
/// Operates in place on the JSON array produced by `select_tools_for_turn`.
///
/// Why a curated list, not free substring search: short fragments like `put`
/// or `post` collide with `output_*` / `compose_*` and would over-filter. The
/// list below sticks to verbs that unambiguously mean "mutates state" when
/// they appear as a token boundary in the tool name.
fn plan_mode_filter_writes(tools_json: &mut serde_json::Value) {
    const WRITE_TOKENS: &[&str] = &[
        "write", "edit", "append", "create", "delete", "remove", "patch", "update", "save",
        "rename", "move", "upsert", "insert", "destroy", "mutate", "replace",
    ];
    let Some(arr) = tools_json.as_array_mut() else {
        return;
    };
    arr.retain(|t| {
        let name = t
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if name.is_empty() {
            return true;
        }
        !contains_write_token(&name, WRITE_TOKENS)
    });
}

/// True when any of `tokens` appears in `name` at a name-component boundary
/// (start, end, or surrounded by `_` / `.` / `-`). Avoids false positives like
/// `output` containing `put`.
fn contains_write_token(name: &str, tokens: &[&str]) -> bool {
    let bytes = name.as_bytes();
    for tok in tokens {
        let needle = tok.as_bytes();
        if needle.is_empty() || bytes.len() < needle.len() {
            continue;
        }
        let mut i = 0;
        while i + needle.len() <= bytes.len() {
            if &bytes[i..i + needle.len()] == needle {
                let left_ok = i == 0 || matches!(bytes[i - 1], b'_' | b'.' | b'-');
                let right = i + needle.len();
                let right_ok = right == bytes.len() || matches!(bytes[right], b'_' | b'.' | b'-');
                if left_ok && right_ok {
                    return true;
                }
            }
            i += 1;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_mode_filter_drops_write_tools() {
        let mut v = json!([
            { "type": "function", "function": { "name": "fetch" } },
            { "type": "function", "function": { "name": "memory_create_entity" } },
            { "type": "function", "function": { "name": "fs_write" } },
            { "type": "function", "function": { "name": "brave_web_search" } },
            { "type": "function", "function": { "name": "te_provider.edit_file" } },
        ]);
        plan_mode_filter_writes(&mut v);
        let names: Vec<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["fetch", "brave_web_search"]);
    }

    #[test]
    fn plan_mode_filter_keeps_read_only_lookalikes() {
        // Names that contain write-verb substrings but not at a token boundary
        // must not be filtered out (e.g. `compose_*`, `output_*`).
        let mut v = json!([
            { "type": "function", "function": { "name": "compose_message" } },
            { "type": "function", "function": { "name": "output_format" } },
            { "type": "function", "function": { "name": "asset_lookup" } },
            { "type": "function", "function": { "name": "search_results" } },
        ]);
        plan_mode_filter_writes(&mut v);
        assert_eq!(v.as_array().unwrap().len(), 4);
    }

    #[test]
    fn think_prefix_parsed_and_stripped() {
        assert_eq!(
            parse_think_override("/think solve this"),
            (Some(true), "solve this")
        );
        assert_eq!(parse_think_override("  /nothink  hi"), (Some(false), "hi"));
        assert_eq!(parse_think_override("/think"), (Some(true), ""));
    }

    #[test]
    fn think_prefix_ignored_when_not_a_word_boundary() {
        assert_eq!(parse_think_override("/thinker"), (None, "/thinker"));
    }

    #[test]
    fn decide_think_precedence() {
        assert_eq!(decide_think(Some(true), "anything"), ThinkSource::SlashOn);
        assert_eq!(
            decide_think(Some(false), "think hard please"),
            ThinkSource::SlashOff
        );
        assert_eq!(
            decide_think(None, "think hard about this"),
            ThinkSource::Keyword
        );
        assert_eq!(
            decide_think(None, "what is the weather"),
            ThinkSource::Default
        );
    }

    #[test]
    fn think_source_enabled_maps_correctly() {
        assert!(ThinkSource::SlashOn.enabled());
        assert!(ThinkSource::Keyword.enabled());
        assert!(!ThinkSource::SlashOff.enabled());
        assert!(!ThinkSource::Default.enabled());
    }

    #[test]
    fn fetch_tool_name_detection() {
        assert!(tool_name_is_fetch("fetch"));
        assert!(tool_name_is_fetch("te_pengine-fetch.fetch"));
        assert!(!tool_name_is_fetch("roll_dice"));
    }

    #[test]
    fn fetch_url_dedup_key_ignores_fragment_and_trailing_slash() {
        assert_eq!(
            fetch_url_dedup_key("https://WWW.Example.COM/path/#frag"),
            fetch_url_dedup_key("https://www.example.com/path")
        );
        assert_eq!(
            fetch_url_dedup_key("https://a.example/page/"),
            fetch_url_dedup_key("HTTPS://A.EXAMPLE/page")
        );
    }

    #[test]
    fn directory_tree_merge_adds_excludes_for_prefixed_tool_name() {
        let out = merge_filesystem_mcp_path_args(
            "te_pengine-file-manager.directory_tree",
            json!({}),
            Some("/app/pengine"),
        );
        let obj = out.as_object().unwrap();
        assert_eq!(
            obj.get("path").and_then(|v| v.as_str()),
            Some("/app/pengine")
        );
        let ep = obj
            .get("excludePatterns")
            .and_then(|v| v.as_array())
            .expect("excludePatterns");
        assert!(ep.iter().any(|v| v.as_str() == Some("**/node_modules/**")));
    }

    #[test]
    fn list_directory_merge_matches_prefixed_server_tool_name() {
        let out = merge_filesystem_mcp_path_args("te_x.list_directory", json!({}), Some("/app/ws"));
        assert_eq!(out["path"], json!("/app/ws"));
    }

    #[test]
    fn search_files_merge_injects_default_excludes() {
        let out = merge_filesystem_mcp_path_args(
            "te_pengine-file-manager.search_files",
            json!({ "pattern": "TODO" }),
            Some("/app/pengine"),
        );
        let obj = out.as_object().unwrap();
        assert_eq!(
            obj.get("path").and_then(|v| v.as_str()),
            Some("/app/pengine")
        );
        assert_eq!(obj.get("pattern").and_then(|v| v.as_str()), Some("TODO"));
        let ep = obj
            .get("excludePatterns")
            .and_then(|v| v.as_array())
            .expect("excludePatterns");
        assert!(ep.iter().any(|v| v.as_str() == Some("**/node_modules/**")));
        assert!(ep.iter().any(|v| v.as_str() == Some("**/target/**")));
    }

    #[test]
    fn search_files_merge_preserves_user_excludes() {
        let out = merge_filesystem_mcp_path_args(
            "search_files",
            json!({ "path": "/app/pengine", "excludePatterns": ["**/private/**"] }),
            Some("/app/pengine"),
        );
        let ep = out["excludePatterns"].as_array().unwrap();
        assert!(ep.iter().any(|v| v.as_str() == Some("**/private/**")));
        assert!(ep.iter().any(|v| v.as_str() == Some("**/node_modules/**")));
    }

    #[test]
    fn message_implies_apply_repo_fix_detects_pre_commit_and_edit_phrases() {
        assert!(message_implies_apply_repo_fix(
            "fix pre-commit hook failure"
        ));
        assert!(message_implies_apply_repo_fix("run rustfmt and clippy"));
        assert!(message_implies_apply_repo_fix(
            "sure edit the files to fix lint"
        ));
        assert!(!message_implies_apply_repo_fix("what is the weather"));
    }

    #[test]
    fn user_expects_repo_write_followthrough_includes_scaffold_phrases() {
        assert!(user_expects_repo_write_followthrough(
            "create a hidden .pengine folder for session notes"
        ));
        assert!(!user_expects_repo_write_followthrough("what is 2+2"));
    }

    #[test]
    fn tool_invocation_writes_files_handles_qualified_names() {
        assert!(tool_invocation_writes_files("edit_file"));
        assert!(tool_invocation_writes_files("te_fm.edit_file"));
        assert!(tool_invocation_writes_files("srv.write_file"));
        assert!(!tool_invocation_writes_files("git_diff"));
        assert!(!tool_invocation_writes_files("read_text_file"));
    }

    #[test]
    fn tool_output_implies_unresolved_failure_detects_clippy_and_hook() {
        assert!(tool_output_implies_unresolved_failure(
            "error: could not compile `pengine` (lib) due to 1 previous error"
        ));
        assert!(tool_output_implies_unresolved_failure(
            "error: this match could be replaced by its body itself\n   --> src/modules/cli/output.rs:274:17"
        ));
        assert!(tool_output_implies_unresolved_failure(
            "husky - pre-commit script failed (code 101)"
        ));
        assert!(!tool_output_implies_unresolved_failure(
            "All checks passed."
        ));
    }

    #[test]
    fn apply_fix_turn_unfinished_needs_write_or_last_failure() {
        let clippy = (
            "run_terminal_cmd".into(),
            "error: could not compile `pengine` (lib)".into(),
        );
        assert!(apply_fix_turn_unfinished(
            std::slice::from_ref(&clippy),
            &[]
        ));
        assert!(!apply_fix_turn_unfinished(
            &[clippy.clone(), ("edit_file".into(), "ok".into())],
            &["edit_file".into()]
        ));
        assert!(apply_fix_turn_unfinished(
            &[("edit_file".into(), "ok".into()), clippy.clone()],
            &["edit_file".into()]
        ));
    }
}
