//! Built-in tools that run in-process. No subprocess, no JSON-RPC overhead.

use super::types::ToolDef;
use serde_json::{json, Value};

const MAX_SIDES: u64 = 1_000_000;

pub struct NativeProvider {
    pub server_name: String,
    pub tools: Vec<ToolDef>,
    handler: fn(&str, &Value) -> Result<String, String>,
}

impl NativeProvider {
    pub fn call(&self, tool_name: &str, args: &Value) -> Result<String, String> {
        if !self.tools.iter().any(|t| t.name == tool_name) {
            return Err(format!("unknown native tool: {tool_name}"));
        }
        (self.handler)(tool_name, args)
    }
}

/// The bundled dice tool — runs in pure Rust, instant response.
pub fn dice() -> NativeProvider {
    NativeProvider {
        server_name: "dice".to_string(),
        tools: vec![ToolDef {
            server_name: "dice".to_string(),
            name: "roll_dice".to_string(),
            description: Some(
                "Roll a die with the given number of sides and return the result.".to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "sides": {
                        "type": "integer",
                        "description": "Number of sides (default 6, max 1 000 000)"
                    }
                }
            }),
            direct_return: true,
        }],
        handler: handle_dice,
    }
}

fn handle_dice(_tool_name: &str, args: &Value) -> Result<String, String> {
    let sides = args
        .get("sides")
        .and_then(|v| v.as_u64())
        .unwrap_or(6)
        .clamp(2, MAX_SIDES);

    let result = fastrand::u64(1..=sides);
    Ok(format!("Rolled a d{sides}: {result}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dice_returns_valid_result() {
        let provider = dice();
        assert_eq!(provider.tools.len(), 1);
        assert_eq!(provider.tools[0].name, "roll_dice");
        assert!(provider.tools[0].direct_return);

        let out = provider
            .call("roll_dice", &json!({"sides": 20}))
            .expect("dice call");
        assert!(out.starts_with("Rolled a d20: "), "got: {out}");

        let num: u64 = out.trim_start_matches("Rolled a d20: ").parse().unwrap();
        assert!((1..=20).contains(&num));
    }

    #[test]
    fn dice_clamps_invalid_sides() {
        let provider = dice();

        let out = provider
            .call("roll_dice", &json!({"sides": 0}))
            .expect("sides=0");
        assert!(out.starts_with("Rolled a d2: "), "clamped to 2, got: {out}");

        let out = provider
            .call("roll_dice", &json!({"sides": 9999999}))
            .expect("sides=9999999");
        assert!(
            out.starts_with("Rolled a d1000000: "),
            "clamped to MAX, got: {out}"
        );
    }

    #[test]
    fn dice_rejects_unknown_tool() {
        let provider = dice();
        let err = provider.call("unknown", &json!({})).unwrap_err();
        assert!(err.contains("unknown native tool"));
    }
}
