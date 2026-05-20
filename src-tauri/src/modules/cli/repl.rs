//! Interactive shell. Entered via bare `pengine` in a TTY (or `pengine` from `pengine-cli`).
//!
//! Layered on top of [`super::router`] and [`super::handlers`]: the REPL reads
//! one line, classifies it, dispatches, and renders the reply — nothing
//! special to this file lives outside line editing and history management.

use super::banner::CLI_WELCOME;
use super::commands;
use super::dispatch::{dispatch_line, format_repl_line_for_audit, DispatchContext};
use super::flavor;
use super::folder_trust::{self, PromptOutcome};
use super::output::{render_reply, CliReply, OutputSink, RenderStyle, TerminalSink};
use super::session::{self, CliSession};
use crate::modules::mcp::service as mcp_service;
use crate::shared::state::AppState;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::{Hint, Hinter};
use rustyline::history::FileHistory;
use rustyline::validate::Validator;
use rustyline::{CompletionType, Config, Context, Editor, ExternalPrinter, Helper};
use std::borrow::Cow;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Window for "press Ctrl+C twice to exit". A second interrupt within this
/// duration breaks the REPL loop instead of just clearing the line.
const DOUBLE_INTERRUPT_WINDOW: Duration = Duration::from_secs(2);

// ── Slash-command completion / hint ─────────────────────────────────────────

/// Ghost-text hint returned by [`SlashHelper`].
struct SlashHint(String);

impl Hint for SlashHint {
    fn display(&self) -> &str {
        &self.0
    }
    fn completion(&self) -> Option<&str> {
        None
    }
}

/// rustyline [`Helper`] that provides:
/// - Tab-completion of `/command` names with their summaries
/// - Ghost-text hint that updates as the user types (filters live)
/// - Cyan highlight of the `/command` portion
struct SlashHelper;

impl Completer for SlashHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let Some(slash) = line[..pos].find('/') else {
            return Ok((0, vec![]));
        };
        let filter = line[slash + 1..pos].to_lowercase();

        let max_name = commands::COMMANDS
            .iter()
            .map(|c| c.name.len())
            .max()
            .unwrap_or(8);

        let candidates = commands::COMMANDS
            .iter()
            .filter(|c| {
                c.name != "quit"
                    && (filter.is_empty()
                        || c.name.contains(filter.as_str())
                        || c.summary.to_lowercase().contains(filter.as_str()))
            })
            .map(|c| Pair {
                display: format!("/{:<width$}  {}", c.name, c.summary, width = max_name),
                replacement: format!("/{}", c.name),
            })
            .collect();

        Ok((slash, candidates))
    }
}

impl Hinter for SlashHelper {
    type Hint = SlashHint;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<SlashHint> {
        // Only show when the cursor is at the end of the line.
        if pos < line.len() {
            return None;
        }
        let slash = line.find('/')?;
        // Ignore `/` that appears in the middle of a sentence.
        if slash > 0 && !line[..slash].trim().is_empty() {
            return None;
        }
        let filter = line[slash + 1..].to_lowercase();

        let matches: Vec<_> = commands::COMMANDS
            .iter()
            .filter(|c| {
                c.name != "quit"
                    && (filter.is_empty()
                        || c.name.starts_with(filter.as_str())
                        || c.summary.to_lowercase().contains(filter.as_str()))
            })
            .collect();

        if matches.is_empty() {
            return Some(SlashHint("\n\x1b[2m  (no matching command)\x1b[0m".into()));
        }

        let max_name = matches.iter().map(|c| c.name.len()).max().unwrap_or(8);
        let mut out = String::new();

        for cmd in &matches {
            // Clamp summary to avoid wrapping on narrow terminals.
            let summary: &str = if cmd.summary.len() > 58 {
                &cmd.summary[..58]
            } else {
                cmd.summary
            };
            out.push_str(&format!(
                "\n  \x1b[1;36m/{:<width$}\x1b[0m  \x1b[2m{summary}\x1b[0m",
                cmd.name,
                width = max_name
            ));
        }

        Some(SlashHint(out))
    }
}

impl Highlighter for SlashHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if line.trim_start().starts_with('/') {
            Cow::Owned(format!("\x1b[1;36m{line}\x1b[0m"))
        } else {
            Cow::Borrowed(line)
        }
    }

    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Borrowed(hint)
    }

    fn highlight_char(&self, line: &str, _pos: usize, _forced: bool) -> bool {
        // Re-render on every keystroke while in slash-command mode so the hint
        // updates live as the user types filter characters.
        line.trim_start().starts_with('/')
    }
}

impl Validator for SlashHelper {}
impl Helper for SlashHelper {}

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

    // Capture project context (cwd + git root + branch) and preset the session.
    // If bootstrap already loaded a session (e.g. `--continue`), keep it.
    // Otherwise auto-resume the most recent session for this project so the AI
    // remembers prior turns without the user needing `--continue` every time.
    let project = std::env::current_dir()
        .ok()
        .map(|cwd| session::detect_project_context(&cwd));

    let resumed_turns = {
        let mut guard = state.cli_session.write().await;
        match guard.as_mut() {
            Some(existing) if existing.project.is_none() => {
                existing.project = project.clone();
                existing.turns.len()
            }
            Some(existing) => existing.turns.len(),
            None => {
                // Auto-resume: try per-project first, fall back to global last.
                let loaded = auto_load_session(&state.store_path, project.as_ref());
                let n = loaded.as_ref().map(|s| s.turns.len()).unwrap_or(0);
                *guard = Some(match loaded {
                    Some(s) => s,
                    None => match project.clone() {
                        Some(p) => CliSession::fresh_with_project(p),
                        None => CliSession::fresh(),
                    },
                });
                n
            }
        }
    };

    let mut banner = format!(
        "{}\
\n\
Pengine REPL — slash commands + free text; /exit or Ctrl+D to quit.\n\
store:     {}{}",
        CLI_WELCOME.trim_start_matches('\n'),
        state.store_path.display(),
        format_project_banner_lines(project.as_ref()),
    );
    if resumed_turns > 0 {
        banner.push_str(&format!(
            "\nsession:   resumed {resumed_turns} turn(s)  \
             — /compact to summarize, /new to start fresh"
        ));
    }
    sink.render(&CliReply::text(banner));
    if std::io::stdout().is_terminal() {
        sink.render(&CliReply::text(format!(
            "\n\x1b[2m{}\x1b[0m",
            flavor::repl_tagline()
        )));
        sink.render(&CliReply::text(
            "\n\x1b[2m  Commands:\x1b[0m  /help  ·  /status  ·  /tools  ·  /model  ·  /clear  ·  /exit\n\
\x1b[2m  Tip:\x1b[0m type freely to talk to the agent — slash commands skip the model.\n",
        ));
    }

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

    // Build the editor before MCP warmup so we can extract an ExternalPrinter.
    // ExternalPrinter lets a background task print above the readline prompt
    // without corrupting the user's current input line.
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

    // ExternalPrinter lets the background warmup task notify the user safely.
    let ext_printer = rl.create_external_printer().ok();

    // Best-effort MCP warmup so /tools and agent turns land with tools available.
    // Failure is non-fatal — some REPL commands don't need MCP (/config, /status).
    //
    // UX guard: cold Podman/container startup can take 30+ seconds. Don't block
    // the prompt on that; continue in background when the timeout fires.
    match tokio::time::timeout(
        Duration::from_secs(8),
        mcp_service::rebuild_registry_into_state(state),
    )
    .await
    {
        Ok(Ok(())) => {
            let n = state.mcp.read().await.tool_names().len();
            if tty {
                eprintln!("  \x1b[2m⎿  MCP ready · {n} tools\x1b[0m");
            }
        }
        Ok(Err(e)) => {
            sink.render(&CliReply::error(format!("mcp warmup: {e}")));
        }
        Err(_) => {
            // Timeout — show what connected so far and continue in background.
            let n_now = state.mcp.read().await.tool_names().len();
            if tty {
                let connected = mcp_connected_stdio_labels(state).await;
                let server_line = if connected.is_empty() {
                    String::new()
                } else {
                    format!(
                        "\n     \x1b[2mConnected: {}\x1b[0m",
                        connected.join("  ·  ")
                    )
                };
                eprintln!(
                    "  \x1b[2m⎿  MCP: {n_now} tools ready · background servers still connecting…{server_line}\x1b[0m"
                );
            } else {
                sink.render(&CliReply::text(
                    "mcp warmup is still running in background; the prompt is ready now.",
                ));
            }
            let bg_state = state.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = mcp_service::rebuild_registry_into_state(&bg_state).await {
                    bg_state
                        .emit_log("mcp", &format!("background warmup failed: {e}"))
                        .await;
                    if let Some(mut ep) = ext_printer {
                        let _ = ep.print(format!(
                            "  \x1b[2m⎿  MCP warmup error: {e}\x1b[0m"
                        ));
                    }
                    return;
                }
                let n = bg_state.mcp.read().await.tool_names().len();
                if let Some(mut ep) = ext_printer {
                    let _ = ep.print(format!(
                        "  \x1b[2m⎿  MCP ready · {n} tools\x1b[0m"
                    ));
                }
            });
        }
    }

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

/// Try to load the most recent CLI session for `project` from disk.
/// Falls back to the global last session when no project-specific one exists.
/// Returns `None` when there are no saved sessions at all.
fn auto_load_session(
    store_path: &std::path::Path,
    project: Option<&session::ProjectContext>,
) -> Option<session::CliSession> {
    if let Some(p) = project {
        if let Ok(Some(s)) = session::load_last_for_path(store_path, p.match_key()) {
            return Some(s);
        }
    }
    session::load_last(store_path).ok().flatten()
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

fn build_editor() -> Result<Editor<SlashHelper, FileHistory>, String> {
    let cfg = Config::builder()
        .auto_add_history(false)
        .completion_type(CompletionType::List)
        .build();
    let mut rl = Editor::with_config(cfg).map_err(|e| e.to_string())?;
    rl.set_helper(Some(SlashHelper));
    Ok(rl)
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

/// Render the `project:` and `branch:` banner lines, abbreviating `$HOME` to
/// `~` so the start screen stays readable on long paths. Returns an empty
/// string when there is no project context (no cwd available).
fn format_project_banner_lines(project: Option<&session::ProjectContext>) -> String {
    let Some(project) = project else {
        return String::new();
    };
    let display_root = project.git_root.as_deref().unwrap_or(project.cwd.as_path());
    let project_str = abbreviate_home(display_root);
    let mut out = format!("\nproject:   {project_str}");
    if let Some(branch) = project.git_branch.as_deref() {
        out.push_str(&format!("\nbranch:    {branch}"));
    }
    out
}

/// Returns display labels for non-native MCP providers that are currently
/// registered. Strips the "te_pengine-" prefix so labels stay short.
async fn mcp_connected_stdio_labels(state: &AppState) -> Vec<String> {
    use crate::modules::mcp::registry::Provider;
    state
        .mcp
        .read()
        .await
        .providers()
        .iter()
        .filter(|p| matches!(p, Provider::Mcp(_)))
        .map(|p| {
            p.server_name()
                .strip_prefix("te_pengine-")
                .unwrap_or(p.server_name())
                .to_string()
        })
        .collect()
}

fn abbreviate_home(p: &std::path::Path) -> String {
    let raw = p.to_string_lossy();
    if let Ok(home) = std::env::var("HOME") {
        if let Some(rest) = raw.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    raw.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_empty_for_no_project() {
        assert_eq!(format_project_banner_lines(None), "");
    }

    #[test]
    fn banner_shows_project_and_branch() {
        let p = session::ProjectContext {
            cwd: std::path::PathBuf::from("/tmp/repo/sub"),
            git_root: Some(std::path::PathBuf::from("/tmp/repo")),
            git_branch: Some("main".into()),
        };
        let out = format_project_banner_lines(Some(&p));
        assert!(out.contains("project:   /tmp/repo"));
        assert!(out.contains("branch:    main"));
    }

    #[test]
    fn banner_omits_branch_outside_repo() {
        let p = session::ProjectContext {
            cwd: std::path::PathBuf::from("/tmp/loose"),
            git_root: None,
            git_branch: None,
        };
        let out = format_project_banner_lines(Some(&p));
        assert!(out.contains("project:   /tmp/loose"));
        assert!(!out.contains("branch:"));
    }
}
