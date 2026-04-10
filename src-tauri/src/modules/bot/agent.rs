use crate::modules::ollama::service as ollama;
use crate::shared::state::AppState;
use serde_json::json;
use std::time::{Duration, Instant};

const MAX_STEPS: usize = 3;

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
}

pub async fn run_turn(state: &AppState, user_message: &str) -> Result<TurnResult, String> {
    let model = if let Some(selected) = state.preferred_ollama_model.read().await.clone() {
        selected
    } else {
        ollama::active_model().await?
    };

    let (ollama_tools, has_tools) = {
        let reg = state.mcp.read().await;
        (reg.ollama_tools(), !reg.is_empty())
    };

    let fs_context = {
        use crate::modules::mcp::service as mcp_service;
        if state.mcp_config_path.exists() {
            mcp_service::read_config(&state.mcp_config_path)
                .ok()
                .and_then(|cfg| mcp_service::filesystem_allowed_path(&cfg))
                .map(|p| format!("\nFile tools operate on: {p}\nAlways use absolute paths starting with {p}/.", p = p))
                .unwrap_or_default()
        } else {
            String::new()
        }
    };

    let system = if has_tools {
        format!(
            "You are a helpful assistant with tool access.\n\
             Rules:\n\
             - Call a tool ONLY when you need external data you don't already have.\n\
             - After receiving tool results, answer the user's question immediately in the same response.\n\
             - Be concise and direct.{fs_context}"
        )
    } else {
        "Answer concisely.".to_string()
    };

    let mut messages = json!([
        { "role": "system", "content": system },
        { "role": "user", "content": user_message }
    ]);

    let mut tool_results: Vec<(String, String)> = Vec::new();

    // Phase 1: let the model call tools (up to MAX_STEPS rounds).
    for step in 0..MAX_STEPS {
        let t_model = Instant::now();
        let msg = ollama::chat_with_tools(&model, &messages, &ollama_tools).await?;
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
                });
            }

            // Model returned no text after tools ran — fall through to summarize.
            if tool_results.is_empty() {
                return Ok(TurnResult {
                    text: String::new(),
                    source: ReplySource::Model,
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

        let mut direct_reply: Option<String> = None;

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
            let (result_text, is_direct) = match state.mcp.read().await.call_tool(&name, args).await
            {
                Ok((text, direct)) => {
                    state
                        .emit_log("tool", &format!("result ({} bytes)", text.len()))
                        .await;
                    (text, direct)
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
                direct_reply = Some(result_text.clone());
            }

            if let Some(arr) = messages.as_array_mut() {
                arr.push(json!({
                    "role": "tool",
                    "name": name,
                    "content": result_text,
                }));
            }
        }

        if let Some(text) = direct_reply {
            return Ok(TurnResult {
                text,
                source: ReplySource::Tool,
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
        let summary_msg = ollama::chat_with_tools(&model, &summary_messages, &empty).await?;
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
            });
        }

        let fallback = tool_results.last().map(|(_, c)| c.clone()).unwrap();
        state
            .emit_log("tool", "empty summary, returning raw tool output")
            .await;
        return Ok(TurnResult {
            text: fallback,
            source: ReplySource::Tool,
        });
    }

    Err(format!(
        "agent exceeded {MAX_STEPS} steps without finishing"
    ))
}
