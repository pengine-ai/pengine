use crate::modules::ollama::cloud;
use crate::modules::ollama::constants::{OLLAMA_CHAT_URL, OLLAMA_PS_URL, OLLAMA_TAGS_URL};
use crate::shared::text::normalize_assistant_message_content;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, OnceLock,
};

static HTTP: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP.get_or_init(reqwest::Client::new)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Local,
    Cloud,
}

impl ModelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelKind::Local => "local",
            ModelKind::Cloud => "cloud",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub name: String,
    pub kind: ModelKind,
}

#[derive(Debug, Clone)]
pub struct ModelCatalog {
    pub active: Option<String>,
    pub models: Vec<ModelInfo>,
}

/// Cloud models are surfaced by the local Ollama daemon after `ollama signin`
/// and are tagged with `-cloud` (e.g. `gpt-oss:120b-cloud`) or the bare tag
/// `cloud`. Tag is the part after the first `:` (defaulting to `latest`).
pub fn classify_model(name: &str) -> ModelKind {
    let tag = name.split_once(':').map(|(_, t)| t).unwrap_or("");
    if tag == "cloud" || tag.ends_with("-cloud") {
        ModelKind::Cloud
    } else {
        ModelKind::Local
    }
}

/// Returns active model and the full pulled model list (`/api/tags`).
pub async fn model_catalog(timeout_ms: u64) -> Result<ModelCatalog, String> {
    let client = http_client();
    let timeout = std::time::Duration::from_millis(timeout_ms);

    let mut active: Option<String> = None;
    let mut daemon_reachable = false;
    match client.get(OLLAMA_PS_URL).timeout(timeout).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                log::warn!(
                    "ollama {}: non-success HTTP {}",
                    OLLAMA_PS_URL,
                    resp.status()
                );
            } else {
                daemon_reachable = true;
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

    let mut models: Vec<ModelInfo> = Vec::new();
    match client.get(OLLAMA_TAGS_URL).timeout(timeout).send().await {
        Ok(resp) => {
            if !resp.status().is_success() {
                log::warn!(
                    "ollama {}: non-success HTTP {}",
                    OLLAMA_TAGS_URL,
                    resp.status()
                );
            } else {
                daemon_reachable = true;
                match resp.json::<serde_json::Value>().await {
                    Ok(body) => {
                        models = body["models"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|m| {
                                        m["name"].as_str().map(|s| ModelInfo {
                                            name: s.to_string(),
                                            kind: classify_model(s),
                                        })
                                    })
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
        if !models.iter().any(|m| &m.name == a) {
            models.insert(
                0,
                ModelInfo {
                    name: a.clone(),
                    kind: classify_model(a),
                },
            );
        }
    }

    for cloud_name in cloud::list_cloud_models().await {
        if !models.iter().any(|m| m.name == cloud_name) {
            models.push(ModelInfo {
                name: cloud_name,
                kind: ModelKind::Cloud,
            });
        }
    }

    if !daemon_reachable && models.is_empty() {
        return Err("ollama unreachable: no active model and no pulled models".to_string());
    }

    Ok(ModelCatalog { active, models })
}

/// Best-guess local fallback model when a cloud rate-limit forces a downgrade.
/// Prefers `preferred` if local, then `last_local`, then the active model if
/// local, then the first local entry in the catalog.
pub fn pick_local_fallback(
    catalog: &ModelCatalog,
    preferred: Option<&str>,
    last_local: Option<&str>,
) -> Option<String> {
    let local_named = |name: &str| {
        catalog
            .models
            .iter()
            .find(|m| m.name == name && m.kind == ModelKind::Local)
            .map(|m| m.name.clone())
    };
    if let Some(p) = preferred {
        if let Some(m) = local_named(p) {
            return Some(m);
        }
    }
    if let Some(p) = last_local {
        if let Some(m) = local_named(p) {
            return Some(m);
        }
    }
    if let Some(active) = catalog.active.as_deref() {
        if let Some(m) = local_named(active) {
            return Some(m);
        }
    }
    catalog
        .models
        .iter()
        .find(|m| m.kind == ModelKind::Local)
        .map(|m| m.name.clone())
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
        .map(|m| m.name.clone())
        .ok_or_else(|| "no models pulled in ollama".to_string())
}

/// Load `model` in the Ollama daemon so `/api/ps` reports it (same as a first chat turn).
///
/// Uses a minimal `/api/chat` request with `num_predict: 1`. Large models may take
/// minutes on first load; timeout is generous.
pub async fn touch_activate_model(model: &str) -> Result<(), String> {
    let payload = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": " "}],
        "stream": false,
        "keep_alive": "30m",
        "options": {
            "num_predict": 1,
            "num_ctx": 2048
        }
    });
    let timeout = std::time::Duration::from_secs(300);
    let resp = http_client()
        .post(OLLAMA_CHAT_URL)
        .json(&payload)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let err = body.get("error").and_then(|v| v.as_str()).unwrap_or("");
        return Err(if err.is_empty() {
            format!("ollama chat HTTP {status}")
        } else {
            format!("ollama chat HTTP {status}: {err}")
        });
    }
    if let Some(err) = body.get("error").and_then(|v| v.as_str()) {
        if !err.is_empty() {
            return Err(err.to_string());
        }
    }
    Ok(())
}

/// Detect cloud-side failures that warrant downgrading to a local model.
/// Covers explicit rate limits (429 / "rate limit" / "quota"), upstream
/// outages proxied as 5xx with the cloud's `ref: <uuid>` envelope, and the
/// "sign in / unauthorized" responses returned when the user hasn't run
/// `ollama signin`. Any of these mean the picked cloud model can't serve
/// this turn — the local fallback keeps the agent responsive.
pub fn is_cloud_unavailable_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    lower.contains("http 429")
        || lower.contains("rate limit")
        || lower.contains("rate-limit")
        || lower.contains("quota")
        || lower.contains("too many requests")
        || lower.contains("http 500")
        || lower.contains("http 502")
        || lower.contains("http 503")
        || lower.contains("http 504")
        || lower.contains("internal server error")
        || lower.contains("unauthorized")
        || lower.contains("sign in")
        || lower.contains("not signed in")
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
    /// When set, the Ollama request uses streaming mode and each generated chunk
    /// increments this counter by 1 (≈ 1 token/chunk). The spinner reads it
    /// atomically every tick to show a live `out:N↑` display.
    pub live_out_tokens: Option<Arc<AtomicU64>>,
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
            live_out_tokens: None,
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
    if options.live_out_tokens.is_some() {
        payload["stream"] = serde_json::Value::Bool(true);
    }
    if has_tools {
        payload["tools"] = tools.clone();
        // `format` constrains the whole completion; tool turns need native `tool_calls` shape.
        if let Some(obj) = payload.as_object_mut() {
            obj.remove("format");
        }
    }

    let (status, body) = if let Some(counter) = &options.live_out_tokens {
        post_chat_streaming(&payload, counter).await?
    } else {
        post_chat(&payload).await?
    };

    if !status.is_success() {
        let err_text = body["error"].as_str().unwrap_or("");
        if has_tools && err_text.contains("does not support tools") {
            // Retry without tools using non-streaming (edge-case fallback).
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

/// Per-request ceiling for `/api/chat`. Tool-heavy coding sessions carry large message JSON;
/// local models can exceed two minutes on CPU — the previous 120s cap caused misleading failures
/// (`error sending request for url`) while MCP tools had already succeeded.
const OLLAMA_CHAT_REQUEST_TIMEOUT_SECS: u64 = 600;

/// Maps reqwest errors into actionable CLI/agent text (daemon down vs timeout vs generic).
fn explain_ollama_chat_transport_error(err_msg: &str) -> String {
    let m = err_msg.to_ascii_lowercase();
    if m.contains("connection refused")
        || m.contains("connection reset")
        || m.contains("failed to connect")
    {
        format!(
            "cannot reach Ollama at {OLLAMA_CHAT_URL} ({err_msg}). Start the daemon: `ollama serve`"
        )
    } else if m.contains("timed out") || m.contains("timeout") {
        format!(
            "Ollama chat timed out after {OLLAMA_CHAT_REQUEST_TIMEOUT_SECS}s ({OLLAMA_CHAT_URL}). \
Try a smaller model, shorter session, or ensure the GPU/CPU is not overloaded."
        )
    } else if m.contains("error sending request") {
        // Reqwest’s generic text for several failures (incl. some timeouts / connection drops).
        format!(
            "Ollama chat request to {OLLAMA_CHAT_URL} failed ({err_msg}). \
If the daemon is not running, start `ollama serve`. Otherwise retry — long tool-heavy prompts can exceed limits or overload Ollama."
        )
    } else {
        format!("Ollama chat transport error ({OLLAMA_CHAT_URL}): {err_msg}")
    }
}

/// Streaming variant of [`post_chat`]. Reads NDJSON chunks from Ollama's
/// `"stream": true` response, accumulates content and tool_calls, and
/// increments `token_counter` once per content/thinking chunk (≈ 1 per token).
/// Returns a body shaped identically to the non-streaming response so the rest
/// of the call stack can stay unchanged.
async fn post_chat_streaming(
    payload: &serde_json::Value,
    token_counter: &Arc<AtomicU64>,
) -> Result<(reqwest::StatusCode, serde_json::Value), String> {
    let mut resp = http_client()
        .post(OLLAMA_CHAT_URL)
        .json(payload)
        .timeout(std::time::Duration::from_secs(
            OLLAMA_CHAT_REQUEST_TIMEOUT_SECS,
        ))
        .send()
        .await
        .map_err(|e| explain_ollama_chat_transport_error(&e.to_string()))?;

    let status = resp.status();

    // For non-success responses there is no NDJSON stream; read body normally.
    if !status.is_success() {
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("ollama response JSON decode ({OLLAMA_CHAT_URL}): {e}"))?;
        return Ok((status, body));
    }

    let mut content_buf = String::new();
    let mut tool_calls_opt: Option<serde_json::Value> = None;
    let mut done_chunk = serde_json::Value::Null;
    // Raw bytes buffer for reassembling partial HTTP chunks into complete lines.
    let mut byte_buf: Vec<u8> = Vec::with_capacity(4096);

    'read: loop {
        let Some(raw) = resp
            .chunk()
            .await
            .map_err(|e| format!("ollama stream read ({OLLAMA_CHAT_URL}): {e}"))?
        else {
            break;
        };
        byte_buf.extend_from_slice(&raw);

        // Drain every complete \n-terminated JSON line from the buffer.
        while let Some(nl) = byte_buf.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = byte_buf.drain(..=nl).collect();
            let line = match std::str::from_utf8(&line_bytes) {
                Ok(s) => s.trim().to_string(),
                Err(_) => continue,
            };
            if line.is_empty() {
                continue;
            }
            let Ok(ev) = serde_json::from_str::<serde_json::Value>(&line) else {
                continue;
            };

            let msg = ev.get("message");

            // Increment live counter for each content or thinking chunk (≈ 1 token).
            let has_content = msg
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .is_some_and(|s| !s.is_empty());
            let has_thinking = msg
                .and_then(|m| m.get("thinking"))
                .and_then(|t| t.as_str())
                .is_some_and(|s| !s.is_empty());
            if has_content || has_thinking {
                token_counter.fetch_add(1, Ordering::Relaxed);
            }

            // Accumulate content (thinking is intentionally excluded — extract_message strips it).
            if let Some(c) = msg
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .filter(|s| !s.is_empty())
            {
                content_buf.push_str(c);
            }

            // Ollama emits tool_calls as a complete object in a single chunk.
            if let Some(tc) = msg.and_then(|m| m.get("tool_calls")) {
                if tc.as_array().is_some_and(|a| !a.is_empty()) {
                    tool_calls_opt = Some(tc.clone());
                }
            }

            if ev.get("done").and_then(|v| v.as_bool()).unwrap_or(false) {
                done_chunk = ev;
                break 'read;
            }
        }
    }

    // Reconstruct a message object matching the non-streaming shape.
    let mut message = serde_json::json!({
        "role": "assistant",
        "content": content_buf,
    });
    if let Some(tc) = tool_calls_opt {
        message["tool_calls"] = tc;
    }

    // Carry forward the done-chunk's stat fields (prompt_eval_count, eval_count, …)
    // and replace `message` with our reassembled version.
    let mut body = match done_chunk.as_object() {
        Some(obj) => serde_json::Value::Object(obj.clone()),
        None => serde_json::json!({}),
    };
    body["message"] = message;

    Ok((status, body))
}

async fn post_chat(
    payload: &serde_json::Value,
) -> Result<(reqwest::StatusCode, serde_json::Value), String> {
    let resp = http_client()
        .post(OLLAMA_CHAT_URL)
        .json(payload)
        .timeout(std::time::Duration::from_secs(
            OLLAMA_CHAT_REQUEST_TIMEOUT_SECS,
        ))
        .send()
        .await
        .map_err(|e| explain_ollama_chat_transport_error(&e.to_string()))?;
    let status = resp.status();
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("ollama response JSON decode ({OLLAMA_CHAT_URL}): {e}"))?;
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

#[cfg(test)]
mod tests {
    use super::explain_ollama_chat_transport_error;

    #[test]
    fn explain_maps_connection_refused() {
        let s = explain_ollama_chat_transport_error(
            "error sending request for url (http://localhost:11434/api/chat): connection refused",
        );
        assert!(s.contains("cannot reach Ollama"), "unexpected message: {s}");
        assert!(s.contains("ollama serve"), "{s}");
    }

    #[test]
    fn explain_maps_timeout() {
        let s =
            explain_ollama_chat_transport_error("operation timed out waiting for response body");
        assert!(s.contains("timed out"), "{s}");
        assert!(s.contains("600"), "{s}");
    }

    #[test]
    fn explain_preserves_unknown_suffix() {
        let raw = "something obscure xyz";
        let s = explain_ollama_chat_transport_error(raw);
        assert!(s.contains("transport error"), "{s}");
        assert!(s.contains(raw), "{s}");
    }

    #[test]
    fn explain_maps_reqwest_generic_sending_error() {
        let raw = "error sending request for url (http://localhost:11434/api/chat)";
        let s = explain_ollama_chat_transport_error(raw);
        assert!(s.contains("Ollama chat request"), "{s}");
        assert!(s.contains("ollama serve"), "{s}");
    }
}
