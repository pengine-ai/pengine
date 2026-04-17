use crate::modules::memory::{self, MemoryProvider, SessionCommand};
use crate::modules::ollama::service as ollama;
use crate::modules::skills::service as skills;
use crate::modules::tool_engine::service::workspace_app_bind_pairs;
use crate::shared::state::{AppState, MemorySession};
use chrono::Utc;
use serde_json::json;
use std::time::{Duration, Instant};

const MAX_STEPS: usize = 3;

fn memory_hint(session_active: Option<&str>, diary_active: bool) -> String {
    let status = match session_active {
        Some(name) if diary_active => {
            format!(" Diary recording ACTIVE (`{name}`); your replies are NOT saved.")
        }
        Some(name) => format!(
            " Chat session ACTIVE (`{name}`); host records each turn — do NOT call memory write tools."
        ),
        None => String::new(),
    };
    format!(
        "\nMemory MCP server connected. Host controls recording via keywords \
(\"captain's log\" / \"record\" to start; \"close session\" / \"over and out\" / \
\"Commander <name> out\" to stop). Use read tools (`read_graph`, `search_nodes`, \
`open_nodes`) when prior context helps.{status}"
    )
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

    let result = run_model_turn(state, user_message).await?;
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

async fn run_model_turn(state: &AppState, user_message: &str) -> Result<TurnResult, String> {
    let model = match state.preferred_ollama_model.read().await.clone() {
        Some(m) => m,
        None => ollama::active_model().await?,
    };

    let (ollama_tools, has_tools, has_memory) = {
        let reg = state.mcp.read().await;
        (
            reg.ollama_tools(),
            !reg.is_empty(),
            MemoryProvider::detect(&reg).is_some(),
        )
    };

    let mem_snapshot = state.memory_session.read().await.clone();

    let system = if has_tools {
        let fs_hint = {
            let paths = state.cached_filesystem_paths.read().await.clone();
            if paths.is_empty() {
                String::new()
            } else {
                let mounts: String = workspace_app_bind_pairs(&paths)
                    .iter()
                    .map(|(host, cpath)| format!("{cpath} ← {host}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("\nFile tools use container paths under /app/. Mounts: {mounts}. Use /app/… paths only.")
            }
        };
        let mem_hint = if has_memory {
            memory_hint(
                mem_snapshot.as_ref().map(|s| s.entity_name.as_str()),
                mem_snapshot.as_ref().is_some_and(|s| s.diary_only),
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
            "Helpful assistant with tools. Call a tool ONLY when you need external data. \
             After tool results, answer immediately. Be concise.{fs_hint}{mem_hint}{skills_hint}"
        )
    } else {
        "Answer concisely.".to_string()
    };

    let mut messages = json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user_message }
    ]);

    let mut tool_results: Vec<(String, String)> = Vec::new();
    let mut tools_supported = true;
    let empty_tools = json!([]);

    for step in 0..MAX_STEPS {
        let t0 = Instant::now();
        let effective_tools = if tools_supported {
            &ollama_tools
        } else {
            &empty_tools
        };
        let result = ollama::chat_with_tools(&model, &messages, effective_tools).await?;
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
                &format!("model step {step} {}", fmt_duration(t0.elapsed())),
            )
            .await;

        if let Some(arr) = messages.as_array_mut() {
            arr.push(msg.clone());
        }

        let tool_calls = msg
            .get("tool_calls")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if tool_calls.is_empty() {
            let text = msg
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if !text.is_empty() {
                return Ok(TurnResult {
                    text,
                    source: ReplySource::Model,
                    suppress_telegram_reply: false,
                });
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
            if let Some(arr) = messages.as_array_mut() {
                arr.push(json!({ "role": "tool", "name": name, "content": &text }));
            }
            tool_results.push((name.clone(), text));
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

        let t0 = Instant::now();
        let result = ollama::chat_with_tools(&model, &summary_messages, &json!([])).await?;
        state
            .emit_log("time", &format!("summarize {}", fmt_duration(t0.elapsed())))
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
