//! Telegram `$` prefix → same router + handlers as the REPL, rendered for Telegram.
//!
//! Policy (see `cli_plan.md` §10): lines whose trimmed text starts with `$`
//! are CLI intent; the rest of the line is classified by [`super::router`].
//! Non-`$` messages stay on the normal [`crate::modules::agent::run_turn`] path.

use super::dispatch::{dispatch_line, DispatchContext};
use super::output::{CliReply, ReplyKind};
use crate::modules::mcp::service as mcp_service;
use crate::shared::state::AppState;
use crate::shared::text::split_by_chars;

/// Same UTF-16 safety rationale as `bot/service.rs::TELEGRAM_CHUNK_BUDGET`.
const TELEGRAM_CLI_CHUNK_BUDGET: usize = 2000;

/// If `msg` is CLI intent (`$…`), returns the payload after `$` (may be empty).
/// Otherwise returns [`None`] so the caller should run the normal agent path.
pub fn strip_dollar_cli_payload(msg: &str) -> Option<&str> {
    let trimmed = msg.trim_start();
    trimmed.strip_prefix('$').map(|rest| rest.trim_start())
}

/// Run one CLI-classified line for Telegram (MCP warmup + dispatch with Telegram rails).
pub async fn run_telegram_cli_line(state: &AppState, line_after_dollar: &str) -> CliReply {
    if line_after_dollar.trim().is_empty() {
        return CliReply::error(
            "empty message after `$`; try e.g. `$ /help`, `$ /status`, or text without `$` for the agent.",
        );
    }
    if let Err(e) = mcp_service::rebuild_registry_into_state(state).await {
        log::warn!("telegram cli: mcp warmup failed (continuing): {e}");
    }
    dispatch_line(state, line_after_dollar, DispatchContext::telegram()).await
}

/// Format a [`CliReply`] as Telegram message text, then chunk under the UTF-16-safe budget.
pub fn telegram_cli_reply_chunks(reply: &CliReply) -> Vec<String> {
    let formatted = format_reply_body_telegram(reply);
    split_by_chars(&formatted, TELEGRAM_CLI_CHUNK_BUDGET)
}

fn format_reply_body_telegram(reply: &CliReply) -> String {
    match &reply.kind {
        ReplyKind::Text => reply.body.clone(),
        ReplyKind::Error => {
            if reply.body.is_empty() {
                "Error".to_string()
            } else {
                format!("Error:\n{}", reply.body)
            }
        }
        ReplyKind::CodeBlock { lang } => fenced(lang, &reply.body),
        ReplyKind::Log => fenced("bash", &reply.body),
        ReplyKind::Diff => fenced("diff", &reply.body),
    }
}

fn fenced(lang: &str, body: &str) -> String {
    format!("```{lang}\n{}\n```", body.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::cli::output::CliReply;

    #[test]
    fn strip_dollar_none_for_plain_agent() {
        assert_eq!(strip_dollar_cli_payload("hello"), None);
    }

    #[test]
    fn strip_dollar_some_payload() {
        assert_eq!(strip_dollar_cli_payload("$ /status"), Some("/status"));
        assert_eq!(strip_dollar_cli_payload("  $  hello "), Some("hello "));
    }

    #[test]
    fn telegram_chunks_fence_diff() {
        let reply = CliReply::diff("+a\n-b\n");
        let chunks = telegram_cli_reply_chunks(&reply);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("```diff"));
    }
}
