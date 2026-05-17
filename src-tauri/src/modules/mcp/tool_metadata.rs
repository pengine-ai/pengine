//! Heuristic `category` + `risk` for MCP tools (MCP payloads do not include these).

use super::types::{ToolDef, ToolRisk};
use serde_json::{json, Value};

pub fn apply(tool: &mut ToolDef) {
    let n = tool.name.to_lowercase();
    let d = tool.description.as_deref().unwrap_or("").to_lowercase();

    let (category, risk) = classify(&n, &d);
    tool.category = Some(category);
    tool.risk = risk;
    ensure_directory_tree_path_required(tool);
}

/// Models sometimes call `directory_tree` with no arguments. If the MCP schema exposes `path`,
/// enforce JSON Schema `required: ["path"]` so runtimes that honor `required` reject empty calls.
fn ensure_directory_tree_path_required(tool: &mut ToolDef) {
    if !tool.name.eq_ignore_ascii_case("directory_tree") {
        return;
    }
    let Some(root) = tool.input_schema.as_object_mut() else {
        return;
    };
    let has_path = root
        .get("properties")
        .and_then(|p| p.as_object())
        .is_some_and(|props| props.contains_key("path"));
    if !has_path {
        return;
    }
    match root.get_mut("required") {
        Some(Value::Array(arr)) => {
            if !arr.iter().any(|v| v.as_str() == Some("path")) {
                arr.push(json!("path"));
            }
        }
        Some(_) => {
            root.insert("required".into(), json!(["path"]));
        }
        None => {
            root.insert("required".into(), json!(["path"]));
        }
    }
}

fn classify(name: &str, desc: &str) -> (String, ToolRisk) {
    if name == "fetch" || name.contains("http") || desc.contains("http") || desc.contains("url") {
        return ("web".into(), ToolRisk::Low);
    }
    if name == "time" || name.contains("clock") {
        return ("utility".into(), ToolRisk::Low);
    }
    if name == "roll_dice" {
        return ("utility".into(), ToolRisk::Low);
    }
    if name == "manage_tools" || desc.contains("uninstall") && desc.contains("catalog") {
        return ("system".into(), ToolRisk::High);
    }
    if matches!(
        name,
        "create_entities"
            | "add_observations"
            | "create_relations"
            | "delete_entities"
            | "delete_observations"
            | "delete_relations"
    ) || desc.contains("knowledge graph")
        || desc.contains("entity")
    {
        return ("memory".into(), ToolRisk::Medium);
    }
    if name.contains("search_nodes") || name.contains("open_nodes") || name.contains("read_graph") {
        return ("memory".into(), ToolRisk::Low);
    }
    if name.contains("write")
        || name.contains("delete")
        || name.contains("remove")
        || name.contains("exec")
        || name.contains("run_terminal")
        || name.contains("shell")
    {
        return ("filesystem".into(), ToolRisk::High);
    }
    if name.contains("read") || name.contains("list") || name.contains("directory") {
        return ("filesystem".into(), ToolRisk::Medium);
    }
    ("other".into(), ToolRisk::Medium)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn directory_tree_schema_gets_required_path() {
        let mut tool = ToolDef {
            server_name: "te_test".into(),
            name: "directory_tree".into(),
            description: Some("tree".into()),
            input_schema: json!({
                "type": "object",
                "properties": { "path": { "type": "string" } }
            }),
            direct_return: false,
            category: None,
            risk: ToolRisk::Low,
        };
        apply(&mut tool);
        assert_eq!(tool.input_schema["required"], json!(["path"]));
    }
}
