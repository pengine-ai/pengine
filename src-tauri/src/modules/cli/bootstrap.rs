//! CLI entry branch — reads `tauri-plugin-cli` matches and dispatches.
//!
//! Invoked from `app::run` before any window is created. If no CLI subcommand
//! is present, returns and setup continues into the normal UI path.
//! Otherwise runs the handler, prints to the chosen sink, and exits the
//! process. No Tauri event loop is needed for one-shot commands.
//!
//! **Bare `pengine`** (no subcommand): in a real terminal (**TTY**), starts the
//! interactive REPL only — never the GUI in that process. Without a TTY the
//! launch is treated as a GUI launch (Finder / Dock / `.desktop` file /
//! Windows Start menu / `open -a pengine`) and setup continues into
//! [`crate::app::open_main_window`].
//!
//! **`pengine app`** spawns a **separate** GUI process (`PENGINE_OPEN_GUI=1`) so
//! the shell and the desktop can run in parallel.
//!
//! **`PENGINE_LAUNCH_MODE=cli`** (e.g. `pengine-cli` launcher) or **`--shell`**:
//! never opens the GUI in-process. With no subcommand, a TTY is required for
//! the REPL; without a TTY the process exits with an error instead.

use super::output::{CliReply, JsonSink, OutputSink, TerminalSink};
use super::{commands, handlers};
use crate::infrastructure::audit_log;
use crate::modules::bot::repository as bot_repository;
use crate::modules::mcp::service as mcp_service;
use crate::modules::secure_store;
use crate::shared::state::{AppState, ConnectionData};
use serde_json::Value;
use std::collections::HashMap;
use std::io::IsTerminal;
use tauri::Manager;
use tauri_plugin_cli::{ArgData, CliExt, Matches};

/// Entry — call from Tauri `setup`. Returns in UI mode; in CLI mode the
/// handler runs and [`std::process::exit`] is called.
///
/// Three paths, in priority order:
/// 1. `--help` / auto-`help` subcommand — tauri-plugin-cli surfaces clap's
///    generated text in `matches.args["help"]` (see its `parser.rs`).
/// 2. `--version` — surfaces an empty `matches.args["version"]`.
/// 3. A registered subcommand — dispatch via [`run_subcommand`].
///
/// Otherwise (bare `pengine`): **TTY** → REPL then exit; **not a TTY** → GUI
/// (all platforms; covers Finder / Dock / `.desktop` / Start-menu launches).
/// The `pengine-cli` launcher sets `PENGINE_LAUNCH_MODE=cli` (or `--shell`)
/// so non-TTY never falls through to the GUI there.
pub fn handle_cli_or_continue(app: &tauri::App) {
    if consume_gui_spawn_env() {
        return;
    }

    // Tauri defaults to `Regular` (foreground app with Dock icon). For CLI
    // invocations we don't want a Dock entry at all — make the process an
    // "accessory" up front. If we later decide this is a GUI launch (bare
    // `pengine` with no TTY), `switch_to_gui_activation_policy` flips it back
    // to `Regular` before we fall through to `setup`.
    set_macos_activation_policy(app, tauri::ActivationPolicy::Accessory);

    let matches = match app.cli().matches() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("cli parse error: {e}");
            std::process::exit(2);
        }
    };

    let json = flag_true(&matches.args, "json");
    let output_format = single_string(&matches.args, "output-format")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| {
            if json {
                "json".to_string()
            } else {
                "text".to_string()
            }
        });
    let sink: Box<dyn OutputSink> = match output_format.as_str() {
        "json" | "stream-json" => Box::new(JsonSink),
        _ => Box::new(TerminalSink::new()),
    };

    if let Some(arg) = matches.args.get("help") {
        if matches!(arg.value, Value::String(_)) {
            sink.render(&handlers::help(help_topic_from_argv().as_deref()));
            std::process::exit(0);
        }
    }
    if matches.args.contains_key("version") {
        sink.render(&handlers::version());
        std::process::exit(0);
    }

    // `-p` / `--print` short-circuits to a single agent turn and exits.
    if let Some(prompt) = single_string(&matches.args, "print") {
        let state = match build_state(app) {
            Ok(s) => s,
            Err(e) => {
                sink.render(&CliReply::error(format!("state: {e}")));
                std::process::exit(1);
            }
        };
        if flag_true(&matches.args, "continue") {
            if let Ok(Some(s)) = crate::modules::cli::session::load_last(&state.store_path) {
                tauri::async_runtime::block_on(async {
                    *state.cli_session.write().await = Some(s);
                });
            }
        }
        let reply = tauri::async_runtime::block_on(async {
            if let Err(e) = mcp_service::rebuild_registry_into_state(&state).await {
                return CliReply::error(format!("mcp warmup failed: {e}"));
            }
            handlers::ask_in_session(&state, &prompt, flag_true(&matches.args, "continue")).await
        });
        let is_error = matches!(reply.kind, crate::modules::cli::output::ReplyKind::Error);
        sink.render(&reply);
        std::process::exit(if is_error { 1 } else { 0 });
    }

    if matches.subcommand.is_none() {
        match argv_intent() {
            ArgvIntent::None => {
                let tty = std::io::stdin().is_terminal();
                let force_terminal_only =
                    force_cli_launch_mode() || flag_true(&matches.args, "shell");
                if force_terminal_only && !tty {
                    eprintln!(
                        "A terminal (TTY) is required for the interactive shell \
                         (`pengine --shell`, `pengine-cli`, or `PENGINE_LAUNCH_MODE=cli`)."
                    );
                    eprintln!(
                        "For one-shot use without a TTY, run e.g. `pengine status` or `pengine ask \"…\"`."
                    );
                    std::process::exit(1);
                }
                if !tty {
                    // Double-click from Finder / Dock / `.desktop` file /
                    // Windows Start menu / `open -a pengine` all land here:
                    // no CLI subcommand, no `-psn_` guarantee across platforms,
                    // no TTY. Treat it as a GUI launch — flip the activation
                    // policy back to `Regular` so the Dock icon appears, and
                    // return so `setup` continues into `open_main_window`.
                    set_macos_activation_policy(app, tauri::ActivationPolicy::Regular);
                    return;
                }
                let sink = TerminalSink::new();
                let state = match build_state(app) {
                    Ok(s) => s,
                    Err(e) => {
                        sink.render(&CliReply::error(format!("state: {e}")));
                        std::process::exit(1);
                    }
                };
                if flag_true(&matches.args, "continue") {
                    if let Ok(Some(s)) = crate::modules::cli::session::load_last(&state.store_path)
                    {
                        tauri::async_runtime::block_on(async {
                            *state.cli_session.write().await = Some(s);
                        });
                    }
                }
                let reply = tauri::async_runtime::block_on(super::repl::run(&state));
                sink.render(&reply);
                std::process::exit(0);
            }
            ArgvIntent::Help => {
                sink.render(&handlers::help(help_topic_from_argv().as_deref()));
                std::process::exit(0);
            }
            ArgvIntent::Version => {
                sink.render(&handlers::version());
                std::process::exit(0);
            }
            ArgvIntent::CommandLike => {
                let shown = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
                sink.render(&CliReply::error(format!(
                    "cli invocation detected (`{shown}`) but no subcommand was parsed; \
                     try `pengine help`"
                )));
                std::process::exit(2);
            }
        }
    }

    let code = run_subcommand(app, matches, sink.as_ref());
    std::process::exit(code);
}

fn run_subcommand(app: &tauri::App, matches: Matches, sink: &dyn OutputSink) -> i32 {
    let sub = matches
        .subcommand
        .expect("checked in handle_cli_or_continue");
    let name = sub.name.as_str();
    let sub_args = &sub.matches.args;
    let sub_inner = sub.matches.subcommand.as_deref();

    // Zero-state commands run without constructing AppState.
    match name {
        "help" => {
            // `help` here is clap's auto-generated subcommand. Read the topic
            // from argv since the subcommand schema isn't ours to extend.
            sink.render(&handlers::help(help_topic_from_argv().as_deref()));
            return 0;
        }
        "version" => {
            sink.render(&handlers::version());
            return 0;
        }
        "app" => match spawn_gui_app_process() {
            Ok(()) => {
                sink.render(&CliReply::text(
                    "Started the Pengine desktop window in a separate process. \
                     This terminal is free; run `pengine` here or in another tab for the shell — both can run at once.",
                ));
                return 0;
            }
            Err(e) => {
                sink.render(&CliReply::error(e));
                return 1;
            }
        },
        _ => {}
    }

    // Stateful commands: build a minimal AppState.
    let state = match build_state(app) {
        Ok(s) => s,
        Err(e) => {
            sink.render(&CliReply::error(format!("state: {e}")));
            return 1;
        }
    };

    let audit_line = cli_subcommand_audit_summary(name, sub_args, sub_inner);
    tauri::async_runtime::block_on(state.emit_log("cli", &audit_line));

    let reply =
        tauri::async_runtime::block_on(dispatch_stateful(name, sub_args, sub_inner, &state));
    let is_error = matches!(reply.kind, crate::modules::cli::output::ReplyKind::Error);
    sink.render(&reply);
    if is_error {
        1
    } else {
        0
    }
}

async fn dispatch_stateful(
    name: &str,
    args: &HashMap<String, ArgData>,
    sub: Option<&tauri_plugin_cli::SubcommandMatches>,
    state: &AppState,
) -> CliReply {
    match name {
        "status" => handlers::status(state).await,
        "doctor" => handlers::doctor(state).await,
        "plan" => {
            let action = single_string(args, "action");
            handlers::plan(state, action.as_deref()).await
        }
        "cost" => handlers::cost(state).await,
        "resume" => handlers::resume(state).await,
        "compact" => handlers::compact(state).await,
        "clear" => handlers::clear(),
        "config" => {
            let kvs = multi_string(args, "kv");
            handlers::config(state, &kvs).await
        }
        "model" => {
            let name = single_string(args, "name");
            let clear = flag_true(args, "clear");
            handlers::model(state, name.as_deref(), clear).await
        }
        "bot" => {
            let Some(inner) = sub else {
                return CliReply::error("bot: expected `connect <token>` or `disconnect`");
            };
            match inner.name.as_str() {
                "connect" => {
                    let token = single_string(&inner.matches.args, "token").unwrap_or_default();
                    handlers::bot_connect(state, &token).await
                }
                "disconnect" => handlers::bot_disconnect(state).await,
                other => CliReply::error(format!("bot: unknown subcommand `{other}`")),
            }
        }
        "tools" => {
            // Warm MCP so the list is meaningful.
            if let Err(e) = mcp_service::rebuild_registry_into_state(state).await {
                return CliReply::error(format!("mcp warmup failed: {e}"));
            }
            let search = single_string(args, "search");
            handlers::tools(state, search.as_deref()).await
        }
        "mcp" => {
            let action = single_string(args, "action").unwrap_or_default();
            let rest_tokens = multi_string(args, "rest");
            let rest = shellish_join(&rest_tokens);
            super::mcp_cmd::run_from_args(state, action.trim(), rest.trim()).await
        }
        "skills" => {
            let action = single_string(args, "action");
            let slug = single_string(args, "slug");
            handlers::skills(state, action.as_deref(), slug.as_deref()).await
        }
        "fs" => {
            let action = single_string(args, "action");
            let path = single_string(args, "path");
            handlers::fs(state, action.as_deref(), path.as_deref()).await
        }
        "logs" => {
            let follow = flag_true(args, "follow");
            let tail = single_string(args, "tail").and_then(|s| s.parse::<usize>().ok());
            handlers::logs(state, tail, follow).await
        }
        "ask" => {
            let text = single_string(args, "text").unwrap_or_default();
            if let Err(e) = mcp_service::rebuild_registry_into_state(state).await {
                return CliReply::error(format!("mcp warmup failed: {e}"));
            }
            let cont = flag_true(args, "continue");
            if cont {
                if let Ok(Some(s)) = crate::modules::cli::session::load_last(&state.store_path) {
                    *state.cli_session.write().await = Some(s);
                }
            }
            handlers::ask_in_session(state, &text, cont).await
        }
        other => CliReply::error(format!("unknown subcommand `{other}`")),
    }
}

fn build_state(app: &tauri::App) -> Result<AppState, String> {
    let base = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("app_data_dir: {e}"))?;
    let store_path = base.join("connection.json");
    let (mcp_path, mcp_src) = mcp_service::resolve_mcp_config_path(&store_path);
    let (state, audit_rx) = AppState::new(store_path, mcp_path, mcp_src.to_string());
    hydrate_connection_from_disk(&state);
    let audit_store = state.store_path.clone();
    tauri::async_runtime::spawn(async move {
        audit_log::run_audit_writer(audit_store, audit_rx).await;
    });
    Ok(state)
}

fn truncate_audit_str(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    let head: String = s.chars().take(max_chars).collect();
    format!("{head}… ({count} chars)")
}

/// One-line summary for NDJSON audit (no secrets).
fn cli_subcommand_audit_summary(
    name: &str,
    args: &HashMap<String, ArgData>,
    sub: Option<&tauri_plugin_cli::SubcommandMatches>,
) -> String {
    use std::fmt::Write;
    let mut out = String::from("pengine ");
    out.push_str(name);
    match name {
        "status" | "app" | "doctor" | "cost" | "resume" | "compact" | "clear" => {}
        "plan" => {
            if let Some(a) = single_string(args, "action") {
                let _ = write!(out, " {}", truncate_audit_str(&a, 32));
            }
        }
        "config" => {
            let kvs = multi_string(args, "kv");
            if !kvs.is_empty() {
                let _ = write!(out, " {}", truncate_audit_str(&kvs.join(" "), 400));
            }
        }
        "model" => {
            if let Some(n) = single_string(args, "name") {
                let _ = write!(out, " {}", truncate_audit_str(&n, 120));
            }
            if flag_true(args, "clear") {
                out.push_str(" --clear");
            }
        }
        "bot" => {
            if let Some(inner) = sub {
                match inner.name.as_str() {
                    "connect" => out.push_str(" connect <redacted>"),
                    "disconnect" => out.push_str(" disconnect"),
                    other => {
                        let _ = write!(out, " {other}");
                    }
                }
            }
        }
        "tools" => {
            if let Some(q) = single_string(args, "search") {
                let _ = write!(out, " {}", truncate_audit_str(&q, 200));
            }
        }
        "skills" => {
            if let Some(a) = single_string(args, "action") {
                let _ = write!(out, " {}", truncate_audit_str(&a, 64));
            }
            if let Some(sl) = single_string(args, "slug") {
                let _ = write!(out, " {}", truncate_audit_str(&sl, 120));
            }
        }
        "fs" => {
            if let Some(a) = single_string(args, "action") {
                let _ = write!(out, " {}", truncate_audit_str(&a, 32));
            }
            if let Some(p) = single_string(args, "path") {
                let _ = write!(out, " {}", truncate_audit_str(&p, 400));
            }
        }
        "mcp" => {
            if let Some(a) = single_string(args, "action") {
                let _ = write!(out, " {}", truncate_audit_str(&a, 32));
            }
            let rest = multi_string(args, "rest");
            if !rest.is_empty() {
                let joined = rest.join(" ");
                let _ = write!(out, " {}", truncate_audit_str(&joined, 400));
            }
        }
        "logs" => {
            if flag_true(args, "follow") {
                out.push_str(" --follow");
            }
            if let Some(t) = single_string(args, "tail").and_then(|x| x.parse::<usize>().ok()) {
                let _ = write!(out, " --tail {t}");
            }
        }
        "ask" => {
            let text = single_string(args, "text").unwrap_or_default();
            if !text.is_empty() {
                let _ = write!(out, " {}", truncate_audit_str(&text, 800));
            }
        }
        _ => out.push_str(" …"),
    }
    out
}

/// Best-effort hydration for one-shot CLI mode:
/// - status should reflect persisted bot metadata
/// - disconnect should have bot_id available for keychain cleanup
///
/// If keychain unlock fails, we still carry bot_id/bot_username with an empty
/// token so metadata-aware commands keep behaving deterministically.
fn hydrate_connection_from_disk(state: &AppState) {
    let mut migration_log: Vec<String> = Vec::new();
    let Some(meta) = bot_repository::load(&state.store_path, &mut migration_log) else {
        return;
    };
    let token = secure_store::load_token(&meta.bot_id).unwrap_or_default();
    tauri::async_runtime::block_on(async {
        *state.connection.lock().await = Some(ConnectionData {
            bot_token: token,
            bot_id: meta.bot_id,
            bot_username: meta.bot_username,
            connected_at: meta.connected_at,
        });
    });
}

fn flag_true(args: &HashMap<String, ArgData>, name: &str) -> bool {
    matches!(args.get(name).map(|a| &a.value), Some(Value::Bool(true)))
}

fn single_string(args: &HashMap<String, ArgData>, name: &str) -> Option<String> {
    match args.get(name)?.value {
        Value::String(ref s) => Some(s.clone()),
        _ => None,
    }
}

/// Re-join argv-style tokens into a single string the slash dispatch parser
/// can re-tokenize. Only quotes tokens that contain whitespace; everything
/// else passes through verbatim so simple flags stay readable in audit logs.
fn shellish_join(tokens: &[String]) -> String {
    tokens
        .iter()
        .map(|t| {
            if t.chars().any(char::is_whitespace) {
                format!("\"{}\"", t.replace('"', "\\\""))
            } else {
                t.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn multi_string(args: &HashMap<String, ArgData>, name: &str) -> Vec<String> {
    let Some(arg) = args.get(name) else {
        return Vec::new();
    };
    match &arg.value {
        Value::String(s) => vec![s.clone()],
        Value::Array(arr) => arr
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArgvIntent {
    None,
    Help,
    Version,
    CommandLike,
}

fn force_cli_launch_mode() -> bool {
    std::env::var("PENGINE_LAUNCH_MODE")
        .map(|v| v == "cli")
        .unwrap_or(false)
}

fn argv_intent() -> ArgvIntent {
    argv_intent_from(std::env::args().skip(1))
}

/// When the user runs `pengine help <topic>` (or `pengine --help <topic>`),
/// return `<topic>`. Returns `None` if no topic word is present after the help token.
fn help_topic_from_argv() -> Option<String> {
    let mut iter = std::env::args().skip(1).filter(|a| !is_ignored_os_arg(a));
    while let Some(a) = iter.next() {
        let t = a.trim();
        if matches!(t, "--help" | "-h" | "help") {
            return iter
                .find(|next| !next.starts_with('-'))
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
        }
    }
    None
}

fn argv_intent_from<I, S>(args: I) -> ArgvIntent
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let Some(first) = args
        .into_iter()
        .map(|a| a.as_ref().trim().to_string())
        .find(|a| !a.is_empty() && !is_ignored_os_arg(a))
    else {
        return ArgvIntent::None;
    };

    match first.as_str() {
        "--help" | "-h" | "help" => ArgvIntent::Help,
        "--version" | "-V" | "version" => ArgvIntent::Version,
        "--json" | "--no-terminal" | "--no-telegram" | "--continue" | "-p" | "--print"
        | "--output-format" => ArgvIntent::CommandLike,
        other if !other.starts_with('-') && commands::lookup(other).is_some() => {
            ArgvIntent::CommandLike
        }
        _ => ArgvIntent::None,
    }
}

fn is_ignored_os_arg(arg: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        // Finder launches GUI apps with this synthetic process serial number arg.
        arg.starts_with("-psn_")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = arg;
        false
    }
}

/// On macOS, set NSApp's activation policy. No-op on other platforms.
///
/// `Accessory` removes the process from the Dock / Cmd-Tab; perfect for CLI
/// invocations that don't show a window. `Regular` restores the normal
/// foreground-app behavior (Dock icon + menu bar), used when bare `pengine`
/// turns out to be a GUI launch after all.
fn set_macos_activation_policy(app: &tauri::App, policy: tauri::ActivationPolicy) {
    #[cfg(target_os = "macos")]
    {
        if let Err(e) = app.handle().set_activation_policy(policy) {
            log::warn!("set_activation_policy failed: {e}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, policy);
    }
}

/// Second process spawned by `pengine app`; strip markers then continue into full Tauri setup.
fn consume_gui_spawn_env() -> bool {
    if std::env::var("PENGINE_OPEN_GUI")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        std::env::remove_var("PENGINE_OPEN_GUI");
        std::env::remove_var("PENGINE_LAUNCH_MODE");
        return true;
    }
    false
}

/// Start the desktop app in a **new** process so the current terminal can keep a REPL (or exit).
pub(super) fn spawn_gui_app_process() -> Result<(), String> {
    use std::process::{Command, Stdio};
    let exe = std::env::current_exe().map_err(|e| format!("current_exe: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.env("PENGINE_OPEN_GUI", "1");
    cmd.env_remove("PENGINE_LAUNCH_MODE");
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    cmd.spawn()
        .map_err(|e| format!("could not start GUI process: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_intent_none_for_empty_args() {
        let v: Vec<&str> = Vec::new();
        assert_eq!(argv_intent_from(v), ArgvIntent::None);
    }

    #[test]
    fn argv_intent_ignores_macos_psn_arg() {
        assert_eq!(argv_intent_from(vec!["-psn_0_12345"]), ArgvIntent::None);
    }

    #[test]
    fn argv_intent_detects_help_and_version() {
        assert_eq!(argv_intent_from(vec!["--help"]), ArgvIntent::Help);
        assert_eq!(argv_intent_from(vec!["version"]), ArgvIntent::Version);
    }

    #[test]
    fn argv_intent_detects_known_command() {
        assert_eq!(argv_intent_from(vec!["status"]), ArgvIntent::CommandLike);
        assert_eq!(argv_intent_from(vec!["app"]), ArgvIntent::CommandLike);
    }

    #[test]
    fn argv_intent_detects_global_cli_flags() {
        assert_eq!(argv_intent_from(vec!["--json"]), ArgvIntent::CommandLike);
    }

    #[test]
    fn argv_intent_none_for_shell_flag_alone() {
        assert_eq!(argv_intent_from(vec!["--shell"]), ArgvIntent::None);
    }
}
