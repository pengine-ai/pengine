//! Interactive shell. Entered via bare `pengine` in a TTY (or `pengine` from `pengine-cli`).
//!
//! Layered on top of [`super::router`] and [`super::handlers`]: the REPL reads
//! one line, classifies it, dispatches, and renders the reply — nothing
//! special to this file lives outside line editing and history management.

use super::banner::CLI_WELCOME;
use super::dispatch::{dispatch_line, format_repl_line_for_audit, DispatchContext};
use super::output::{render_reply, CliReply, OutputSink, RenderStyle, TerminalSink};
use crate::modules::mcp::service as mcp_service;
use crate::shared::state::AppState;
use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use rustyline::{Config, Editor};
use std::io::IsTerminal;
use std::path::PathBuf;

/// Styled prompt when stdout is a TTY (cyan-bold `❯`). Falls back to plain
/// `>` when piped, so history grepping stays readable.
const PROMPT_TTY: &str = "\x1b[1;36m❯\x1b[0m ";
const PROMPT_PLAIN: &str = "> ";

pub async fn run(state: &AppState) -> CliReply {
    let sink = TerminalSink::new();
    sink.render(&CliReply::text(format!(
        "{}\
\n\
Pengine REPL — slash commands + free text; /exit or Ctrl+D to quit.\n\
store:     {}",
        CLI_WELCOME.trim_start_matches('\n'),
        state.store_path.display()
    )));

    // Best-effort MCP warmup so /tools and free-text /ask land with tools
    // available. Failure is reported but non-fatal — some REPL commands don't
    // need MCP (e.g. /config, /status).
    if let Err(e) = mcp_service::rebuild_registry_into_state(state).await {
        sink.render(&CliReply::error(format!("mcp warmup skipped: {e}")));
    }

    let history_path = history_path(&state.store_path);
    let mut rl = match build_editor() {
        Ok(r) => r,
        Err(e) => return CliReply::error(format!("repl: editor init failed: {e}")),
    };
    let _ = rl.load_history(&history_path);

    let prompt = if std::io::stdout().is_terminal() {
        PROMPT_TTY
    } else {
        PROMPT_PLAIN
    };

    loop {
        match rl.readline(prompt) {
            Ok(line) => {
                let line = line.trim_end_matches('\n').to_string();
                if line.trim().is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line.as_str());
                if is_exit(&line) {
                    break;
                }
                let audit = format_repl_line_for_audit(&line);
                if !audit.is_empty() {
                    state.emit_log("cli", &format!("repl {audit}")).await;
                }
                let reply = dispatch_line(state, &line, DispatchContext::default()).await;
                render_reply(&sink, &reply, RenderStyle::ReplIndent);
            }
            Err(ReadlineError::Interrupted) => continue, // ^C clears the line
            Err(ReadlineError::Eof) => break,            // ^D exits
            Err(e) => {
                render_reply(
                    &sink,
                    &CliReply::error(format!("repl: {e}")),
                    RenderStyle::ReplIndent,
                );
                break;
            }
        }
    }

    let _ = rl.save_history(&history_path);
    CliReply::text("bye.")
}

fn build_editor() -> Result<Editor<(), FileHistory>, String> {
    let cfg = Config::builder().auto_add_history(false).build();
    Editor::with_config(cfg).map_err(|e| e.to_string())
}

fn history_path(store_path: &std::path::Path) -> PathBuf {
    store_path
        .parent()
        .map(|p| p.join("cli_history"))
        .unwrap_or_else(|| PathBuf::from("cli_history"))
}

fn is_exit(line: &str) -> bool {
    let t = line.trim();
    matches!(t, "/exit" | "/quit" | "exit" | "quit")
}
