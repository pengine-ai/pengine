//! Interactive shell. Entered via bare `pengine` in a TTY (or `pengine` from `pengine-cli`).
//!
//! Layered on top of [`super::router`] and [`super::handlers`]: the REPL reads
//! one line, classifies it, dispatches, and renders the reply — nothing
//! special to this file lives outside line editing and history management.

use super::banner::CLI_WELCOME;
use super::dispatch::{dispatch_line, format_repl_line_for_audit, DispatchContext};
use super::folder_trust::{self, PromptOutcome};
use super::output::{render_reply, CliReply, OutputSink, RenderStyle, TerminalSink};
use crate::modules::mcp::service as mcp_service;
use crate::shared::state::AppState;
use rustyline::error::ReadlineError;
use rustyline::history::FileHistory;
use rustyline::{Config, Editor};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Window for "press Ctrl+C twice to exit". A second interrupt within this
/// duration breaks the REPL loop instead of just clearing the line.
const DOUBLE_INTERRUPT_WINDOW: Duration = Duration::from_secs(2);

/// Continuation prompt shown for additional lines while a backslash-escaped
/// multi-line edit is in progress.
const PROMPT_CONT_TTY: &str = "\x1b[2;36m·\x1b[0m ";
const PROMPT_CONT_PLAIN: &str = ". ";

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

    // First-run trust prompt: when starting in a folder not yet decided, ask
    // whether to add the cwd as an MCP filesystem root. Skipped when stdin is
    // not a TTY, when the folder is already covered, or when the user has
    // previously decided. Must run *before* MCP warmup so a "yes" is included
    // in the registry rebuild.
    if let Ok(cwd) = std::env::current_dir() {
        match folder_trust::maybe_prompt_for_cwd(state, &cwd).await {
            Ok(PromptOutcome::Added) => {
                sink.render(&CliReply::text(format!(
                    "  ⎿  added {} to MCP filesystem roots",
                    cwd.display()
                )));
                state
                    .emit_log(
                        "cli",
                        &format!("trust: added {} to mcp fs roots", cwd.display()),
                    )
                    .await;
            }
            Ok(PromptOutcome::Declined) => {
                sink.render(&CliReply::text(
                    "  ⎿  folder not added (saved; will not ask again for this path)",
                ));
                state
                    .emit_log("cli", &format!("trust: declined {}", cwd.display()))
                    .await;
            }
            Ok(_) => {}
            Err(e) => sink.render(&CliReply::error(format!("trust prompt: {e}"))),
        }
    }

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

    let tty = std::io::stdout().is_terminal();
    let (prompt, cont_prompt) = if tty {
        (PROMPT_TTY, PROMPT_CONT_TTY)
    } else {
        (PROMPT_PLAIN, PROMPT_CONT_PLAIN)
    };

    let mut last_interrupt: Option<Instant> = None;

    loop {
        let first = match rl.readline(prompt) {
            Ok(l) => {
                last_interrupt = None;
                l
            }
            Err(ReadlineError::Interrupted) => {
                if last_interrupt
                    .map(|t| t.elapsed() < DOUBLE_INTERRUPT_WINDOW)
                    .unwrap_or(false)
                {
                    sink.render(&CliReply::text("(interrupted twice — exiting)"));
                    break;
                }
                last_interrupt = Some(Instant::now());
                if tty {
                    sink.render(&CliReply::text(
                        "(press Ctrl+C again to exit, or type /exit)",
                    ));
                }
                continue;
            }
            Err(ReadlineError::Eof) => break,
            Err(e) => {
                render_reply(
                    &sink,
                    &CliReply::error(format!("repl: {e}")),
                    RenderStyle::ReplIndent,
                );
                break;
            }
        };

        let mut line = first.trim_end_matches('\n').to_string();
        // Backslash-newline continuation — read additional lines until the
        // edit ends without a trailing `\`. Empty continuation lines stay
        // in the joined message so paste of multi-paragraph prose survives.
        while line.ends_with('\\') {
            line.pop();
            line.push('\n');
            match rl.readline(cont_prompt) {
                Ok(more) => line.push_str(more.trim_end_matches('\n')),
                Err(ReadlineError::Interrupted) => {
                    sink.render(&CliReply::text("(multi-line edit cancelled)"));
                    line.clear();
                    break;
                }
                Err(ReadlineError::Eof) => break,
                Err(e) => {
                    render_reply(
                        &sink,
                        &CliReply::error(format!("repl: {e}")),
                        RenderStyle::ReplIndent,
                    );
                    line.clear();
                    break;
                }
            }
        }

        let line = line;
        if line.trim().is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(line.as_str());
        if is_exit(&line) {
            break;
        }
        if is_clear_command(&line) {
            clear_screen(tty);
            continue;
        }
        let audit = format_repl_line_for_audit(&line);
        if !audit.is_empty() {
            state.emit_log("cli", &format!("repl {audit}")).await;
        }
        let reply = dispatch_line(state, &line, DispatchContext::default()).await;
        render_reply(&sink, &reply, RenderStyle::ReplIndent);
    }

    let _ = rl.save_history(&history_path);
    CliReply::text("bye.")
}

fn is_clear_command(line: &str) -> bool {
    let t = line.trim();
    matches!(t, "/clear" | "clear")
}

fn clear_screen(tty: bool) {
    if !tty {
        println!();
        return;
    }
    use std::io::Write;
    // ESC[2J clears screen, ESC[H moves cursor to home.
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b"\x1b[2J\x1b[H");
    let _ = out.flush();
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
