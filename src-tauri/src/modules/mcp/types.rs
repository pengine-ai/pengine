use serde::{Deserialize, Serialize};

/// Definition of a single tool, regardless of where it runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    #[serde(skip)]
    pub server_name: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: serde_json::Value,
    /// When true the agent returns tool output verbatim, skipping the LLM
    /// summarization round-trip.  Good for deterministic tools like dice.
    #[serde(skip)]
    pub direct_return: bool,
}
