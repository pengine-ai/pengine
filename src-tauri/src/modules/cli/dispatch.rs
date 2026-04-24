//! Shared argv/REPL/Telegram `$` dispatch — one implementation for router outcomes.
//!
//! Kept separate from [`super::repl`] so [`super::telegram_bridge`] can call it
//! without pulling in `rustyline`.

use super::handlers;
use super::output::CliReply;
use super::router::{self, RouterOutcome};
use crate::shared::state::AppState;

/// Where the line is being executed (affects safety rails).
#[derive(Clone, Copy, Default)]
pub struct DispatchContext {
    /// When true, disallow blocking operations that would stall the Telegram bot.
    pub telegram_surface: bool,
}

impl DispatchContext {
    pub fn telegram() -> Self {
        Self {
            telegram_surface: true,
        }
    }
}

/// Redact secrets and cap length for `AppState::emit_log` / audit NDJSON (terminal REPL).
pub(crate) fn format_repl_line_for_audit(line: &str) -> String {
    let t = line.trim();
    if t.is_empty() {
        return String::new();
    }
    fn starts_with_ci(hay: &str, needle: &str) -> bool {
        let h = hay.len();
        let n = needle.len();
        h >= n && hay[..n].eq_ignore_ascii_case(needle)
    }
    if starts_with_ci(t, "/bot connect ") {
        return "/bot connect <redacted>".to_string();
    }
    if starts_with_ci(t, "bot connect ") {
        return "bot connect <redacted>".to_string();
    }
    const MAX: usize = 2048;
    let n = t.chars().count();
    if n <= MAX {
        return t.to_string();
    }
    let head: String = t.chars().take(MAX).collect();
    format!("{head}… ({n} chars)")
}

pub async fn dispatch_line(state: &AppState, line: &str, ctx: DispatchContext) -> CliReply {
    match router::classify_line(line) {
        RouterOutcome::Unknown(name) => {
            CliReply::error(format!("unknown command: /{name} (try /help)",))
        }
        RouterOutcome::Agent(text) => handlers::ask(state, text).await,
        RouterOutcome::Native { name, rest } => dispatch_native(state, name, rest, ctx).await,
    }
}

async fn dispatch_native(
    state: &AppState,
    name: &str,
    rest: &str,
    ctx: DispatchContext,
) -> CliReply {
    match name {
        "help" => handlers::help(),
        "version" => handlers::version(),
        "status" => handlers::status(state).await,
        "config" => {
            let kvs: Vec<String> = rest.split_whitespace().map(str::to_string).collect();
            handlers::config(state, &kvs).await
        }
        "model" => {
            let (first, _) = split_first(rest);
            if first == "--clear" || first == "clear" {
                handlers::model(state, None, true).await
            } else {
                handlers::model(state, (!first.is_empty()).then_some(first), false).await
            }
        }
        "bot" => {
            let (action, tail) = split_first(rest);
            match action {
                "connect" => handlers::bot_connect(state, tail.trim()).await,
                "disconnect" => handlers::bot_disconnect(state).await,
                "" => CliReply::error("bot: expected `connect <token>` or `disconnect`"),
                other => CliReply::error(format!("bot: unknown action `{other}`")),
            }
        }
        "tools" => {
            let trimmed = rest.trim();
            let search = (!trimmed.is_empty()).then_some(trimmed);
            handlers::tools(state, search).await
        }
        "skills" => {
            let (action, tail) = split_first(rest);
            let slug_tok = tail.trim();
            let slug = (!slug_tok.is_empty()).then_some(slug_tok);
            let action = (!action.is_empty()).then_some(action);
            handlers::skills(state, action, slug).await
        }
        "fs" => {
            let (action, tail) = split_first(rest);
            let path_tok = tail.trim();
            let path = (!path_tok.is_empty()).then_some(path_tok);
            let action = (!action.is_empty()).then_some(action);
            handlers::fs(state, action, path).await
        }
        "logs" => {
            let mut follow = false;
            let mut tail: Option<usize> = None;
            let mut toks = rest.split_whitespace().peekable();
            while let Some(t) = toks.next() {
                match t {
                    "--follow" | "-f" => follow = true,
                    "--tail" => tail = toks.next().and_then(|s| s.parse().ok()),
                    _ => {}
                }
            }
            if ctx.telegram_surface && follow {
                return CliReply::error(
                    "logs --follow is not supported over the Telegram `$` bridge (it would block the bot).",
                );
            }
            handlers::logs(state, tail, follow).await
        }
        "ask" => handlers::ask(state, rest).await,
        "app" => {
            if ctx.telegram_surface {
                return CliReply::error("app: starting the GUI is not supported over Telegram.");
            }
            match super::bootstrap::spawn_gui_app_process() {
                Ok(()) => CliReply::text(
                    "Started the Pengine desktop window in a separate process. You can keep this REPL open.",
                ),
                Err(e) => CliReply::error(e),
            }
        }
        "exit" | "quit" => CliReply::text("bye."),
        other => CliReply::error(format!("unknown command: /{other}")),
    }
}

fn split_first(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shared::state::AppState;
    use std::path::PathBuf;

    #[test]
    fn repl_audit_redacts_bot_connect() {
        assert_eq!(
            format_repl_line_for_audit("  /BOT CONNECT secret-token-here  "),
            "/bot connect <redacted>"
        );
        assert_eq!(
            format_repl_line_for_audit("bot connect abc"),
            "bot connect <redacted>"
        );
    }

    #[test]
    fn repl_audit_truncates_long_input() {
        let s = "x".repeat(3000);
        let out = format_repl_line_for_audit(&s);
        assert!(out.contains('…'));
        assert!(out.contains("3000 chars"));
        assert!(out.len() < 3200);
    }

    fn minimal_state() -> AppState {
        let store = PathBuf::from("/nonexistent/pengine-test/connection.json");
        let (state, _rx) = AppState::new(
            store.clone(),
            store.with_file_name("mcp.json"),
            "default".into(),
        );
        state
    }

    #[tokio::test]
    async fn telegram_context_rejects_logs_follow() {
        let state = minimal_state();
        let reply = dispatch_line(&state, "/logs --follow", DispatchContext::telegram()).await;
        assert!(matches!(
            reply.kind,
            crate::modules::cli::output::ReplyKind::Error
        ));
        assert!(reply.body.contains("logs --follow"));
    }
}
