//! Tool registry: aggregates providers behind a single dispatch interface.
//!
//! Adding a new provider kind (e.g. Docker-backed MCP) means adding a variant
//! to [`Provider`] and a match arm in each method — nothing else changes.

use super::native::NativeProvider;
use super::types::ToolDef;
use serde_json::{json, Value};

// ── Provider ───────────────────────────────────────────────────────

/// Where a tool lives.  Extend this enum for Docker / external MCP
/// servers — the registry, agent loop, and HTTP API stay untouched.
pub enum Provider {
    Native(NativeProvider),
}

impl Provider {
    pub fn server_name(&self) -> &str {
        match self {
            Provider::Native(n) => &n.server_name,
        }
    }

    pub fn tools(&self) -> &[ToolDef] {
        match self {
            Provider::Native(n) => &n.tools,
        }
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        match self {
            Provider::Native(n) => n.call(name, &args),
        }
    }
}

// ── Registry ───────────────────────────────────────────────────────

/// Central tool registry.  Pre-caches the Ollama tool JSON and the
/// human-readable name list at construction time so the hot path
/// (each chat turn) is just a cheap `clone()`.
pub struct ToolRegistry {
    providers: Vec<Provider>,
    cached_ollama_tools: Value,
    cached_tool_names: Vec<String>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self {
            providers: Vec::new(),
            cached_ollama_tools: Value::Array(Vec::new()),
            cached_tool_names: Vec::new(),
        }
    }
}

impl ToolRegistry {
    pub fn new(providers: Vec<Provider>) -> Self {
        let cached_ollama_tools = build_ollama_tools(&providers);
        let cached_tool_names = providers
            .iter()
            .flat_map(|p| p.tools().iter().map(|t| t.name.clone()))
            .collect();
        Self {
            providers,
            cached_ollama_tools,
            cached_tool_names,
        }
    }

    pub fn all_tools(&self) -> Vec<ToolDef> {
        self.providers
            .iter()
            .flat_map(|p| p.tools().iter().cloned())
            .collect()
    }

    pub fn ollama_tools(&self) -> Value {
        self.cached_ollama_tools.clone()
    }

    pub fn tool_names(&self) -> &[String] {
        &self.cached_tool_names
    }

    pub fn is_empty(&self) -> bool {
        self.cached_tool_names.is_empty()
    }

    /// Dispatch a tool call.  Returns `(output_text, direct_return)`.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<(String, bool), String> {
        let (server, tool) = match name.split_once('.') {
            Some((s, t)) => (Some(s), t),
            None => (None, name),
        };

        for provider in &self.providers {
            if let Some(s) = server {
                if provider.server_name() != s {
                    continue;
                }
            }
            if let Some(def) = provider.tools().iter().find(|t| t.name == tool) {
                let direct = def.direct_return;
                let text = provider.call_tool(tool, args).await?;
                return Ok((text, direct));
            }
        }
        Err(format!("tool not found: {name}"))
    }
}

fn build_ollama_tools(providers: &[Provider]) -> Value {
    let arr: Vec<Value> = providers
        .iter()
        .flat_map(|p| p.tools().iter())
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description.clone().unwrap_or_default(),
                    "parameters": t.input_schema,
                }
            })
        })
        .collect();
    Value::Array(arr)
}
