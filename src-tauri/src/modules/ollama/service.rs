use crate::modules::ollama::constants::{OLLAMA_CHAT_URL, OLLAMA_PS_URL, OLLAMA_TAGS_URL};
use std::sync::OnceLock;

static HTTP: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP.get_or_init(reqwest::Client::new)
}

/// Returns the currently loaded model (from `/api/ps`), falling back to the
/// first pulled model (from `/api/tags`) if nothing is loaded yet.
pub async fn active_model() -> Result<String, String> {
    let client = http_client();
    let timeout = std::time::Duration::from_secs(5);

    if let Ok(resp) = client.get(OLLAMA_PS_URL).timeout(timeout).send().await {
        if let Ok(body) = resp.json::<serde_json::Value>().await {
            if let Some(name) = body["models"]
                .as_array()
                .and_then(|arr| arr.first())
                .and_then(|m| m["name"].as_str())
            {
                return Ok(name.to_string());
            }
        }
    }

    let resp = client
        .get(OLLAMA_TAGS_URL)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format!("ollama unreachable: {e}"))?;

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    body["models"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|m| m["name"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "no models pulled in ollama".to_string())
}

/// Tool-aware chat for the agent loop. Sends a full message history plus a
/// list of tool definitions and returns the raw assistant message (which may
/// contain `tool_calls`). Caller is responsible for executing tools and
/// looping. Returns the `message` object verbatim.
pub async fn chat_with_tools(
    model: &str,
    messages: &serde_json::Value,
    tools: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let payload = serde_json::json!({
        "model": model,
        "messages": messages,
        "tools": tools,
        "stream": false,
    });

    let resp = http_client()
        .post(OLLAMA_CHAT_URL)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body
        .get("message")
        .cloned()
        .unwrap_or(serde_json::Value::Null))
}
