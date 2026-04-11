use super::client::McpClient;
use super::native::NativeProvider;
use super::types::ToolDef;
use serde_json::{json, Value};
use std::sync::Arc;

/// Normalize File Manager paths: absolute container paths pass through; relative `pengine/README.md` → `/app/pengine/README.md`.
fn rewrite_file_manager_path(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return t.to_string();
    }
    // Models often confuse the image WORKDIR (`/mcp`) with an allowed root — it is not.
    if let Some(rest) = t.strip_prefix("/mcp/") {
        return format!("/app/{rest}");
    }
    if t == "/mcp" {
        log::warn!(
            "rewrite_file_manager_path: `/mcp` is the server working directory, not an MCP file root; leaving path unchanged"
        );
        return "/mcp".to_string();
    }
    if t.starts_with("/opt/mcp-filesystem") {
        return t.to_string();
    }
    if t.starts_with('/') {
        return t.to_string();
    }
    if t.contains(':') || t.starts_with("\\\\") {
        return t.to_string();
    }
    resolve_relative_under_app(t)
}

/// Resolve a relative path under `/app` with `..` handling; escaping above the root → `/app`.
fn resolve_relative_under_app(raw: &str) -> String {
    let u = raw.replace('\\', "/");
    let mut stack: Vec<&str> = Vec::new();
    let mut escaped = false;
    for seg in u.split('/').filter(|s| !s.is_empty() && *s != ".") {
        if seg == ".." {
            if stack.pop().is_none() {
                escaped = true;
            }
        } else {
            stack.push(seg);
        }
    }
    if escaped {
        return "/app".to_string();
    }
    if stack.is_empty() {
        "/app".to_string()
    } else {
        format!("/app/{}", stack.join("/"))
    }
}

fn normalize_file_manager_tool_args(v: Value) -> Value {
    match v {
        Value::Object(mut map) => {
            let keys: Vec<String> = map.keys().cloned().collect();
            for k in keys {
                let Some(val) = map.remove(&k) else {
                    continue;
                };
                let val = match k.as_str() {
                    "path" => match val {
                        Value::String(s) => Value::String(rewrite_file_manager_path(&s)),
                        other => normalize_file_manager_tool_args(other),
                    },
                    "paths" => match val {
                        Value::Array(arr) => Value::Array(
                            arr.into_iter()
                                .map(|item| match item {
                                    Value::String(s) => {
                                        Value::String(rewrite_file_manager_path(&s))
                                    }
                                    other => normalize_file_manager_tool_args(other),
                                })
                                .collect(),
                        ),
                        other => normalize_file_manager_tool_args(other),
                    },
                    _ => normalize_file_manager_tool_args(val),
                };
                map.insert(k, val);
            }
            Value::Object(map)
        }
        Value::Array(arr) => Value::Array(
            arr.into_iter()
                .map(normalize_file_manager_tool_args)
                .collect(),
        ),
        other => other,
    }
}

#[derive(Clone)]
pub enum Provider {
    Native(Arc<NativeProvider>),
    Mcp(Arc<McpClient>),
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
            .flat_map(|p| p.tools().iter())
            .filter(|t| should_expose_to_model(t))
            .cloned()
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
        let (provider, tool, direct) = self.resolve_tool(name)?;
        let args = match &provider {
            Provider::Mcp(c) if c.server_name == "te_pengine-file-manager" => {
                normalize_file_manager_tool_args(args)
            }
            _ => args,
        };
        let text = provider.call_tool(&tool, args).await?;
        Ok((text, direct))
    }

    pub fn resolve_tool(&self, name: &str) -> Result<(Provider, String, bool), String> {
        let (server, tool) = match name.split_once('.') {
            Some((s, t)) => (Some(s), t),
            None => (None, name),
        };

        if server.is_none() {
            let mut found: Vec<(&Provider, &ToolDef)> = Vec::new();
            for provider in &self.providers {
                if let Some(def) = provider.tools().iter().find(|t| t.name == tool) {
                    found.push((provider, def));
                }
            }
            return match found.len() {
                0 => Err(format!("tool not found: {name}")),
                1 => {
                    let (p, d) = found[0];
                    Ok((p.clone(), tool.to_string(), d.direct_return))
                }
                _ => {
                    let servers: Vec<_> = found.iter().map(|(p, _)| p.server_name()).collect();
                    Err(format!(
                        "ambiguous tool `{tool}`: matches servers {}",
                        servers.join(", ")
                    ))
                }
            };
        }

        if let Some(s) = server {
            let key = s.trim();
            for provider in &self.providers {
                if !provider.server_name().eq_ignore_ascii_case(key) {
                    continue;
                }
                if let Some(def) = provider.tools().iter().find(|t| t.name == tool) {
                    return Ok((provider.clone(), tool.to_string(), def.direct_return));
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_container_and_relative() {
        assert_eq!(
            rewrite_file_manager_path("/app/pengine/README.md"),
            "/app/pengine/README.md"
        );
        assert_eq!(
            rewrite_file_manager_path("/mcp/pengine/readme.md"),
            "/app/pengine/readme.md"
        );
        assert_eq!(rewrite_file_manager_path("/mcp"), "/mcp");
        assert_eq!(
            rewrite_file_manager_path("pengine/README.md"),
            "/app/pengine/README.md"
        );
        assert_eq!(rewrite_file_manager_path("README.md"), "/app/README.md");
    }

    #[test]
    fn relative_path_traversal_collapses_to_app_root() {
        assert_eq!(
            rewrite_file_manager_path("pengine/../../etc/passwd"),
            "/app"
        );
    }

    #[test]
    fn normalize_paths_in_arguments() {
        let raw = json!({ "path": "pengine/readme.md" });
        let out = normalize_file_manager_tool_args(raw);
        assert_eq!(out["path"], "/app/pengine/readme.md");
    }
}
