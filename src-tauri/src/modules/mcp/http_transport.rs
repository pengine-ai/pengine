//! HTTP transport for MCP. Speaks JSON-RPC over POST against a single URL.
//!
//! Supports two response shapes:
//! - `Content-Type: application/json` — body is the JSON-RPC response.
//! - `Content-Type: text/event-stream` — first `data:` line carries the
//!   JSON-RPC response (basic SSE, sufficient for MCP "Streamable HTTP"
//!   replies that don't multiplex notifications back).
//!
//! Compatible with Claude Code's `"type": "http"` server entries.

use super::protocol::JsonRpcResponse;
use reqwest::Client;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

pub struct HttpTransport {
    client: Client,
    url: String,
    headers: HashMap<String, String>,
    next_id: AtomicU64,
}

impl HttpTransport {
    pub fn new(url: String, headers: HashMap<String, String>) -> Result<Self, String> {
        // Global reqwest ceiling (individual requests also set `.timeout(...)` in `call_with_timeout`).
        // Must be ≥ longest per-method `tools/call` timeout (e.g. shell MCP 300s).
        let client = Client::builder()
            .timeout(Duration::from_secs(360))
            .build()
            .map_err(|e| format!("reqwest client: {e}"))?;
        Ok(Self {
            client,
            url,
            headers,
            next_id: AtomicU64::new(1),
        })
    }

    pub fn default_call_timeout() -> Duration {
        Duration::from_secs(60)
    }

    pub async fn call(&self, method: &str, params: Option<Value>) -> Result<Value, String> {
        self.call_with_timeout(method, params, Self::default_call_timeout())
            .await
    }

    pub async fn call_with_timeout(
        &self,
        method: &str,
        params: Option<Value>,
        timeout: Duration,
    ) -> Result<Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut req = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
        });
        if let Some(p) = params {
            req["params"] = p;
        }

        let mut request = self
            .client
            .post(&self.url)
            .timeout(timeout)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&req);
        for (k, v) in &self.headers {
            request = request.header(k, v);
        }

        let resp = request
            .send()
            .await
            .map_err(|e| format!("http {method} {}: {e}", self.url))?;

        let status = resp.status();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = resp.text().await.map_err(|e| format!("read body: {e}"))?;

        if !status.is_success() {
            return Err(format!(
                "http {method} {}: HTTP {status} {}",
                self.url,
                truncate_for_error(&body, 256)
            ));
        }

        let parsed = parse_response(&content_type, &body)?;
        if let Some(err) = parsed.error {
            return Err(format!("mcp error: {}", err.message));
        }
        Ok(parsed.result.unwrap_or(Value::Null))
    }

    pub async fn notify(&self, method: &str, params: Option<Value>) -> Result<(), String> {
        let mut req = json!({ "jsonrpc": "2.0", "method": method });
        if let Some(p) = params {
            req["params"] = p;
        }
        let mut request = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .json(&req);
        for (k, v) in &self.headers {
            request = request.header(k, v);
        }
        request
            .send()
            .await
            .map_err(|e| format!("http notify: {e}"))?;
        Ok(())
    }
}

fn parse_response(content_type: &str, body: &str) -> Result<JsonRpcResponse, String> {
    if content_type.contains("event-stream") {
        let data = body
            .lines()
            .filter_map(|l| l.strip_prefix("data:").map(str::trim_start))
            .next()
            .ok_or_else(|| "sse response had no `data:` line".to_string())?;
        serde_json::from_str(data).map_err(|e| format!("parse sse json: {e}: {data}"))
    } else {
        serde_json::from_str(body).map_err(|e| {
            format!(
                "parse json response: {e}: {}",
                truncate_for_error(body, 256)
            )
        })
    }
}

fn truncate_for_error(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max).collect();
    format!("{head}…")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_response_handles_plain_json() {
        let r = parse_response(
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#,
        )
        .unwrap();
        assert_eq!(r.result.unwrap()["ok"], json!(true));
    }

    #[test]
    fn parse_response_handles_sse_data_line() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"ok\":true}}\n\n";
        let r = parse_response("text/event-stream", body).unwrap();
        assert_eq!(r.result.unwrap()["ok"], json!(true));
    }

    #[test]
    fn parse_response_propagates_jsonrpc_error_through_caller() {
        let r = parse_response(
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"unknown method"}}"#,
        )
        .unwrap();
        assert_eq!(r.error.unwrap().message, "unknown method");
    }

    #[test]
    fn parse_response_rejects_sse_without_data_line() {
        let err = parse_response("text/event-stream", "event: ping\n\n").unwrap_err();
        assert!(err.contains("data"));
    }

    #[test]
    fn truncate_for_error_caps_long_strings() {
        let s = "x".repeat(500);
        let out = truncate_for_error(&s, 100);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().count(), 101);
    }
}
