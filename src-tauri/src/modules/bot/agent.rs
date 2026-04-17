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
use std::time::{Duration, Instant};

const MAX_STEPS: usize = 3;

/// After tool results (agent step ≥1), cap completion tokens. The model should
/// put the user-visible answer in `<pengine_reply>` (see system prompt); this
/// cap bounds wall time if it drafts a long `<pengine_plan>`. ~1024 fits a
/// concise multilingual answer in most cases.
const POST_TOOL_NUM_PREDICT: u32 = 1024;
const POST_TOOL_TEMPERATURE: f32 = 0.35;

/// Fallback summarize pass when the tool loop exits without a user-visible reply.
const SUMMARY_NUM_PREDICT: u32 = 768;
const SUMMARY_TEMPERATURE: f32 = 0.3;

fn chat_options_for_agent_step(step: usize, user_wants_think: bool) -> ChatOptions {
    if step == 0 {
        ChatOptions {
            think: Some(user_wants_think),
            num_predict: None,
            temperature: None,
            ..ChatOptions::default()
        }
    } else {
        ChatOptions {
            think: Some(false),
            num_predict: Some(POST_TOOL_NUM_PREDICT),
            temperature: Some(POST_TOOL_TEMPERATURE),
            ..ChatOptions::default()
        }
    }
}

/// Cap on tool output fed back to the model. Raw fetch bodies can be 5–10 kB
/// of HTML; the model only needs the first screen to answer, and larger
/// feedback balloons the step-1 prompt. Direct replies (answers routed
/// straight to the user) are NOT truncated.
const TOOL_OUTPUT_CHAR_CAP: usize = 4000;

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

    let result = run_model_turn(state, user_message, think.enabled()).await?;
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
async fn build_system_prompt(state: &AppState, has_tools: bool, has_memory: bool) -> String {
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

    let skills_raw = skills::skills_prompt_hint(&state.store_path);
    let (skills_hint, skills_truncated) =
        skills::limit_skills_hint_bytes(skills_raw, skills::MAX_TOTAL_SKILL_HINT_BYTES);
    if skills_truncated {
        state
            .emit_log(
                "run",
                &format!(
                    "skills hint truncated to {} bytes (cap {})",
                    skills_hint.len(),
                    skills::MAX_TOTAL_SKILL_HINT_BYTES
                ),
            )
            .await;
    }

    format!(
        "{PENGINE_OUTPUT_CONTRACT_LEAD}Assistant with tools. Call a tool only for external data; otherwise answer directly. \
         After tool results, answer immediately. Be concise.{fs_hint}{mem_hint}{skills_hint}"
    )
}

async fn run_model_turn(
    state: &AppState,
    user_message: &str,
    think: bool,
) -> Result<TurnResult, String> {
    let model = match state.preferred_ollama_model.read().await.clone() {
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

    let mut tool_ctx = {
        let reg = state.mcp.read().await;
        reg.select_tools_for_turn(
            user_message,
            &recent_tools,
            memory_server_key.as_deref(),
            chat_session_recording,
        )
    };
    state
        .emit_log(
            "tool_ctx",
            &format!(
                "select_ms={} active={}/{} subset={} routing={} recording={} high_risk={} recent_n={}",
                tool_ctx.select_ms,
                tool_ctx.active_count,
                tool_ctx.total_count,
                tool_ctx.used_subset,
                tool_ctx.routing,
                chat_session_recording,
                tool_ctx.high_risk_active,
                recent_tools.len()
            ),
        )
        .await;
    state.record_tool_selection_ms(tool_ctx.select_ms).await;

    let system = build_system_prompt(state, has_tools, has_memory).await;

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

    for step in 0..MAX_STEPS {
        let t0 = Instant::now();
        let effective_tools = if tools_supported {
            &tool_ctx.tools_json
        } else {
            &empty_tools
        };
        let chat_opts = chat_options_for_agent_step(step, think);

        let inject_post_tool = step > 0;
        if inject_post_tool {
            push_ephemeral_post_tool_reminder(&mut messages);
        }

        let result = ollama::chat_with_tools(&model, &messages, effective_tools, &chat_opts).await;
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
                reg.full_tool_context()
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
        let prepared: Vec<_> = {
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
                .collect()
        };

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
                Err(_) => {
                    handles.push(tokio::spawn(async { Err("resolve failed".to_string()) }));
                }
            }
        }

        let mut direct_replies: Vec<String> = Vec::new();
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

            if is_direct {
                direct_replies.push(text.clone());
            }
            let compacted = compact_tool_output(&text);
            let for_model = truncate_for_model(&compacted, TOOL_OUTPUT_CHAR_CAP);
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
        let mut data = String::new();
        for (name, content) in &tool_results {
            data.push_str(&format!("--- {name} ---\n{content}\n"));
        }

        let summary_messages = json!([
            { "role": "system", "content": "Answer using ONLY the data below. Be concise." },
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
        let result =
            ollama::chat_with_tools(&model, &summary_messages, &json!([]), &summary_opts).await?;
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
}
