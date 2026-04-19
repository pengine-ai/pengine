use super::search_followup;
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
use std::time::{Duration, Instant};

/// Tool rounds + at least one completion-only step. Research flows (sitemap + several
/// `fetch` calls) otherwise exhaust the loop and fall through to summarize, which
/// used to drop URLs and paraphrase loosely.
const MAX_STEPS: usize = 6;

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

/// When the MCP catalog is empty and the user did not enable `/think`, constrain the model to JSON
/// `{\"reply\":...}` so the host can take a single user-visible field (same schema as the summarize pass).
fn chat_options_for_agent_step(
    post_tool: bool,
    user_wants_think: bool,
    json_only_user_reply: bool,
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
        ChatOptions {
            think: Some(false),
            num_predict: Some(POST_TOOL_NUM_PREDICT),
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

/// Run a chat call; if the request goes to a cloud model and the daemon
/// returns a rate-limit error, downgrade to the user's last local model and
/// retry once. The downgraded model is also written back to
/// `preferred_ollama_model` so the rest of the turn (and future turns) stay
/// local until the user picks again.
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
            if ollama::classify_model(model) != ollama::ModelKind::Cloud
                || !ollama::is_cloud_unavailable_error(&err)
            {
                return Err(err);
            }
            let last_local = state.last_local_model.read().await.clone();
            let catalog = ollama::model_catalog(3000).await.ok();
            let fallback = catalog
                .as_ref()
                .and_then(|c| ollama::pick_local_fallback(c, None, last_local.as_deref()));
            let Some(local) = fallback else {
                state
                    .emit_log(
                        "ollama",
                        &format!("cloud '{model}' unavailable ({err}); no local fallback"),
                    )
                    .await;
                return Err(err);
            };
            if local == *model {
                return Err(err);
            }
            state
                .emit_log(
                    "ollama",
                    &format!("cloud '{model}' unavailable, switching to local '{local}': {err}"),
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
}

impl TurnResult {
    fn reply(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        }
    }

    fn suppressed() -> Self {
        Self {
            text: String::new(),
            source: ReplySource::Model,
            suppress_telegram_reply: true,
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
        let paths = state.cached_filesystem_paths.read().await.clone();
        if paths.is_empty() {
            String::new()
        } else {
            let mounts = workspace_app_bind_pairs(&paths)
                .iter()
                .map(|(_, cpath)| cpath.clone())
                .collect::<Vec<_>>()
                .join(", ");
            format!("\nFile tools: use /app/… paths only. Mounts: {mounts}.")
        }
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

    format!(
        "{PENGINE_OUTPUT_CONTRACT_LEAD}Assistant with tools. Call a tool only for external data; otherwise answer directly. \
         After tool results, answer immediately. Be concise. \
         `brave_web_search` is only in the tool list when the user asked to search the open web (e.g. “search the internet”, “suche im Internet”, “suche nach …”) or a skill’s `requires` matches this turn — otherwise prefer **`fetch`** on any `http(s)` URL you have (including from the user). \
         At most one `brave_web_search` per user message when it is available. \
         After an allowed search, the host may auto-`fetch` several top result URLs — use those excerpts and end with **Quellen** listing every source URL.{fs_hint}{mem_hint}{weather_directive}{skills_hint}"
    )
}

async fn run_model_turn(
    state: &AppState,
    user_message: &str,
    think: bool,
    skills_slug_filter: Option<&[String]>,
) -> Result<TurnResult, String> {
    let mut model = match state.preferred_ollama_model.read().await.clone() {
        Some(m) => m,
        None => ollama::active_model().await?,
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

    let allow_brave_web_search =
        skills::allow_brave_web_search_for_message(&state.store_path, user_message);

    let mut tool_ctx = {
        let reg = state.mcp.read().await;
        reg.select_tools_for_turn(
            user_message,
            &recent_tools,
            memory_server_key.as_deref(),
            chat_session_recording,
            allow_brave_web_search,
        )
    };
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

    let system = build_system_prompt(
        state,
        user_message,
        has_tools,
        has_memory,
        skills_slug_filter,
    )
    .await;

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

    for step in 0..MAX_STEPS {
        let t0 = Instant::now();
        let effective_tools = if tools_supported {
            &tool_ctx.tools_json
        } else {
            &empty_tools
        };
        let post_tool = tool_rounds > 0;
        let json_only_user_reply = !has_tools;
        let chat_opts = chat_options_for_agent_step(post_tool, think, json_only_user_reply);

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
                return Ok(TurnResult::reply(content));
            }
            if tool_results.is_empty() {
                return Ok(TurnResult::reply(""));
            }
            break;
        }

        state
            .emit_log("tool", &format!("{} tool call(s)", tool_calls.len()))
            .await;

        // Resolve under one lock, then execute in parallel.
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
                    let args = tool_call_arguments(call);
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
        state.note_tools_used(&invoked_names).await;

        let t0 = Instant::now();
        let mut handles = Vec::with_capacity(prepared.len());
        for (name, resolved) in &prepared {
            state.emit_log("tool", &format!("[{step}] {name}")).await;
            match resolved {
                Ok((provider, tool_name, _, args)) => {
                    let (p, tn, a) = (provider.clone(), tool_name.clone(), args.clone());
                    handles.push(tokio::spawn(async move { p.call_tool(&tn, a).await }));
                }
                Err(e) => {
                    let e = e.clone();
                    handles.push(tokio::spawn(async move { Err(e) }));
                }
            }
        }

        let mut direct_replies: Vec<String> = Vec::new();
        let mut last_brave_search_blob: Option<String> = None;
        for (i, handle) in handles.into_iter().enumerate() {
            let (name, resolved) = &prepared[i];
            let (text, is_direct) = match handle.await {
                Ok(Ok(text)) => {
                    let direct = resolved.as_ref().map(|(_, _, d, _)| *d).unwrap_or(false);
                    state
                        .emit_log("tool", &format!("{name}: {} bytes", text.len()))
                        .await;
                    (text, direct)
                }
                Ok(Err(e)) => {
                    state.emit_log("tool", &format!("{name} error: {e}")).await;
                    (format!("ERROR: {e}"), false)
                }
                Err(e) => {
                    state
                        .emit_log("tool", &format!("{name} panicked: {e}"))
                        .await;
                    ("ERROR: task panicked".to_string(), false)
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

        if !direct_replies.is_empty() {
            return Ok(TurnResult {
                text: direct_replies.join("\n\n"),
                source: ReplySource::Tool,
                suppress_telegram_reply: false,
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
            });
        }

        let fallback = tool_results.into_iter().last().unwrap().1;
        return Ok(TurnResult {
            text: fallback,
            source: ReplySource::Tool,
            suppress_telegram_reply: false,
        });
    }

    Err(format!(
        "agent exceeded {MAX_STEPS} steps without finishing"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
