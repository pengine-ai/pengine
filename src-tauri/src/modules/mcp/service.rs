//! Public facade: build the tool registry from native providers.

use super::native;
use super::registry::{Provider, ToolRegistry};

/// Build a registry pre-loaded with all bundled native tools.
/// No I/O, no subprocess — instant.
pub fn build_default_registry() -> ToolRegistry {
    ToolRegistry::new(vec![Provider::Native(native::dice())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_registry_has_dice() {
        let reg = build_default_registry();
        assert_eq!(reg.tool_names(), &["roll_dice"]);
    }

    #[tokio::test]
    async fn native_dice_callable_through_registry() {
        let reg = build_default_registry();
        let (text, direct) = reg
            .call_tool("roll_dice", serde_json::json!({"sides": 6}))
            .await
            .expect("call roll_dice");
        assert!(text.starts_with("Rolled a d6: "), "got: {text}");
        assert!(direct, "dice should be direct_return");
    }

    /// Proves the reply comes from the native tool, not the model:
    ///
    /// 1. `direct_return` is true  → agent returns tool output verbatim,
    ///    the model never sees it and cannot rephrase.
    /// 2. The output matches `Rolled a dN: M` with M in [1, N] — a format
    ///    the native Rust handler produces. If the model fabricated a roll,
    ///    `direct_return` would be false and source would be `Model`.
    #[tokio::test]
    async fn dice_result_is_provably_from_tool_not_model() {
        let reg = build_default_registry();

        for sides in [6, 20, 100] {
            let (text, direct) = reg
                .call_tool("roll_dice", serde_json::json!({ "sides": sides }))
                .await
                .expect("call roll_dice");

            assert!(direct, "direct_return must be true for dice");

            let prefix = format!("Rolled a d{sides}: ");
            assert!(
                text.starts_with(&prefix),
                "expected prefix '{prefix}', got: {text}"
            );

            let num: u64 = text[prefix.len()..].trim().parse().expect("parse roll");
            assert!(
                (1..=sides).contains(&num),
                "roll {num} out of range [1, {sides}]"
            );
        }
    }
}
