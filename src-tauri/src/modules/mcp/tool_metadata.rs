//! Heuristic `category` + `risk` for MCP tools (MCP payloads do not include these).

use super::types::{ToolDef, ToolRisk};

pub fn apply(tool: &mut ToolDef) {
    let n = tool.name.to_lowercase();
    let d = tool.description.as_deref().unwrap_or("").to_lowercase();

    let (category, risk) = classify(&n, &d);
    tool.category = Some(category);
    tool.risk = risk;
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
