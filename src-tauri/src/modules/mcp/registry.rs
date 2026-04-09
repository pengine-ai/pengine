use super::client::McpClient;
use super::native::NativeProvider;
use super::types::ToolDef;
use serde_json::{json, Value};

pub enum Provider {
    Native(NativeProvider),
    Mcp(Box<McpClient>),
}

impl Provider {
    pub fn server_name(&self) -> &str {
        match self {
            Provider::Native(n) => &n.server_name,
            Provider::Mcp(c) => &c.server_name,
        }
    }

    pub fn tools(&self) -> &[ToolDef] {
        match self {
            Provider::Native(n) => &n.tools,
            Provider::Mcp(c) => &c.tools,
        }
    }

    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        match self {
            Provider::Native(n) => n.call(name, &args),
            Provider::Mcp(c) => c.call_tool(name, args).await,
        }
    }
}

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
            .flat_map(|p| p.tools().iter())
            .filter(|t| should_expose_to_model(t))
            .map(|t| t.name.clone())
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

fn should_expose_to_model(tool: &ToolDef) -> bool {
    let desc = tool.description.as_deref().unwrap_or("");
    if desc.to_ascii_uppercase().contains("DEPRECATED") {
        return false;
    }
    !REDUNDANT_TOOLS.contains(&tool.name.as_str())
}

/// Tools that add noise without value for a small local model.
const REDUNDANT_TOOLS: &[&str] = &[
    "read_media_file",
    "read_multiple_files",
    "list_directory_with_sizes",
    "directory_tree",
    "list_allowed_directories",
];

fn build_ollama_tools(providers: &[Provider]) -> Value {
    let arr: Vec<Value> = providers
        .iter()
        .flat_map(|p| p.tools().iter())
        .filter(|t| should_expose_to_model(t))
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
