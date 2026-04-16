use crate::modules::memory::{
    self, MemoryProvider, SessionCommand, DIARY_END_PHRASES, DIARY_START_PHRASES,
    SESSION_END_PHRASES, SESSION_START_PHRASES,
};
use crate::modules::ollama::service as ollama;
use crate::modules::tool_engine::service::workspace_app_bind_pairs;
use crate::shared::state::{AppState, MemorySession};
use chrono::Utc;
use serde_json::json;
use std::time::{Duration, Instant};

const MAX_STEPS: usize = 3;

/// Hint appended to the system prompt when a memory server is connected. Generic on
/// purpose — specific tool names live in `modules::memory` so swapping backends doesn't
/// drift the prompt.
fn memory_hint(session_active: Option<&str>, diary_active: bool) -> String {
    let starts = SESSION_START_PHRASES.join("\", \"");
    let diary_starts = DIARY_START_PHRASES.join("\", \"");
    let diary_ends = DIARY_END_PHRASES.join("\", \"");
    let ends = SESSION_END_PHRASES.join("\", \"");
    let base = format!(
        "\n\n\
You have long-term memory via a connected Memory MCP server (see the available tools list). \
Memory recording is controlled by the HOST, not by you:\n\
- When the user says \"{starts}\", the host opens a **chat** memory session and saves **each \
user message and your reply** after every turn.\n\
- When the user says \"{diary_starts}\", the host opens **diary** mode: only the user's lines \
are saved; the host does **not** run you for those lines — you will not see them as normal chat.\n\
- Diary mode stops on \"{diary_ends}\" or any session end phrase (e.g. \"{ends}\").\n\
- \"over and out\" always ends the memory session (chat or diary).\n\
- While a **chat** session is active (not diary), each user message + your reply is persisted by \
the host after your response. You do NOT need to call memory write tools yourself.\n\
- When the user says \"{ends}\", or signs off Starfleet-style (\"Commander <name> out\" / \
\"Captain <name> out\"), the host closes the session.\n\
\n\
Feel free to call the server's read tools (e.g. `read_graph`, `search_nodes`, `open_nodes`) \
when recalling prior context helps the user. Outside an active session you may also write \
facts on your own when the user explicitly asks you to remember something specific."
    );
    match session_active {
        Some(name) if diary_active => format!(
            "{base}\n\n\
A **diary** memory session is ACTIVE (`{name}`). The user may be sending lines that are saved \
without invoking you — if you receive a normal user message in this chat, answer as usual when \
not in diary-only flow."
        ),
        Some(name) => format!(
            "{base}\n\n\
A **chat** memory session is ACTIVE (`{name}`). The host is recording — do not call memory \
write tools; just answer clearly and helpfully."
        ),
        None => base,
    }
}

/// Ollama sometimes returns `function.arguments` as a JSON string; normalize to an object.
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
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms}ms")
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
    /// When true, the Telegram layer must not send `text` to the user (diary lines).
    pub suppress_telegram_reply: bool,
}

pub async fn run_turn(state: &AppState, user_message: &str) -> Result<TurnResult, String> {
    // Session keyword commands short-circuit the model — they're host-level controls.
    if let Some(cmd) = memory::detect_session_command(user_message) {
        return match cmd {
            SessionCommand::Start => handle_session_start(state).await,
            SessionCommand::End => handle_session_end(state).await,
            SessionCommand::DiaryStart => handle_diary_start(state).await,
            SessionCommand::DiaryEnd => handle_diary_end(state).await,
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

/// If a session is active and a memory server is connected, persist this turn in the
/// background. The MCP transport allows up to 2 minutes per call, so doing the append
/// inline would leak that latency into the user's reply path.
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
                    "session `{}` active but no memory server connected — turn dropped",
                    session.entity_name
                ),
            )
            .await;
        return;
    };

    let content = format!("[user] {user_message}\n[assistant] {reply}");
    let state_bg = state.clone();
    let entity = session.entity_name;
    tokio::spawn(async move {
        state_bg
            .emit_log("memory", &format!("saving turn to `{entity}`…"))
            .await;
        match mem.append(&entity, &content).await {
            Ok(()) => {
                if let Some(s) = state_bg.memory_session.write().await.as_mut() {
                    if s.entity_name == entity {
                        s.turn_count += 1;
                    }
                }
                state_bg
                    .emit_log("memory", &format!("saved turn to `{entity}`"))
                    .await;
            }
            Err(e) => {
                state_bg
                    .emit_log("memory", &format!("save turn to `{entity}` failed: {e}"))
                    .await;
            }
        }
    });
}

async fn handle_session_start(state: &AppState) -> Result<TurnResult, String> {
    let memory = MemoryProvider::detect(&*state.mcp.read().await);
    let Some(memory) = memory else {
        return Ok(TurnResult {
            text: "No memory server is connected. Install a memory tool in Dashboard → MCP Tools first."
                .into(),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        });
    };

    if let Some(existing) = state.memory_session.read().await.clone() {
        if existing.diary_only {
            return Ok(TurnResult {
                text: "Diary recording is active — say \"record end\" or \"over and out\" before starting a chat memory session."
                    .into(),
                source: ReplySource::Model,
                suppress_telegram_reply: false,
            });
        }
        return Ok(TurnResult {
            text: format!(
                "Already recording to memory as `{}`. Say \"close session\" to end it.",
                existing.entity_name
            ),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        });
    }

    let now = Utc::now();
    let entity_name = memory::session_entity_name(now);
    let description = format!(
        "Chat session opened at {} UTC.",
        now.format("%Y-%m-%d %H:%M:%S")
    );

    if let Err(e) = memory.start_session(&entity_name, &description).await {
        state
            .emit_log("memory", &format!("failed to open session: {e}"))
            .await;
        return Ok(TurnResult {
            text: format!("Could not open memory session: {e}"),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        });
    }

    *state.memory_session.write().await = Some(MemorySession {
        entity_name: entity_name.clone(),
        started_at: now,
        turn_count: 0,
        diary_only: false,
    });

    state
        .emit_log(
            "memory",
            &format!("session opened on {}: {entity_name}", memory.server_name()),
        )
        .await;

    Ok(TurnResult {
        text: format!(
            "Captain's log opened as `{entity_name}`. Every message from here is saved to memory. \
             Say \"close session\", \"end log\", or sign off Starfleet-style (\"Commander <name> out\") \
             to close it."
        ),
        source: ReplySource::Model,
        suppress_telegram_reply: false,
    })
}

async fn handle_diary_start(state: &AppState) -> Result<TurnResult, String> {
    let memory = MemoryProvider::detect(&*state.mcp.read().await);
    let Some(memory) = memory else {
        return Ok(TurnResult {
            text: "No memory server is connected. Install a memory tool in Dashboard → MCP Tools first."
                .into(),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        });
    };

    if let Some(existing) = state.memory_session.read().await.clone() {
        if existing.diary_only {
            return Ok(TurnResult {
                text: format!(
                    "Diary recording is already active (`{}`). Say \"record end\" or \"over and out\" to stop.",
                    existing.entity_name
                ),
                source: ReplySource::Model,
                suppress_telegram_reply: false,
            });
        }
        return Ok(TurnResult {
            text: "A chat memory session is active — say \"close session\" or \"over and out\" first, then send \"record\" for diary-only mode."
                .into(),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        });
    }

    let now = Utc::now();
    let entity_name = memory::diary_entity_name(now);
    let description = format!(
        "Diary recording opened at {} UTC (user lines only; no assistant replies).",
        now.format("%Y-%m-%d %H:%M:%S")
    );

    if let Err(e) = memory.start_session(&entity_name, &description).await {
        state
            .emit_log("memory", &format!("failed to open diary session: {e}"))
            .await;
        return Ok(TurnResult {
            text: format!("Could not open diary session: {e}"),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        });
    }

    *state.memory_session.write().await = Some(MemorySession {
        entity_name: entity_name.clone(),
        started_at: now,
        turn_count: 0,
        diary_only: true,
    });

    state
        .emit_log(
            "memory",
            &format!(
                "diary session opened on {}: {entity_name}",
                memory.server_name()
            ),
        )
        .await;

    Ok(TurnResult {
        text: format!(
            "Diary recording started (`{entity_name}`). Your lines are saved silently. \
 Say \"record end\" or \"over and out\" to stop."
        ),
        source: ReplySource::Model,
        suppress_telegram_reply: false,
    })
}

async fn handle_diary_end(state: &AppState) -> Result<TurnResult, String> {
    let session = {
        let mut guard = state.memory_session.write().await;
        match guard.as_ref() {
            None => {
                return Ok(TurnResult {
                    text: "No diary recording is active. Send \"record\" to start.".into(),
                    source: ReplySource::Model,
                    suppress_telegram_reply: false,
                });
            }
            Some(s) if !s.diary_only => {
                return Ok(TurnResult {
                    text: "Not in diary mode — you're in a chat memory session. Say \"close session\" or \"over and out\" to end that."
                        .into(),
                    source: ReplySource::Model,
                    suppress_telegram_reply: false,
                });
            }
            _ => guard.take().unwrap(),
        }
    };

    let memory = MemoryProvider::detect(&*state.mcp.read().await);
    if let Some(memory) = memory {
        let now = Utc::now();
        let note = format!(
            "Diary recording stopped at {} UTC after {} line(s).",
            now.format("%Y-%m-%d %H:%M:%S"),
            session.turn_count
        );
        if let Err(e) = memory.append(&session.entity_name, &note).await {
            state
                .emit_log("memory", &format!("diary close note not saved: {e}"))
                .await;
        }
    }

    state
        .emit_log(
            "memory",
            &format!(
                "diary closed: {} ({} line(s))",
                session.entity_name, session.turn_count
            ),
        )
        .await;

    Ok(TurnResult {
        text: format!(
            "Diary recording stopped (`{}`, {} line(s) saved).",
            session.entity_name, session.turn_count
        ),
        source: ReplySource::Model,
        suppress_telegram_reply: false,
    })
}

async fn handle_diary_line(state: &AppState, user_message: &str) -> Result<TurnResult, String> {
    let Some(session) = state.memory_session.read().await.clone() else {
        return Ok(TurnResult {
            text: "Diary session ended; please start a new session with \"record\".".into(),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        });
    };
    let Some(mem) = MemoryProvider::detect(&*state.mcp.read().await) else {
        state
            .emit_log(
                "memory",
                "diary active but no memory server — line dropped; closing session",
            )
            .await;
        *state.memory_session.write().await = None;
        return Ok(TurnResult {
            text: "Memory server disconnected — diary recording stopped.".into(),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        });
    };

    let entity = session.entity_name.clone();
    let line = format!("[diary] {user_message}");
    let state_bg = state.clone();
    tokio::spawn(async move {
        state_bg
            .emit_log("memory", &format!("saving diary line to `{entity}`…"))
            .await;
        match mem.append(&entity, &line).await {
            Ok(()) => {
                if let Some(s) = state_bg.memory_session.write().await.as_mut() {
                    if s.entity_name == entity {
                        s.turn_count += 1;
                    }
                }
                state_bg
                    .emit_log("memory", &format!("saved diary line to `{entity}`"))
                    .await;
            }
            Err(e) => {
                state_bg
                    .emit_log("memory", &format!("diary append to `{entity}` failed: {e}"))
                    .await;
            }
        }
    });

    Ok(TurnResult {
        text: String::new(),
        source: ReplySource::Model,
        suppress_telegram_reply: true,
    })
}

async fn handle_session_end(state: &AppState) -> Result<TurnResult, String> {
    let taken = state.memory_session.write().await.take();
    let Some(session) = taken else {
        return Ok(TurnResult {
            text: "No memory session is active.".into(),
            source: ReplySource::Model,
            suppress_telegram_reply: false,
        });
    };

    let memory = MemoryProvider::detect(&*state.mcp.read().await);
    if let Some(memory) = memory {
        let now = Utc::now();
        let kind = if session.diary_only {
            "Diary"
        } else {
            "Session"
        };
        let note = format!(
            "{kind} closed at {} UTC after {} turn(s).",
            now.format("%Y-%m-%d %H:%M:%S"),
            session.turn_count
        );
        if let Err(e) = memory.append(&session.entity_name, &note).await {
            state
                .emit_log("memory", &format!("close note not saved: {e}"))
                .await;
        }
    }

    state
        .emit_log(
            "memory",
            &format!(
                "session closed: {} ({} turn(s))",
                session.entity_name, session.turn_count
            ),
        )
        .await;

    Ok(TurnResult {
        text: format!(
            "Memory session `{}` closed after {} turn(s).",
            session.entity_name, session.turn_count
        ),
        source: ReplySource::Model,
        suppress_telegram_reply: false,
    })
}

async fn run_model_turn(state: &AppState, user_message: &str) -> Result<TurnResult, String> {
    let model = if let Some(selected) = state.preferred_ollama_model.read().await.clone() {
        selected
    } else {
        ollama::active_model().await?
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
    let session_active_name = mem_snapshot.as_ref().map(|s| s.entity_name.clone());
    let diary_active = mem_snapshot.as_ref().is_some_and(|s| s.diary_only);

    let fs_context = {
        let paths = state.cached_filesystem_paths.read().await.clone();
        let host_lines: String = workspace_app_bind_pairs(&paths)
            .iter()
            .map(|(host, cpath)| format!("  - {cpath}  ← {host}"))
            .collect::<Vec<_>>()
            .join("\n");
        let roots_note = if paths.is_empty() {
            "No shared folders are configured yet — the container only allows **`/tmp`** for MCP file tools. \
             To read a project like `pengine`, add its folder in Dashboard → MCP Tools (File Manager) first; \
             then use **`/app/<folder-name>/README.md`** (folder-name = last path segment)."
        } else {
            "Use the **`/app/...`** paths below only — not host paths like /Users/…, and not **`/mcp/...`** (that is the server working directory, not a file root)."
        };
        format!(
            "\nFile Manager runs in a container. Allowed file roots are **`/tmp`** plus **`/app/<folder-name>`** for each folder you add in MCP Tools.\n\
             {roots_note}\n\
             Relative paths in tools are resolved under **`/app/`** (e.g. **`pengine/README.md`** → **`/app/pengine/README.md`**).\n\
{host_lines}\n"
        )
    };

    let system = if has_tools {
        let memory = if has_memory {
            memory_hint(session_active_name.as_deref(), diary_active)
        } else {
            String::new()
        };
        format!(
            "You are a helpful assistant with tool access.\n\
             Rules:\n\
             - Call a tool ONLY when you need external data you don't already have.\n\
             - After receiving tool results, answer the user's question immediately in the same response.\n\
             - Be concise and direct.{fs_context}{memory}"
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

    // Phase 1: let the model call tools (up to MAX_STEPS rounds).
    for step in 0..MAX_STEPS {
        let t_model = Instant::now();
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
                .emit_log(
                    "tool",
                    &format!("{model} does not support tools — answering without them"),
                )
                .await;
        }
        state
            .emit_log(
                "time",
                &format!("model step {step} {}", fmt_duration(t_model.elapsed())),
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
                // Model already produced a usable answer (with or without prior tool data).
                state
                    .emit_log(
                        "tool",
                        if tool_results.is_empty() {
                            "model replied in text"
                        } else {
                            "answered from tool data"
                        },
                    )
                    .await;
                return Ok(TurnResult {
                    text,
                    source: ReplySource::Model,
                    suppress_telegram_reply: false,
                });
            }

            // Model returned no text after tools ran — fall through to summarize.
            if tool_results.is_empty() {
                return Ok(TurnResult {
                    text: String::new(),
                    source: ReplySource::Model,
                    suppress_telegram_reply: false,
                });
            }
            break;
        }

        state
            .emit_log(
                "tool",
                &format!("model requested {} tool call(s)", tool_calls.len()),
            )
            .await;

        let mut direct_replies: Vec<String> = Vec::new();

        for call in &tool_calls {
            let name = call
                .get("function")
                .and_then(|f| f.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let args = tool_call_arguments(call);

            state.emit_log("tool", &format!("[{step}] {name}")).await;

            let t_tool = Instant::now();
            let resolved = {
                let reg = state.mcp.read().await;
                reg.prepare_tool_invocation(&name, args)
            };
            let (result_text, is_direct) = match resolved {
                Ok((provider, tool_name, direct, args)) => {
                    match provider.call_tool(&tool_name, args).await {
                        Ok(text) => {
                            state
                                .emit_log("tool", &format!("result ({} bytes)", text.len()))
                                .await;
                            (text, direct)
                        }
                        Err(e) => {
                            state.emit_log("tool", &format!("error: {e}")).await;
                            (format!("ERROR: {e}"), false)
                        }
                    }
                }
                Err(e) => {
                    state.emit_log("tool", &format!("error: {e}")).await;
                    (format!("ERROR: {e}"), false)
                }
            };
            state
                .emit_log(
                    "time",
                    &format!("tool {name} {}", fmt_duration(t_tool.elapsed())),
                )
                .await;

            tool_results.push((name.clone(), result_text.clone()));

            if is_direct {
                direct_replies.push(result_text.clone());
            }

            if let Some(arr) = messages.as_array_mut() {
                arr.push(json!({
                    "role": "tool",
                    "name": name,
                    "content": result_text,
                }));
            }
        }

        if !direct_replies.is_empty() {
            return Ok(TurnResult {
                text: direct_replies.join("\n\n"),
                source: ReplySource::Tool,
                suppress_telegram_reply: false,
            });
        }
    }

    // Phase 2: tools ran but model didn't produce a good answer yet.
    // Make a clean summarization call — no tools, plain Q&A with inlined data.
    if !tool_results.is_empty() {
        let mut data_block = String::new();
        for (name, content) in &tool_results {
            data_block.push_str(&format!("--- {name} result ---\n{content}\n"));
        }

        let summary_messages = json!([
            {
                "role": "system",
                "content": "Answer the user's question using ONLY the data provided below. Be concise and direct."
            },
            {
                "role": "user",
                "content": format!("{user_message}\n\nData:\n{data_block}")
            }
        ]);

        let empty = json!([]);
        let t_summary = Instant::now();
        let summary_result = ollama::chat_with_tools(&model, &summary_messages, &empty).await?;
        let summary_msg = summary_result.message;
        state
            .emit_log(
                "time",
                &format!("summarize {}", fmt_duration(t_summary.elapsed())),
            )
            .await;

        let text = summary_msg
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if !text.trim().is_empty() {
            state.emit_log("tool", "answered from tool data").await;
            return Ok(TurnResult {
                text,
                source: ReplySource::Model,
                suppress_telegram_reply: false,
            });
        }

        let fallback = tool_results
            .last()
            .map(|(_, c)| c.clone())
            .expect("tool_results must be non-empty here after guard");
        state
            .emit_log("tool", "empty summary, returning raw tool output")
            .await;
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
