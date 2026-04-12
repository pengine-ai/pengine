use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct JsonRpcRequest<'a> {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl<'a> JsonRpcRequest<'a> {
    pub fn new(id: u64, method: &'a str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method,
            params,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    /// MCP permits `string | number` request ids; echo must deserialize or responses are dropped.
    #[serde(default)]
    pub id: Option<Value>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<JsonRpcError>,
}

/// Normalize JSON-RPC `id` for matching our monotonic numeric outbound ids.
pub fn jsonrpc_id_as_u64(id: &Value) -> Option<u64> {
    match id {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_i64().and_then(|i| u64::try_from(i).ok())),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    #[allow(dead_code)]
    pub code: i64,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonrpc_response_deserializes_string_id() {
        let line = r#"{"jsonrpc":"2.0","id":"3","result":{"tools":[]}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(line).expect("parse");
        let id = resp.id.as_ref().expect("id");
        assert_eq!(jsonrpc_id_as_u64(id), Some(3));
    }

    #[test]
    fn jsonrpc_response_deserializes_numeric_id() {
        let line = r#"{"jsonrpc":"2.0","id":3,"result":{"tools":[]}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(line).expect("parse");
        let id = resp.id.as_ref().expect("id");
        assert_eq!(jsonrpc_id_as_u64(id), Some(3));
    }
}
