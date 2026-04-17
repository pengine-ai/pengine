use crate::modules::ollama::constants::{OLLAMA_CHAT_URL, OLLAMA_PS_URL, OLLAMA_TAGS_URL};
use crate::shared::text::normalize_assistant_message_content;
use std::sync::OnceLock;

static HTTP: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP.get_or_init(reqwest::Client::new)
}

#[derive(Debug, Clone)]
pub struct ModelCatalog {
    pub active: Option<String>,
    pub models: Vec<String>,
}

/// Returns active model and the full pulled model list (`/api/tags`).
pub async fn model_catalog(timeout_ms: u64) -> Result<ModelCatalog, String> {
    let client = http_client();
    let timeout = std::time::Duration::from_millis(timeout_ms);

    let mut active: Option<String> = None;
    match client.get(OLLAMA_PS_URL).timeout(timeout).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                log::warn!(
                    "ollama {}: non-success HTTP {}",
                    OLLAMA_PS_URL,
                    resp.status()
                );
            } else {
                match resp.json::<serde_json::Value>().await {
                    Ok(body) => {
                        active = body["models"]
                            .as_array()
                            .and_then(|arr| arr.first())
                            .and_then(|m| m["name"].as_str())
                            .map(|s| s.to_string());
                    }
                    Err(e) => {
                        log::warn!("ollama {}: JSON decode error: {e}", OLLAMA_PS_URL);
                    }
                }
            }
        }
        Err(e) => log::warn!("ollama {}: request error: {e}", OLLAMA_PS_URL),
    }

    let mut models: Vec<String> = Vec::new();
    match client.get(OLLAMA_TAGS_URL).timeout(timeout).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                log::warn!(
                    "ollama {}: non-success HTTP {}",
                    OLLAMA_TAGS_URL,
                    resp.status()
                );
            } else {
                match resp.json::<serde_json::Value>().await {
                    Ok(body) => {
                        models = body["models"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|m| m["name"].as_str().map(|s| s.to_string()))
                                    .collect()
                            })
                            .unwrap_or_default();
                    }
                    Err(e) => {
                        log::warn!("ollama {}: JSON decode error: {e}", OLLAMA_TAGS_URL);
                    }
                }
            }
        }
        Err(e) => log::warn!("ollama {}: request error: {e}", OLLAMA_TAGS_URL),
    }

    if let Some(ref a) = active {
        if !models.iter().any(|m| m == a) {
            models.insert(0, a.clone());
        }
    }

    if active.is_none() && models.is_empty() {
        return Err("ollama unreachable: no active model and no pulled models".to_string());
    }

    Ok(ModelCatalog { active, models })
}

/// Returns the currently loaded model (from `/api/ps`), falling back to the
/// first pulled model (from `/api/tags`) if nothing is loaded yet.
pub async fn active_model() -> Result<String, String> {
    let catalog = model_catalog(5000).await?;
    if let Some(active) = catalog.active {
        return Ok(active);
    }
    catalog
        .models
        .first()
        .cloned()
        .ok_or_else(|| "no models pulled in ollama".to_string())
}

/// Outcome of a single chat call so the caller knows whether tools were included in the request.
pub struct ChatResult {
    pub message: serde_json::Value,
    /// `true` when this request included a non-empty `tools` payload; `false` for plain chat
    /// (including transparent fallback when the model rejects tools).
    pub tools_sent: bool,
    /// Ollama `prompt_eval_count` — tokens in the prompt. `None` if the field is missing.
    pub prompt_tokens: Option<u64>,
    /// Ollama `eval_count` — tokens produced by the model. `None` if the field is missing.
    pub eval_tokens: Option<u64>,
}

/// Per-request model controls. Extend here as we add knobs (`num_predict`,
/// `num_ctx`, `keep_alive`, …); keep the surface of `chat_with_tools` stable.
#[derive(Debug, Clone)]
pub struct ChatOptions {
    /// Ollama `think` flag. `Some(true)` enables reasoning mode (qwen3 et al.),
    /// `Some(false)` disables it, `None` omits the field so the model's own
    /// default applies.
    pub think: Option<bool>,
    /// Ollama `options.num_ctx`. Controls the KV-cache window. Default 2048 is
    /// smaller than our turn-1 prompt (~6k tokens) which forces a silent
    /// recompute; setting this explicitly lets Ollama reuse the cached prefix
    /// across turns.
    pub num_ctx: u32,
    /// Ollama `options.num_predict`. Caps completion length — critical for
    /// qwen3-class models that still emit long hidden chains when synthesizing
    /// after tool results even with `think: false`.
    pub num_predict: Option<u32>,
    /// Ollama `options.temperature`. Lower after tools → shorter, faster answers.
    pub temperature: Option<f32>,
    /// Ollama `keep_alive`. How long the model stays resident after a request.
    /// `"30m"` avoids cold-start reloads between user messages.
    pub keep_alive: &'static str,
    /// When set, Ollama structured output (`format` in the chat payload). Use only
    /// for plain chat requests (no tools); grammar masks invalid tokens so the
    /// model emits JSON matching the schema.
    pub format: Option<serde_json::Value>,
}

impl Default for ChatOptions {
    fn default() -> Self {
        Self {
            think: None,
            num_ctx: 8192,
            num_predict: None,
            temperature: None,
            keep_alive: "30m",
            format: None,
        }
    }
}

/// JSON schema for the tool-summary pass: one string field, no extra keys.
pub fn summarize_reply_json_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "reply": { "type": "string" }
        },
        "required": ["reply"],
        "additionalProperties": false
    })
}

/// Tool-aware chat for the agent loop. Sends a full message history plus a
/// list of tool definitions and returns the raw assistant message (which may
/// contain `tool_calls`). Caller is responsible for executing tools and
/// looping.
///
/// If the model rejects tools (HTTP 400 "does not support tools"), the request
/// is transparently retried without tools so older models still work.
pub async fn chat_with_tools(
    model: &str,
    messages: &serde_json::Value,
    tools: &serde_json::Value,
    options: &ChatOptions,
) -> Result<ChatResult, String> {
    let has_tools = tools.as_array().is_some_and(|a| !a.is_empty());

    let mut payload = build_payload(model, messages, options);
    if has_tools {
        payload["tools"] = tools.clone();
        // `format` constrains the whole completion; tool turns need native `tool_calls` shape.
        if let Some(obj) = payload.as_object_mut() {
            obj.remove("format");
        }
    }

    let (status, body) = post_chat(&payload).await?;

    if !status.is_success() {
        let err_text = body["error"].as_str().unwrap_or("");
        if has_tools && err_text.contains("does not support tools") {
            let plain = build_payload(model, messages, options);
            let (st, b) = post_chat(&plain).await?;
            if !st.is_success() {
                return Err(format!("ollama chat HTTP {st}: {b}"));
            }
            return build_chat_result(&b, false, options.format.is_some());
        }
        return Err(format!("ollama chat HTTP {status}: {body}"));
    }

    build_chat_result(&body, has_tools, options.format.is_some())
}

fn build_chat_result(
    body: &serde_json::Value,
    tools_sent: bool,
    expect_json_object_reply: bool,
) -> Result<ChatResult, String> {
    let (prompt_tokens, eval_tokens) = extract_token_counts(body);
    Ok(ChatResult {
        message: extract_message(body, expect_json_object_reply)?,
        tools_sent,
        prompt_tokens,
        eval_tokens,
    })
}

fn build_payload(
    model: &str,
    messages: &serde_json::Value,
    options: &ChatOptions,
) -> serde_json::Value {
    let mut opt = serde_json::Map::new();
    opt.insert("num_ctx".to_string(), serde_json::json!(options.num_ctx));
    if let Some(n) = options.num_predict {
        opt.insert("num_predict".to_string(), serde_json::json!(n));
    }
    if let Some(t) = options.temperature {
        opt.insert("temperature".to_string(), serde_json::json!(t));
    }

    let mut payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "keep_alive": options.keep_alive,
        "options": serde_json::Value::Object(opt),
    });
    if let Some(think) = options.think {
        payload["think"] = serde_json::Value::Bool(think);
    }
    if let Some(fmt) = &options.format {
        payload["format"] = fmt.clone();
    }
    payload
}

fn extract_token_counts(body: &serde_json::Value) -> (Option<u64>, Option<u64>) {
    (
        body.get("prompt_eval_count").and_then(|v| v.as_u64()),
        body.get("eval_count").and_then(|v| v.as_u64()),
    )
}

async fn post_chat(
    payload: &serde_json::Value,
) -> Result<(reqwest::StatusCode, serde_json::Value), String> {
    let resp = http_client()
        .post(OLLAMA_CHAT_URL)
        .json(payload)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok((status, body))
}

fn extract_message(
    body: &serde_json::Value,
    expect_json_object_reply: bool,
) -> Result<serde_json::Value, String> {
    let mut msg = body
        .get("message")
        .cloned()
        .ok_or_else(|| format!("ollama protocol error: missing `message` in response: {body}"))?;

    // Ollama thinking-capable models can return a separate `message.thinking` trace
    // (see https://docs.ollama.com/capabilities/thinking). Never persist or forward it:
    // only `content` is user-visible after normalization.
    if let Some(obj) = msg.as_object_mut() {
        obj.remove("thinking");
    }

    // Strip template-injected reasoning, then apply our reply contract (JSON or
    // `<pengine_reply>`) so Telegram and the next-step history never carry plan text.
    if let Some(content) = msg.get("content").and_then(|v| v.as_str()) {
        let cleaned = normalize_assistant_message_content(content, expect_json_object_reply);
        if let Some(obj) = msg.as_object_mut() {
            obj.insert("content".to_string(), serde_json::Value::String(cleaned));
        }
    }
    Ok(msg)
}
