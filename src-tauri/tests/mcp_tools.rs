//! Integration tests for MCP tooling.

use pengine_lib::modules::mcp::registry::ToolRegistry;
use pengine_lib::modules::mcp::{native, service};
use serde_json::json;
use std::path::PathBuf;

fn temp_mcp_path(name: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    p.push(format!("pengine-mcp-{name}-{nanos}.json"));
    p
}

#[tokio::test]
async fn dice_returns_valid_result() {
    let provider = native::dice();
    assert_eq!(provider.tools.len(), 1);
    assert_eq!(provider.tools[0].name, "roll_dice");
    assert!(provider.tools[0].direct_return);

    let out = provider
        .call("roll_dice", &json!({"sides": 20}))
        .await
        .expect("dice call");
    assert!(out.starts_with("Rolled a d20: "), "got: {out}");

    let num: u64 = out.trim_start_matches("Rolled a d20: ").parse().unwrap();
    assert!((1..=20).contains(&num));
}

#[tokio::test]
async fn dice_clamps_invalid_sides() {
    let provider = native::dice();

    let out = provider
        .call("roll_dice", &json!({"sides": 0}))
        .await
        .expect("sides=0");
    assert!(out.starts_with("Rolled a d2: "), "clamped to 2, got: {out}");

    let out = provider
        .call("roll_dice", &json!({"sides": 9999999}))
        .await
        .expect("sides=9999999");
    assert!(
        out.starts_with("Rolled a d1000000: "),
        "clamped to MAX, got: {out}"
    );
}

#[tokio::test]
async fn dice_rejects_unknown_tool() {
    let provider = native::dice();
    let err = provider.call("unknown", &json!({})).await.unwrap_err();
    assert!(err.contains("unknown native tool"));
}

#[tokio::test]
async fn mcp_json_loads_native_dice() {
    let path = temp_mcp_path("load");
    let cfg = service::load_or_init_config(&path).expect("load_or_init");
    assert!(cfg.servers.contains_key("dice"));

    let (providers, status) = service::build_mcp_providers(&cfg).await;
    let reg = ToolRegistry::new(providers);
    assert!(status
        .iter()
        .any(|s| s.contains("dice") && s.contains("native")));
    assert_eq!(reg.tool_names(), &["roll_dice"]);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn native_dice_callable_through_registry_from_config() {
    let path = temp_mcp_path("registry");
    let cfg = service::load_or_init_config(&path).expect("load_or_init");
    let (providers, _) = service::build_mcp_providers(&cfg).await;
    let reg = ToolRegistry::new(providers);
    let (text, direct) = reg
        .call_tool("roll_dice", json!({"sides": 6}))
        .await
        .expect("call roll_dice");
    assert!(text.starts_with("Rolled a d6: "), "got: {text}");
    assert!(direct, "dice should be direct_return");
    let _ = std::fs::remove_file(path);
}

#[test]
fn native_server_key_rename_in_config() {
    let path = temp_mcp_path("rename");
    std::fs::write(
        &path,
        r#"{"servers":{"mydice":{"type":"native","id":"dice"}}}"#,
    )
    .unwrap();
    let cfg = service::load_or_init_config(&path).expect("load");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (providers, _) = rt.block_on(service::build_mcp_providers(&cfg));
    let reg = ToolRegistry::new(providers);
    assert_eq!(reg.all_tools()[0].server_name, "mydice");
    let _ = std::fs::remove_file(path);
}
