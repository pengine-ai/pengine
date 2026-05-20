use super::commands::{self, NativeCommand};
use super::flavor;
use super::mentions;
use super::output::{fmt_elapsed, CliReply, Progress, ProgressStatus};
use super::session::{self, CliSession, HISTORY_TURN_BUDGET};
use crate::build_info;
use crate::infrastructure::audit_log;
use crate::infrastructure::bot_lifecycle;
use crate::modules::agent;
use crate::modules::bot::{repository as bot_repo, token_verify};
use crate::modules::mcp::service as mcp_service;
use crate::modules::ollama::service::{self as ollama, ModelInfo};
use crate::modules::secure_store;
use crate::modules::skills::service as skills_service;
use crate::modules::tool_engine::service::workspace_app_bind_pairs;
use crate::shared::state::{AppState, ConnectionData, ConnectionMetadata, LogEntry};
use crate::shared::user_settings;
use chrono::Utc;
use serde::Deserialize;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::mpsc;

pub fn help(topic: Option<&str>) -> CliReply {
    if let Some(t) = topic.map(str::trim).filter(|s| !s.is_empty()) {
        return help_for_topic(t);
    }
    let mut out = String::from(
        "Pengine CLI\n\nUsage:\n  pengine              interactive shell in a terminal (TTY only); never starts the GUI in that process\n  pengine app          open the desktop window in a **separate** process (can run together with a shell)\n  pengine <command>    one-shot command, then exit (e.g. status, ask, …)\n  pengine -p \"…\"       non-interactive: run the agent on the prompt and exit\n\nCommands:\n",
    );
    let width = commands::COMMANDS
        .iter()
        .map(|c: &NativeCommand| c.name.len())
        .max()
        .unwrap_or(0);
    for c in commands::COMMANDS {
        out.push_str(&format!(
            "  {:<width$}  {}\n",
            c.name,
            c.summary,
            width = width,
        ));
    }
    out.push_str(
        "\nGlobal flags (must appear BEFORE the subcommand):\n  \
         --json                       Emit JSON envelope (one per line), e.g. pengine --json status\n  \
         --shell                      With no subcommand: require a TTY for REPL; never open the GUI in-process (like `pengine-cli`)\n  \
         -p, --print <prompt>         Non-interactive: run agent on <prompt> and exit\n  \
         --output-format <fmt>        With -p: text (default), json, stream-json\n  \
         --continue                   Resume the most recent saved REPL session\n  \
         -V, --version                Print version and exit\n\n\
         Run `pengine help <command>` (or `/help <command>` in the REPL) for command-specific usage.",
    );
    CliReply::text(out.trim_end())
}

fn help_for_topic(topic: &str) -> CliReply {
    match commands::lookup(topic) {
        Some(cmd) => CliReply::code(
            "bash",
            format!(
                "{} — {}\n\n{}",
                cmd.name,
                cmd.summary,
                cmd.details.trim_end()
            ),
        ),
        None => CliReply::error(format!(
            "help: unknown command `{topic}` (try `/help` for the full list)"
        )),
    }
}

pub fn clear() -> CliReply {
    CliReply::error("clear: only available inside the interactive REPL")
}

pub fn version() -> CliReply {
    CliReply::text(format!(
        "pengine {} ({})",
        build_info::APP_VERSION,
        build_info::GIT_COMMIT,
    ))
}

pub async fn status(state: &AppState) -> CliReply {
    let bot_line = {
        let conn = state.connection.lock().await;
        match conn.as_ref() {
            Some(c) => format!("bot:       connected as @{}", c.bot_username),
            None => "bot:       not connected".to_string(),
        }
    };

    let active = ollama::active_model()
        .await
        .unwrap_or_else(|e| format!("<unreachable: {e}>"));
    let preferred = state
        .preferred_ollama_model
        .read()
        .await
        .clone()
        .unwrap_or_else(|| "<none>".to_string());

    let mcp_tools = state.mcp.read().await.tool_names().len();
    let skills_cap = *state.skills_hint_max_bytes.read().await;
    let session_line = {
        let snap = state.cli_session.read().await.clone();
        match snap {
            Some(s) => {
                let name_part = s
                    .name
                    .as_deref()
                    .map(|n| format!("name={n}  "))
                    .unwrap_or_default();
                format!(
                    "session:   {name_part}turns={}  tokens_in={}  tokens_out={}",
                    s.turns.len(),
                    s.prompt_tokens_total,
                    s.eval_tokens_total
                )
            }
            None => "session:   no active CLI session".to_string(),
        }
    };

    let body = format!(
        "{bot_line}\n\
         ollama:    active={active}  preferred={preferred}\n\
         mcp:       {mcp_tools} tool(s) connected\n\
         {session_line}\n\
         settings:  skills_hint_max_bytes={skills_cap}\n\
         store:     {}",
        state.store_path.display(),
    );
    CliReply::code("bash", body)
}

/// `config` with no args: dump settings. With `key=value`: set (clamped).
pub async fn config(state: &AppState, kvs: &[String]) -> CliReply {
    if kvs.is_empty() {
        let v = *state.skills_hint_max_bytes.read().await;
        return CliReply::code(
            "bash",
            format!(
                "skills_hint_max_bytes={v}  (min={}, max={}, default={})",
                user_settings::MIN_SKILLS_HINT_MAX_BYTES,
                user_settings::MAX_SKILLS_HINT_MAX_BYTES,
                user_settings::DEFAULT_SKILLS_HINT_MAX_BYTES,
            ),
        );
    }

    let mut applied: Vec<String> = Vec::new();
    for kv in kvs {
        let Some((key, value)) = kv.split_once('=') else {
            return CliReply::error(format!("invalid form `{kv}`; expected `key=value`"));
        };
        let key = key.trim();
        let value = value.trim();
        match key {
            "skills_hint_max_bytes" => match value.parse::<u32>() {
                Ok(n) => match user_settings::save_skills_hint_max_bytes(&state.store_path, n) {
                    Ok(clamped) => {
                        let mut w = state.skills_hint_max_bytes.write().await;
                        *w = clamped;
                        applied.push(format!("{key}={clamped}"));
                    }
                    Err(e) => {
                        return CliReply::error(format!("save failed: {e}"));
                    }
                },
                Err(_) => {
                    return CliReply::error(format!("{key}: expected u32, got `{value}`"));
                }
            },
            other => {
                return CliReply::error(format!(
                    "unknown setting `{other}`. Known: skills_hint_max_bytes",
                ));
            }
        }
    }
    CliReply::code("bash", format!("updated: {}", applied.join(", ")))
}

/// If `token` is all ASCII digits and parses to `1..=len`, returns a **0-based** index.
fn model_catalog_index_token(token: &str, len: usize) -> Option<usize> {
    if len == 0 || token.is_empty() || !token.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: usize = token.parse().ok()?;
    if n >= 1 && n <= len {
        Some(n - 1)
    } else {
        None
    }
}

fn format_model_catalog_list(catalog: &ollama::ModelCatalog, preferred: Option<&str>) -> String {
    let n = catalog.models.len();
    let pref_s = preferred.unwrap_or("<none>");
    let active_s = catalog.active.as_deref().unwrap_or("<none>");
    let mut out = format!("ollama models ({n}):  preferred={pref_s}  daemon_active={active_s}\n",);
    if n == 0 {
        out.push_str("(no models returned — is `ollama serve` running?)\n");
    } else {
        for (i, m) in catalog.models.iter().enumerate() {
            let mut tags: Vec<&'static str> = Vec::new();
            if catalog.active.as_deref() == Some(m.name.as_str()) {
                tags.push("active");
            }
            if preferred == Some(m.name.as_str()) {
                tags.push("preferred");
            }
            let tag = if tags.is_empty() {
                String::new()
            } else {
                format!("  [{}]", tags.join(", "))
            };
            out.push_str(&format!(
                "  {:>3}  {} ({}){tag}\n",
                i + 1,
                m.name,
                m.kind.as_str(),
            ));
        }
    }
    out.push_str("\nSet preferred: /model <name>  (same as `pengine model …`)\n");
    out.push_str("Set preferred + load in Ollama: /model <#>  (1-based row from this list)\n");
    out.push_str("Clear: /model --clear");
    out
}

async fn apply_preferred_model(state: &AppState, entry: &ModelInfo) -> CliReply {
    let name = entry.name.as_str();
    *state.preferred_ollama_model.write().await = Some(name.to_string());
    if entry.kind == ollama::ModelKind::Local {
        *state.last_local_model.write().await = Some(name.to_string());
    }
    state
        .emit_log("run", &format!("ollama model set to '{name}' (cli)"))
        .await;
    CliReply::code("bash", format!("preferred model set to {name}"))
}

/// `model` — list models (no args), set preferred by **name** or **1-based #** from the list, or `--clear`.
/// Selecting by **#** also asks Ollama to load that model so it becomes **daemon active** (`/api/ps`).
/// Mirrors the validation in `handle_ollama_model_put` in `http_server.rs`.
pub async fn model(state: &AppState, name: Option<&str>, clear: bool) -> CliReply {
    if clear {
        *state.preferred_ollama_model.write().await = None;
        return CliReply::code("bash", "preferred model cleared (uses active model)");
    }
    let catalog = match ollama::model_catalog(3000).await {
        Ok(c) => c,
        Err(e) => return CliReply::error(format!("ollama catalog: {e}")),
    };
    let preferred = state.preferred_ollama_model.read().await.clone();
    let preferred_ref = preferred.as_deref();

    let Some(name) = name.map(str::trim).filter(|s| !s.is_empty()) else {
        let body = format_model_catalog_list(&catalog, preferred_ref);
        return CliReply::code("bash", body);
    };

    let (entry, activate_in_ollama) =
        if let Some(idx) = model_catalog_index_token(name, catalog.models.len()) {
            (&catalog.models[idx], true)
        } else if let Some(e) = catalog.models.iter().find(|m| m.name == name) {
            (e, false)
        } else {
            return CliReply::error(format!("model `{name}` is not available in Ollama"));
        };

    if activate_in_ollama {
        if let Err(e) = ollama::touch_activate_model(entry.name.as_str()).await {
            return CliReply::error(format!(
                "ollama: could not load model `{}` as daemon active: {e}",
                entry.name
            ));
        }
    }

    let mut reply = apply_preferred_model(state, entry).await;
    if activate_in_ollama {
        reply
            .body
            .push_str("\nollama: model loaded (daemon active in /api/ps)");
    }
    reply
}

pub async fn bot_connect(state: &AppState, token: &str) -> CliReply {
    let token = token.trim();
    if token.is_empty() {
        return CliReply::error("bot connect: token is empty");
    }
    let me = match token_verify::verify_token(token).await {
        Ok(m) => m,
        Err(e) => return CliReply::error(format!("verify: {e}")),
    };
    bot_lifecycle::stop_and_wait_for_bot(state).await;
    let conn = ConnectionData {
        bot_token: token.to_string(),
        bot_id: me.id.to_string(),
        bot_username: me.username().to_string(),
        connected_at: Utc::now(),
    };
    if let Err(e) = secure_store::save_token(&conn.bot_id, &conn.bot_token) {
        return CliReply::error(format!("keychain save: {e}"));
    }
    let metadata = ConnectionMetadata::from(&conn);
    if let Err(e) = bot_repo::persist(&state.store_path, &metadata) {
        let _ = secure_store::delete_token(&conn.bot_id);
        return CliReply::error(format!("persist: {e}"));
    }
    *state.connection.lock().await = Some(conn);
    state
        .emit_log("ok", &format!("Bot @{} connected via CLI", me.username()))
        .await;
    CliReply::code(
        "bash",
        format!(
            "connected: @{}\ntoken saved (keychain + {})",
            me.username(),
            state.store_path.display(),
        ),
    )
}

pub async fn bot_disconnect(state: &AppState) -> CliReply {
    bot_lifecycle::stop_and_wait_for_bot(state).await;
    let bot_id = {
        let mut lock = state.connection.lock().await;
        let id = lock.as_ref().map(|c| c.bot_id.clone());
        *lock = None;
        id
    };
    if let Err(e) = bot_repo::clear(&state.store_path) {
        return CliReply::error(format!("clear store: {e}"));
    }
    if let Some(id) = bot_id {
        if let Err(e) = secure_store::delete_token(&id) {
            return CliReply::error(format!("keychain delete: {e}"));
        }
    }
    CliReply::code("bash", "disconnected and cleared store")
}

pub async fn tools(state: &AppState, search: Option<&str>) -> CliReply {
    let reg = state.mcp.read().await;
    let mut rows: Vec<(String, String, String)> = reg
        .all_tools()
        .into_iter()
        .map(|t| {
            (
                t.server_name.clone(),
                t.name.clone(),
                t.description.unwrap_or_default(),
            )
        })
        .collect();
    if let Some(q) = search {
        let q = q.to_lowercase();
        rows.retain(|(s, n, d)| {
            s.to_lowercase().contains(&q)
                || n.to_lowercase().contains(&q)
                || d.to_lowercase().contains(&q)
        });
    }
    if rows.is_empty() {
        return CliReply::code(
            "bash",
            "no tools (MCP not warmed or filter matched nothing)",
        );
    }
    rows.sort_by(|a, b| (a.0.as_str(), a.1.as_str()).cmp(&(b.0.as_str(), b.1.as_str())));
    let name_w = rows.iter().map(|(_, n, _)| n.len()).max().unwrap_or(0);
    let server_w = rows.iter().map(|(s, _, _)| s.len()).max().unwrap_or(0);
    let mut out = String::new();
    for (server, name, desc) in rows {
        let snippet = desc.lines().next().unwrap_or("");
        out.push_str(&format!(
            "{:<server_w$}  {:<name_w$}  {}\n",
            server,
            name,
            snippet,
            server_w = server_w,
            name_w = name_w,
        ));
    }
    CliReply::code("bash", out.trim_end())
}

/// `skills` — list, enable, disable.
pub async fn skills(state: &AppState, action: Option<&str>, slug: Option<&str>) -> CliReply {
    let action = action.map(str::trim).unwrap_or("list");
    match action {
        "list" | "" => {
            let rows = skills_service::list_skills(&state.store_path);
            if rows.is_empty() {
                return CliReply::code("bash", "no skills");
            }
            let slug_w = rows.iter().map(|s| s.slug.len()).max().unwrap_or(0);
            let mut out = String::new();
            for sk in rows {
                let flag = if sk.enabled { "on" } else { "off" };
                out.push_str(&format!(
                    "[{flag:>3}] {:<slug_w$}  {}\n",
                    sk.slug,
                    sk.description,
                    slug_w = slug_w,
                ));
            }
            CliReply::code("bash", out.trim_end())
        }
        "enable" | "disable" => {
            let Some(slug) = slug.map(str::trim).filter(|s| !s.is_empty()) else {
                return CliReply::error(format!("skills {action}: slug required"));
            };
            let enable = action == "enable";
            if let Err(e) = skills_service::set_skill_enabled(&state.store_path, slug, enable) {
                return CliReply::error(format!("skills {action}: {e}"));
            }
            CliReply::code(
                "bash",
                format!(
                    "skill `{slug}` {}",
                    if enable { "enabled" } else { "disabled" }
                ),
            )
        }
        other => CliReply::error(format!(
            "skills: unknown action `{other}` (use list | enable | disable)"
        )),
    }
}

/// `fs` — show / mutate MCP filesystem roots. Mutations rewrite
/// `mcp.json` directly; docker-runtime tool sync is a dashboard concern.
pub async fn fs(state: &AppState, action: Option<&str>, path: Option<&str>) -> CliReply {
    let action = action.map(str::trim).unwrap_or("list");
    let _guard = state.mcp_config_mutex.lock().await;
    match action {
        "list" | "" => {
            let cfg = match mcp_service::load_or_init_config(&state.mcp_config_path) {
                Ok(c) => c,
                Err(e) => return CliReply::error(format!("fs: {e}")),
            };
            let paths = mcp_service::filesystem_allowed_paths(&cfg);
            if paths.is_empty() {
                CliReply::code("bash", "(no roots)")
            } else {
                CliReply::code("bash", paths.join("\n"))
            }
        }
        "add" | "remove" => {
            let Some(path) = path.map(str::trim).filter(|p| !p.is_empty()) else {
                return CliReply::error(format!("fs {action}: path required"));
            };
            let mut cfg = match mcp_service::load_or_init_config(&state.mcp_config_path) {
                Ok(c) => c,
                Err(e) => return CliReply::error(format!("fs: {e}")),
            };
            let mut paths = mcp_service::filesystem_allowed_paths(&cfg);
            let before = paths.len();
            if action == "add" {
                if !paths.iter().any(|p| p == path) {
                    paths.push(path.to_string());
                }
            } else {
                paths.retain(|p| p != path);
            }
            if paths.len() == before {
                return CliReply::code(
                    "bash",
                    format!("no change ({action} `{path}` had no effect)"),
                );
            }
            mcp_service::set_filesystem_allowed_paths(&mut cfg, &paths);
            if let Err(e) = mcp_service::save_config(&state.mcp_config_path, &cfg) {
                return CliReply::error(format!("save: {e}"));
            }
            CliReply::code("bash", format!("{action}: {path}"))
        }
        other => CliReply::error(format!(
            "fs: unknown action `{other}` (use list | add | remove)"
        )),
    }
}

pub async fn logs(state: &AppState, tail: Option<usize>, follow: bool) -> CliReply {
    if follow {
        return follow_logs_from_broadcast(state).await;
    }
    let n = tail.unwrap_or(50);
    if n == 0 {
        return CliReply::error("logs --tail: N must be ≥ 1");
    }
    tail_logs_from_audit(state, n).await
}

async fn follow_logs_from_broadcast(state: &AppState) -> CliReply {
    let mut rx = match state.log_tx.lock().await.as_ref() {
        Some(tx) => tx.subscribe(),
        None => return CliReply::error("logs: broadcast channel is closed"),
    };
    loop {
        match rx.recv().await {
            Ok(ev) => println!("{}", format_log_line(&ev)),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                eprintln!("[logs lagged: {skipped} event(s) dropped]");
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
    CliReply::code("bash", "log stream closed")
}

async fn tail_logs_from_audit(state: &AppState, n: usize) -> CliReply {
    let files = match audit_log::list_audit_files(&state.store_path).await {
        Ok(f) => f,
        Err(e) => return CliReply::error(format!("logs: list audit files: {e}")),
    };
    // `list_audit_files` sorts newest-date first. Accumulate lines (oldest first
    // of the ones we keep) by walking days backwards; stop once we hit `n`.
    let mut out: Vec<String> = Vec::with_capacity(n);
    for entry in files {
        let content = match audit_log::read_audit_file(&state.store_path, &entry.date).await {
            Ok(s) => s,
            Err(e) => {
                log::warn!("logs: read audit-{}: {e}", entry.date);
                continue;
            }
        };
        let mut day_lines: Vec<String> = content
            .lines()
            .filter_map(format_audit_ndjson_line)
            .collect();
        // Combine `day_lines` (older) with `out` (newer already gathered).
        day_lines.append(&mut out);
        // Keep tail `n` entries.
        let drop = day_lines.len().saturating_sub(n);
        out = day_lines.split_off(drop);
        if out.len() >= n {
            break;
        }
    }
    if out.is_empty() {
        return CliReply::code("bash", "(no audit history)");
    }
    CliReply::log(out.join("\n"))
}

fn format_audit_ndjson_line(raw: &str) -> Option<String> {
    let line = raw.trim();
    if line.is_empty() {
        return None;
    }
    #[derive(Deserialize)]
    struct AuditJson {
        timestamp: String,
        kind: String,
        message: String,
    }
    let j: AuditJson = serde_json::from_str(line).ok()?;
    Some(format!("{} [{}] {}", j.timestamp, j.kind, j.message))
}

fn format_log_line(ev: &LogEntry) -> String {
    format!("{} [{}] {}", ev.timestamp, ev.kind, ev.message)
}

pub async fn ask(state: &AppState, text: &str) -> CliReply {
    ask_in_session(state, text, true).await
}

// ── Streaming reply filter ────────────────────────────────────────────────────

enum StreamFilterState {
    Buffering,
    Streaming,
    Done,
}

/// Extracts the content of `<pengine_reply>…</pengine_reply>` from an
/// incremental token stream. `feed()` returns text to print as it arrives;
/// `finish()` flushes any trailing buffer when the channel closes.
struct ReplyStreamFilter {
    state: StreamFilterState,
    buf: String,
}

impl ReplyStreamFilter {
    fn new() -> Self {
        Self {
            state: StreamFilterState::Buffering,
            buf: String::new(),
        }
    }

    fn feed(&mut self, chunk: &str) -> Option<String> {
        match self.state {
            StreamFilterState::Done => None,
            StreamFilterState::Buffering => self.feed_buffering(chunk),
            StreamFilterState::Streaming => self.feed_streaming(chunk),
        }
    }

    fn feed_buffering(&mut self, chunk: &str) -> Option<String> {
        const OPEN: &str = "<pengine_reply>";
        self.buf.push_str(chunk);
        if let Some(pos) = self.buf.find(OPEN) {
            let rest = self.buf[pos + OPEN.len()..].to_string();
            self.buf.clear();
            self.state = StreamFilterState::Streaming;
            if rest.is_empty() {
                return None;
            }
            return self.feed_streaming(&rest);
        }
        // Keep only the last (OPEN.len()-1) chars to detect a tag split across chunks.
        let keep = OPEN.len() - 1;
        if self.buf.len() > keep {
            self.buf = self.buf[self.buf.len() - keep..].to_string();
        }
        None
    }

    fn feed_streaming(&mut self, chunk: &str) -> Option<String> {
        const CLOSE: &str = "</pengine_reply>";
        self.buf.push_str(chunk);
        if let Some(pos) = self.buf.find(CLOSE) {
            self.state = StreamFilterState::Done;
            let content = self.buf[..pos].to_string();
            self.buf.clear();
            return if content.is_empty() {
                None
            } else {
                Some(content)
            };
        }
        // Safe to emit everything except the last (CLOSE.len()-1) bytes,
        // which might be the start of a split closing tag.
        let safe = self.buf.len().saturating_sub(CLOSE.len() - 1);
        if safe == 0 {
            return None;
        }
        let out = self.buf[..safe].to_string();
        self.buf = self.buf[safe..].to_string();
        Some(out)
    }

    fn finish(&mut self) -> Option<String> {
        if matches!(self.state, StreamFilterState::Streaming) && !self.buf.is_empty() {
            self.state = StreamFilterState::Done;
            Some(std::mem::take(&mut self.buf))
        } else {
            None
        }
    }
}

/// Consume text chunks from the agent's streaming channel, filter for the
/// `<pengine_reply>` block, and render live to stdout with REPL chrome.
/// Sets `did_stream` to `true` when at least one content chunk was printed.
async fn run_stream_consumer(
    mut rx: mpsc::UnboundedReceiver<String>,
    status: ProgressStatus,
    did_stream: Arc<AtomicBool>,
) {
    let mut filter = ReplyStreamFilter::new();
    let mut started = false;
    let mut at_line_start = true;

    while let Some(chunk) = rx.recv().await {
        let Some(content) = filter.feed(&chunk) else {
            continue;
        };
        if content.is_empty() {
            continue;
        }
        if !started {
            started = true;
            // Stop the spinner and wait one tick so it clears its stderr line.
            status.stop_spinner().await;
            tokio::time::sleep(std::time::Duration::from_millis(95)).await;
            super::output::repl_reply_section_open();
            // Print first-line prefix (no trailing newline so content follows immediately).
            let prefix = if std::io::stdout().is_terminal() {
                super::output::REPL_FIRST_PREFIX
            } else {
                super::output::REPL_FIRST_PREFIX_PLAIN
            };
            print!("{prefix}");
            let _ = std::io::stdout().flush();
            at_line_start = false;
        }
        write_stream_chunk(&content, &mut at_line_start);
    }

    // Flush any buffered tail (e.g. content right before `</pengine_reply>`).
    if let Some(tail) = filter.finish() {
        if !tail.is_empty() {
            if !started {
                started = true;
                status.stop_spinner().await;
                tokio::time::sleep(std::time::Duration::from_millis(95)).await;
                super::output::repl_reply_section_open();
                let prefix = if std::io::stdout().is_terminal() {
                    super::output::REPL_FIRST_PREFIX
                } else {
                    super::output::REPL_FIRST_PREFIX_PLAIN
                };
                print!("{prefix}");
                let _ = std::io::stdout().flush();
                at_line_start = false;
            }
            write_stream_chunk(&tail, &mut at_line_start);
        }
    }

    if started {
        if !at_line_start {
            println!();
        }
        super::output::repl_reply_section_close();
        did_stream.store(true, Ordering::Relaxed);
    }
}

/// Print `content` to stdout, inserting continuation prefixes after each newline.
fn write_stream_chunk(content: &str, at_line_start: &mut bool) {
    let mut out = String::with_capacity(content.len() + 16);
    for ch in content.chars() {
        if *at_line_start && ch != '\n' {
            out.push_str(super::output::REPL_CONT_PREFIX);
            *at_line_start = false;
        }
        out.push(ch);
        if ch == '\n' {
            *at_line_start = true;
        }
    }
    print!("{out}");
    let _ = std::io::stdout().flush();
}

/// `/retry` — re-run the most recent user message with a fresh agent turn.
pub async fn retry(state: &AppState) -> CliReply {
    let last_msg = {
        let guard = state.cli_session.read().await;
        guard
            .as_ref()
            .and_then(|s| s.turns.last().map(|t| t.user.clone()))
    };
    match last_msg {
        None => CliReply::error("retry: no turns in the current session — nothing to retry"),
        Some(msg) => ask_in_session(state, &msg, true).await,
    }
}

/// `/search` — case-insensitive substring search across the active session's turns.
pub async fn search(state: &AppState, query: &str) -> CliReply {
    let query = query.trim();
    if query.is_empty() {
        return CliReply::error("search: query is empty");
    }
    let q_lower = query.to_lowercase();
    let turns = {
        let guard = state.cli_session.read().await;
        guard.as_ref().map(|s| s.turns.clone()).unwrap_or_default()
    };
    if turns.is_empty() {
        return CliReply::text("search: session is empty — no turns to search".to_string());
    }
    let mut results: Vec<String> = Vec::new();
    for (i, turn) in turns.iter().enumerate() {
        let num = i + 1;
        if turn.user.to_lowercase().contains(&q_lower) {
            results.push(format!(
                "Turn {num} (you):    {}",
                search_snippet(&turn.user, &q_lower, 120)
            ));
        }
        if turn.assistant.to_lowercase().contains(&q_lower) {
            results.push(format!(
                "Turn {num} (agent):  {}",
                search_snippet(&turn.assistant, &q_lower, 120)
            ));
        }
    }
    if results.is_empty() {
        return CliReply::text(format!("search: no matches for `{query}`"));
    }
    let count = results.len();
    CliReply::code(
        "bash",
        format!("{count} match(es) for `{query}`\n\n{}", results.join("\n")),
    )
}

/// Extract up to `ctx_chars` characters centred on the first match of `needle` in `text`.
fn search_snippet(text: &str, needle: &str, ctx_chars: usize) -> String {
    let lower = text.to_lowercase();
    let pos = lower.find(needle).unwrap_or_default();
    let start = pos.saturating_sub(ctx_chars / 3);
    let end = (pos + needle.len() + ctx_chars * 2 / 3).min(text.len());
    // Safety: find() returns valid UTF-8 byte boundary in `lower`; the same
    // offset is valid in `text` because both strings have identical byte layout.
    let end = text
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| i >= end)
        .unwrap_or(text.len());
    let start = text
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| i >= start)
        .unwrap_or(0);
    let mut out = text[start..end].replace('\n', " ");
    if start > 0 {
        out.insert(0, '…');
    }
    if end < text.len() {
        out.push('…');
    }
    out
}

/// `/session` — named session management: list, new, switch, rename.
pub async fn session_cmd(state: &AppState, action: &str, rest: &str) -> CliReply {
    match action.trim() {
        "" | "list" => session_list(state).await,
        "help" => session_help(),
        "new" => session_new(state, rest.trim()).await,
        "switch" => session_switch(state, rest.trim()).await,
        "rename" => session_rename(state, rest.trim()).await,
        "delete" => session_delete(state, rest.trim()).await,
        "prune" => session_prune(state).await,
        other => CliReply::error(format!(
            "session: unknown action `{other}` — try /session help"
        )),
    }
}

fn session_help() -> CliReply {
    CliReply::code(
        "bash",
        "\
/session list                    list all saved sessions (newest first)
/session new <name>              create a named session and switch to it immediately
/session switch <name-or-id>     resume a saved session; saves current first
/session rename <name>           rename the active session
/session delete <name-or-id>     delete a saved session from disk (not the active one)
/session prune                   delete all unnamed sessions (keeps named + active)
/session help                    show this help

Notes:
  - Sessions persist on disk across restarts.
  - Auto-compaction runs in the background when a session exceeds 12 turns."
            .trim_end(),
    )
}

async fn session_list(state: &AppState) -> CliReply {
    let manifest = session::load_manifest(&state.store_path);
    if manifest.entries.is_empty() {
        return CliReply::text("no saved sessions");
    }
    let active_id = state
        .cli_session
        .read()
        .await
        .as_ref()
        .map(|s| s.id.clone());
    let mut out = format!(
        "{:<3}  {:<15}  {:<22}  {:<5}  {:<10}  {}\n{}\n",
        " ",
        "id",
        "name",
        "turns",
        "in",
        "branch",
        "─".repeat(84),
    );
    for e in manifest.entries.iter().rev() {
        let marker = if active_id.as_deref() == Some(e.id.as_str()) {
            "▶"
        } else {
            " "
        };
        let name = e.name.as_deref().unwrap_or("—");
        let branch = e.git_branch.as_deref().unwrap_or("—");
        let tokens_in = fmt_num(e.prompt_tokens_total);
        out.push_str(&format!(
            "{marker:<3}  {:<15}  {name:<22}  {:<5}  {tokens_in:<10}  {branch}\n",
            e.id, e.turn_count,
        ));
        if let Some(snippet) = &e.summary_snippet {
            out.push_str(&format!("      {snippet}\n"));
        }
    }
    CliReply::code("bash", out.trim_end())
}

async fn session_new(state: &AppState, name: &str) -> CliReply {
    // Save current session before replacing it.
    {
        let snap = state.cli_session.read().await.clone();
        if let Some(s) = snap {
            if let Err(e) = session::save(&state.store_path, &s) {
                state.emit_log("cli", &format!("session save: {e}")).await;
            }
        }
    }
    spawn_compaction_if_needed(state).await;

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project = session::detect_project_context(&cwd);
    let mut new_sess = CliSession::fresh_with_project(project);
    if !name.is_empty() {
        new_sess.name = Some(name.to_string());
    }
    let label = new_sess.name.clone().unwrap_or_else(|| new_sess.id.clone());
    *state.cli_session.write().await = Some(new_sess.clone());
    // Persist immediately so `pengine ask --continue` and `pengine compact` can
    // find this session from subsequent one-shot subprocesses.
    if let Err(e) = session::save(&state.store_path, &new_sess) {
        state.emit_log("cli", &format!("session save (new): {e}")).await;
    }
    CliReply::text(format!("started new session: {label}"))
}

async fn session_switch(state: &AppState, query: &str) -> CliReply {
    if query.is_empty() {
        return CliReply::error("session switch: name or id required (see /session list)");
    }
    let target = match session::load_by_name_or_id(&state.store_path, query) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return CliReply::error(format!(
                "session switch: no session matching `{query}` (see /session list)"
            ))
        }
        Err(e) => return CliReply::error(format!("session switch: {e}")),
    };
    // Don't switch to the already-active session.
    let active_id = state
        .cli_session
        .read()
        .await
        .as_ref()
        .map(|s| s.id.clone());
    if active_id.as_deref() == Some(target.id.as_str()) {
        return CliReply::text(format!(
            "session `{}` is already active",
            target.name.as_deref().unwrap_or(&target.id)
        ));
    }
    // Save + compact current session before switching.
    spawn_compaction_if_needed(state).await;
    {
        let snap = state.cli_session.read().await.clone();
        if let Some(s) = snap {
            if let Err(e) = session::save(&state.store_path, &s) {
                state.emit_log("cli", &format!("session save: {e}")).await;
            }
        }
    }
    let turn_count = target.turns.len();
    let label = target.name.clone().unwrap_or_else(|| target.id.clone());
    let summary_line = target
        .summary
        .as_deref()
        .map(|s| {
            let snippet: String = s.trim().chars().take(100).collect();
            format!("\n  summary: {snippet}")
        })
        .unwrap_or_default();
    *state.cli_session.write().await = Some(target);
    CliReply::text(format!(
        "switched to session: {label}  (turns={turn_count}){summary_line}"
    ))
}

async fn session_delete(state: &AppState, query: &str) -> CliReply {
    if query.is_empty() {
        return CliReply::error("session delete: name or id required (see /session list)");
    }
    let manifest = session::load_manifest(&state.store_path);
    let q_lower = query.to_ascii_lowercase();
    let entry = manifest
        .entries
        .iter()
        .rev()
        .find(|e| {
            e.name
                .as_deref()
                .map(|n| n.eq_ignore_ascii_case(query))
                .unwrap_or(false)
                || e.id == query
                || e.id.to_ascii_lowercase().starts_with(&q_lower)
        })
        .cloned();
    let Some(entry) = entry else {
        return CliReply::error(format!(
            "session delete: no session matching `{query}` (see /session list)"
        ));
    };
    // Refuse to delete the currently active session.
    let active_id = state
        .cli_session
        .read()
        .await
        .as_ref()
        .map(|s| s.id.clone());
    if active_id.as_deref() == Some(entry.id.as_str()) {
        return CliReply::error(
            "session delete: cannot delete the active session — switch away first",
        );
    }
    let label = entry
        .name
        .as_deref()
        .map(|n| n.to_string())
        .unwrap_or_else(|| entry.id.clone());
    if let Err(e) = session::delete(&state.store_path, &entry.id) {
        return CliReply::error(format!("session delete: {e}"));
    }
    CliReply::text(format!("deleted session: {label}"))
}

async fn session_prune(state: &AppState) -> CliReply {
    let manifest = session::load_manifest(&state.store_path);
    let active_id = state
        .cli_session
        .read()
        .await
        .as_ref()
        .map(|s| s.id.clone());

    let to_delete: Vec<_> = manifest
        .entries
        .iter()
        .filter(|e| {
            e.name.is_none() && active_id.as_deref() != Some(e.id.as_str())
        })
        .map(|e| e.id.clone())
        .collect();

    if to_delete.is_empty() {
        return CliReply::text("no unnamed sessions to prune");
    }

    let n = to_delete.len();
    let mut errors = Vec::new();
    for id in &to_delete {
        if let Err(e) = session::delete(&state.store_path, id) {
            errors.push(format!("{id}: {e}"));
        }
    }

    if errors.is_empty() {
        CliReply::text(format!(
            "pruned {n} unnamed session{}",
            if n == 1 { "" } else { "s" }
        ))
    } else {
        CliReply::error(format!(
            "pruned {}/{n} sessions; errors: {}",
            n - errors.len(),
            errors.join(", ")
        ))
    }
}

async fn session_rename(state: &AppState, name: &str) -> CliReply {
    if name.is_empty() {
        return CliReply::error("session rename: new name required");
    }
    let mut guard = state.cli_session.write().await;
    let Some(sess) = guard.as_mut() else {
        return CliReply::error("session rename: no active session");
    };
    sess.name = Some(name.to_string());
    let snapshot = sess.clone();
    drop(guard);
    if let Err(e) = session::save(&state.store_path, &snapshot) {
        return CliReply::error(format!("session rename: save failed: {e}"));
    }
    CliReply::text(format!("session renamed to: {name}"))
}

/// `/compact` — call the AI to summarize old turns, store as `session.summary`,
/// and keep only the last `HISTORY_TURN_BUDGET` turns verbatim.
pub async fn compact_session(state: &AppState) -> CliReply {
    let (to_compact, prior_summary, total_tok, drop_tok) = {
        let guard = state.cli_session.read().await;
        let Some(sess) = guard.as_ref() else {
            return CliReply::error("no active session — start a conversation first");
        };
        if sess.turns.is_empty() {
            return CliReply::text("session is empty — nothing to compact");
        }
        let keep = HISTORY_TURN_BUDGET.min(sess.turns.len());
        let drop_upto = sess.turns.len().saturating_sub(keep);
        if drop_upto == 0 && sess.summary.is_none() {
            return CliReply::text(format!(
                "session has {} turn(s) — within the {HISTORY_TURN_BUDGET}-turn keep budget; nothing to compact",
                sess.turns.len()
            ));
        }
        let total_tok = sess
            .prompt_tokens_total
            .saturating_add(sess.eval_tokens_total);
        let drop_tok: u64 = sess.turns[..drop_upto]
            .iter()
            .map(|t| t.prompt_tokens.saturating_add(t.eval_tokens))
            .sum();
        // Compact ALL turns so a re-compact also merges the prior summary.
        (sess.turns.clone(), sess.summary.clone(), total_tok, drop_tok)
    };

    let n_turns = to_compact.len();
    let keep_n = HISTORY_TURN_BUDGET.min(n_turns);
    let drop_n = n_turns.saturating_sub(keep_n);

    // Print a Claude-Code-style compaction header to the terminal.
    compact_print_header(n_turns, keep_n, total_tok, drop_tok);

    let prompt = session::compact_prompt(prior_summary.as_deref(), &to_compact);
    let progress = Progress::start(format!("Summarizing {n_turns} turn(s)…"));
    let result = agent::run_system_turn(state, &prompt, None).await;
    let elapsed = progress.finish().await;

    match result {
        Ok(turn) => {
            let mut guard = state.cli_session.write().await;
            if let Some(sess) = guard.as_mut() {
                session::apply_compaction(sess, turn.text, keep_n);
                let snapshot = sess.clone();
                drop(guard);
                if let Err(e) = session::save(&state.store_path, &snapshot) {
                    state.emit_log("cli", &format!("compact save: {e}")).await;
                }
            }
            compact_print_result(drop_n, keep_n, drop_tok, elapsed);
            let freed_line = if drop_tok > 0 {
                format!("\n  freed ≈{} tokens", fmt_num(drop_tok))
            } else {
                String::new()
            };
            CliReply::text(format!(
                "Compacted {n_turns} turn(s) → summary + {keep_n} recent turn(s) kept verbatim.{freed_line}"
            ))
        }
        Err(e) => {
            emit_baked_line(elapsed);
            CliReply::error(format!("compact: {e}"))
        }
    }
}

/// Print a visual compaction header to stderr (terminal only).
fn compact_print_header(n_turns: usize, keep_n: usize, total_tok: u64, drop_tok: u64) {
    if !std::io::stderr().is_terminal() {
        return;
    }
    const BAR_W: usize = 32;
    let filled = if total_tok > 0 {
        (drop_tok as f64 / total_tok as f64 * BAR_W as f64).round() as usize
    } else {
        BAR_W / 2
    };
    let filled = filled.min(BAR_W);
    let bar: String = format!(
        "{}{}",
        "█".repeat(filled),
        "░".repeat(BAR_W - filled)
    );
    let pct = if total_tok > 0 {
        format!(
            " ({:.0}% freed)",
            drop_tok as f64 / total_tok as f64 * 100.0
        )
    } else {
        String::new()
    };
    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "\n  \x1b[1mContext compaction\x1b[0m  {n_turns} turn(s) → {keep_n} verbatim + summary"
    );
    let _ = writeln!(err, "  [{bar}]{pct}");
}

/// Print a compact completion line to stderr (terminal only).
fn compact_print_result(
    drop_n: usize,
    keep_n: usize,
    drop_tok: u64,
    elapsed: std::time::Duration,
) {
    if !std::io::stderr().is_terminal() {
        return;
    }
    let freed = if drop_tok > 0 {
        format!("freed ≈{} tokens", fmt_num(drop_tok))
    } else {
        format!("{drop_n} turn(s) compacted")
    };
    let time_s = format!("{:.1}s", elapsed.as_secs_f64());
    let mut err = std::io::stderr().lock();
    let _ = writeln!(
        err,
        "  \x1b[32m✓\x1b[0m \x1b[2m{freed} · kept {keep_n} verbatim · {time_s}\x1b[0m"
    );
}

/// Trigger background auto-compaction when the session grows beyond the threshold.
/// Runs silently after a turn completes; the compacted session is ready for the next turn.
pub(super) async fn spawn_compaction_if_needed(state: &AppState) {
    let needs = {
        let g = state.cli_session.read().await;
        g.as_ref()
            .map(|s| s.turns.len() > session::COMPACT_THRESHOLD)
            .unwrap_or(false)
    };
    if !needs {
        return;
    }
    let state = state.clone();
    tauri::async_runtime::spawn(async move {
        compact_session_background(&state).await;
    });
}

async fn compact_session_background(state: &AppState) {
    let (to_compact, prior_summary, keep) = {
        let g = state.cli_session.read().await;
        let Some(sess) = g.as_ref() else { return };
        if sess.turns.len() <= session::COMPACT_THRESHOLD {
            return;
        }
        let keep = HISTORY_TURN_BUDGET;
        let drop_upto = sess.turns.len().saturating_sub(keep);
        (sess.turns[..drop_upto].to_vec(), sess.summary.clone(), keep)
    };

    if to_compact.is_empty() {
        return;
    }

    let prompt = session::compact_prompt(prior_summary.as_deref(), &to_compact);
    match agent::run_system_turn(state, &prompt, None).await {
        Ok(result) => {
            let mut g = state.cli_session.write().await;
            if let Some(sess) = g.as_mut() {
                if sess.turns.len() > HISTORY_TURN_BUDGET {
                    session::apply_compaction(sess, result.text, keep);
                    let snapshot = sess.clone();
                    drop(g);
                    if let Err(e) = session::save(&state.store_path, &snapshot) {
                        state
                            .emit_log("cli", &format!("auto-compact save: {e}"))
                            .await;
                    } else {
                        state
                            .emit_log("cli", "session auto-compacted in background")
                            .await;
                    }
                }
            }
        }
        Err(e) => {
            state
                .emit_log("cli", &format!("auto-compact failed: {e}"))
                .await;
        }
    }
}

/// `ask` variant that lets callers (one-shot CLI vs REPL vs Telegram) decide
/// whether to extend the persistent session.
pub async fn ask_in_session(state: &AppState, text: &str, persist_session: bool) -> CliReply {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return CliReply::error("ask: prompt is empty");
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if persist_session {
        let mut guard = state.cli_session.write().await;
        if guard.is_none() {
            *guard = Some(CliSession::fresh_with_project(
                session::detect_project_context(&cwd),
            ));
        }
    }
    let allowed_roots: Vec<PathBuf> = state
        .cached_filesystem_paths
        .read()
        .await
        .iter()
        .map(PathBuf::from)
        .collect();
    let expanded = mentions::expand_mentions(trimmed, &cwd, &allowed_roots);
    for err in &expanded.errors {
        state.emit_log("cli", &format!("mention: {err}")).await;
    }

    let (context_prefix, project_for_dot) = if persist_session {
        let snap = state.cli_session.read().await.clone();
        let pfx = snap
            .as_ref()
            .map(|s| s.context_prefix())
            .unwrap_or_default();
        let proj = snap
            .as_ref()
            .and_then(|s| s.project.clone())
            .unwrap_or_else(|| session::detect_project_context(&cwd));
        (pfx, proj)
    } else {
        (String::new(), session::detect_project_context(&cwd))
    };

    let mcp_prefix_for_dot: Option<String> = {
        let fs_paths = state.cached_filesystem_paths.read().await.clone();
        let pairs = workspace_app_bind_pairs(&fs_paths);
        let root = project_for_dot
            .git_root
            .as_deref()
            .unwrap_or(&project_for_dot.cwd);
        pairs.into_iter().find_map(|(host, container)| {
            let hp = std::path::Path::new(host.trim());
            let hcanon = std::fs::canonicalize(hp).unwrap_or_else(|_| hp.to_path_buf());
            if root.starts_with(&hcanon) {
                Some(container)
            } else {
                None
            }
        })
    };
    let dot_prefix =
        session::dot_pengine_prompt_block(&project_for_dot, mcp_prefix_for_dot.as_deref());

    let prompt_for_agent = {
        let mut head = String::new();
        if !dot_prefix.is_empty() {
            head.push_str(&dot_prefix);
        }
        if !context_prefix.is_empty() {
            head.push_str(&context_prefix);
        }
        if head.is_empty() {
            expanded.message.clone()
        } else {
            format!("{head}## New user message\n{}", expanded.message)
        }
    };

    // Snapshot unstaged diff before the turn so we can show only the model's new changes.
    let diff_before: String = project_for_dot
        .git_root
        .as_deref()
        .and_then(|r| {
            std::process::Command::new("git")
                .args(["diff"])
                .current_dir(r)
                .output()
                .ok()
        })
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();

    let progress = Progress::start(flavor::thinking_label().to_string());
    let token_counter = progress.token_counter();
    let forwarder = spawn_status_forwarder(state, progress.status_sender()).await;

    // On TTY: wire a live-streaming channel so content appears as it's generated.
    let did_stream = Arc::new(AtomicBool::new(false));
    let (maybe_text_tx, stream_task) = if std::io::stdout().is_terminal() {
        let (text_tx, text_rx) = mpsc::unbounded_channel::<String>();
        let stream_status = progress.status_sender();
        let did_stream2 = did_stream.clone();
        let task = tokio::spawn(run_stream_consumer(text_rx, stream_status, did_stream2));
        (Some(text_tx), Some(task))
    } else {
        (None, None)
    };

    let result =
        agent::run_turn(state, &prompt_for_agent, Some(token_counter), maybe_text_tx).await;
    if let Some(h) = forwarder {
        h.abort();
    }
    // Wait for the streaming consumer to finish (it exits when the channel closes).
    if let Some(t) = stream_task {
        let _ = t.await;
    }
    let elapsed = progress.finish().await;
    let streamed = did_stream.load(Ordering::Relaxed);

    match result {
        Ok(turn) if turn.suppress_telegram_reply => {
            emit_baked_line(elapsed);
            CliReply::text("(no reply)")
        }
        Ok(turn) => {
            // When the model ran tool calls but produced no final text (e.g. the
            // <pengine_reply> block was empty or missing), fall through with a
            // placeholder so the auto-diff block still gets appended. Only bail
            // early when there were genuinely no tool calls either (steps ≤ 1).
            if turn.text.trim().is_empty() && turn.steps <= 1 && !streamed {
                emit_baked_line(elapsed);
                return CliReply::text("(no reply)");
            }
            if persist_session {
                let mut guard = state.cli_session.write().await;
                let session = guard.get_or_insert_with(|| {
                    CliSession::fresh_with_project(session::detect_project_context(&cwd))
                });
                if session.project.is_none() {
                    session.project = Some(session::detect_project_context(&cwd));
                }
                session.record_turn(
                    &expanded.message,
                    &turn.text,
                    turn.prompt_tokens,
                    turn.eval_tokens,
                    &turn.model,
                );
                let snapshot = session.clone();
                drop(guard);
                if let Err(e) = session::save(&state.store_path, &snapshot) {
                    state.emit_log("cli", &format!("session save: {e}")).await;
                }
                spawn_compaction_if_needed(state).await;
            }
            emit_turn_footer(elapsed, &turn);
            emit_skill_nudge(turn.steps);
            // Body was already rendered live — return empty so render_reply is a no-op.
            if streamed {
                return CliReply::text(String::new());
            }
            let mut body = if turn.text.trim().is_empty() {
                String::from("Done.")
            } else {
                turn.text
            };
            if !expanded.errors.is_empty() {
                body.push_str("\n\n_Note: ");
                body.push_str(&expanded.errors.join("; "));
                body.push('_');
            }
            // Auto-attach only the diff the model introduced this turn.
            if !body.contains("```diff") {
                if let Some(git_root) = project_for_dot.git_root.as_deref() {
                    let diff_after: String = std::process::Command::new("git")
                        .args(["diff"])
                        .current_dir(git_root)
                        .output()
                        .ok()
                        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
                        .unwrap_or_default();

                    let delta = diff_new_sections(&diff_before, &diff_after);
                    if !delta.trim().is_empty() {
                        const FILE_DIFF_CAP: usize = 8_000;
                        for section in diff_file_sections(&delta) {
                            let (label, content) = section;
                            body.push_str("\n\n`");
                            body.push_str(&label);
                            body.push('`');
                            let capped = if content.len() > FILE_DIFF_CAP {
                                format!(
                                    "{}\n…(truncated at {} chars)",
                                    &content[..FILE_DIFF_CAP],
                                    FILE_DIFF_CAP
                                )
                            } else {
                                content
                            };
                            body.push_str("\n```diff\n");
                            body.push_str(&capped);
                            body.push_str("\n```");
                        }
                    }
                }
            }
            CliReply::text(body)
        }
        Err(e) => {
            emit_baked_line(elapsed);
            CliReply::error(format!("agent error: {e}"))
        }
    }
}

/// Subscribe to the broadcast log channel; forward summarized events to the
/// spinner status. No-op when the channel is already closed.
async fn spawn_status_forwarder(
    state: &AppState,
    status: ProgressStatus,
) -> Option<tokio::task::JoinHandle<()>> {
    let mut rx = state
        .log_tx
        .lock()
        .await
        .as_ref()
        .map(|tx| tx.subscribe())?;
    Some(tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if ev.kind == "tool" {
                        if let Some(block) = inline_tool_block(&ev.message) {
                            status.interject(block).await;
                        }
                    }
                    if let Some(s) = summarize_log_for_status(&ev) {
                        status.set(s).await;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    }))
}

/// Plain-language label for an MCP tool id (snake_case), for CLI spinner / interjects.
fn friendly_tool_action(name: &str) -> String {
    match name {
        "directory_tree" => flavor::fun_pair("Scanning folder layout", "Mapping the file jungle"),
        "list_directory" | "list_directory_with_sizes" => "Listing folder contents".into(),
        "search_files" => "Searching files".into(),
        "read_text_file" | "read_multiple_files" | "read_media_file" => "Reading files".into(),
        "write_file" | "edit_file" | "create_directory" | "move_file" => "Updating files".into(),
        "get_file_info" => "Reading file info".into(),
        "fetch" => "Fetching web content".into(),
        "brave_web_search" => "Searching the web".into(),
        "git_status" => "Checking git status".into(),
        "git_branch" => "Listing git branches".into(),
        "git_diff" | "git_diff_unstaged" => "Showing git changes".into(),
        "git_log" => "Reading git history".into(),
        "git_commit" => "Creating git commit".into(),
        "time" => "Getting time".into(),
        "roll_dice" => "Rolling dice".into(),
        "shell_execute" => flavor::fun_pair("Running a shell command", "Poking the subprocess"),
        "run_terminal_cmd" => flavor::fun_pair("Running a terminal command", "Borrowing the shell"),
        _ => name
            .split('_')
            .filter(|s| !s.is_empty())
            .map(|w| {
                let mut it = w.chars();
                match it.next() {
                    None => String::new(),
                    Some(c) => c
                        .to_uppercase()
                        .chain(it.flat_map(|x| x.to_lowercase()))
                        .collect(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

/// Short line for the live `Thinking · …` spinner (no `tool:` prefix, no raw snake_case ids).
fn humanize_tool_status_line(message: &str) -> Option<String> {
    const MAX: usize = 72;
    let msg = message.trim();
    if msg.is_empty() {
        return None;
    }

    if let Some(prefix) = msg.strip_suffix(" tool call(s)") {
        if let Ok(n) = prefix.trim().parse::<usize>() {
            let s = flavor::tool_batch_label(n);
            return Some(truncate_chars(&s, MAX));
        }
    }

    if let Some(rest) = msg.strip_prefix('[') {
        if let Some((step, tail)) = rest.split_once(']') {
            if step == "host" {
                if msg.contains("auto-fetch") {
                    return Some(truncate_chars("Loading a linked page…", MAX));
                }
                return Some(truncate_chars(msg, MAX));
            }
            let name = tail.trim();
            let action = friendly_tool_action(name);
            let line = format!("{action}…");
            return Some(truncate_chars(&line, MAX));
        }
    }

    if let Some((head, tail)) = msg.split_once(": ") {
        if tail.ends_with(" bytes") {
            let action = friendly_tool_action(head.trim());
            return Some(truncate_chars(&format!("{action} · done"), MAX));
        }
    }

    if let Some((head, err)) = msg.split_once(" error: ") {
        let action = friendly_tool_action(head.trim());
        let err_short = truncate_chars(err.trim(), 48);
        return Some(truncate_chars(
            &format!("{action} · issue: {err_short}"),
            MAX,
        ));
    }

    Some(truncate_chars(msg, MAX))
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    format!(
        "{}…",
        s.chars()
            .take(max_chars.saturating_sub(1))
            .collect::<String>()
    )
}

/// Render a `"tool"` log message as a persistent REPL block (matches the
/// reply `  ⎿  ` prefix style). Returns `None` for noise we don't echo.
///
/// Shapes from `modules/agent`:
/// - `[N] name`                 → friendly action (no raw tool ids)
/// - `name: <n> bytes`          → friendly “done” line
/// - `name error: <...>`       → friendly error line
/// - `[host] auto-fetch <url>`  → pass-through / shortened
fn inline_tool_block(message: &str) -> Option<String> {
    let msg = message.trim();
    if msg.is_empty() || msg.ends_with("does not support tools") {
        return None;
    }
    let rendered = if let Some(rest) = msg.strip_prefix('[') {
        if let Some((step, tail)) = rest.split_once(']') {
            let name = tail.trim();
            if step.starts_with("host") {
                msg.to_string()
            } else {
                format!("{}…", friendly_tool_action(name))
            }
        } else {
            msg.to_string()
        }
    } else if msg.contains(" error: ") {
        humanize_tool_status_line(msg).unwrap_or_else(|| msg.to_string())
    } else if msg.contains(": ") && msg.ends_with(" bytes") {
        humanize_tool_status_line(msg).unwrap_or_else(|| msg.to_string())
    } else {
        msg.to_string()
    };
    // Visible prefix is "  ⎿  · " = 7 chars; cap so the full line ≤ 78 cols.
    const MAX: usize = 70;
    let clipped: String = rendered.chars().take(MAX).collect();
    let suffix = if rendered.chars().count() > MAX {
        "…"
    } else {
        ""
    };
    if std::io::stderr().is_terminal() {
        Some(format!(
            "  \x1b[2m⎿\x1b[0m  \x1b[2m·\x1b[0m {clipped}{suffix}"
        ))
    } else {
        Some(format!("  ⎿  · {clipped}{suffix}"))
    }
}

/// One-line compaction of a log event for the live spinner suffix.
/// Returns `None` for log kinds that would just echo ourselves.
fn summarize_log_for_status(ev: &LogEntry) -> Option<String> {
    match ev.kind.as_str() {
        // Self-echo + final reply — user is already about to see it.
        "cli" | "reply" | "msg" | "auth" | "ok" => None,
        // Internal debug / routing info — not useful as spinner status.
        "tool_ctx" | "run" | "memory" | "mcp" => None,
        "tool" => humanize_tool_status_line(&ev.message),
        // Suppress unknown kinds to avoid raw debug strings in the spinner.
        _ => None,
    }
}

/// After 5+ agent steps, suggest capturing the workflow as a reusable skill.
/// Prints a dim hint to stderr — consistent with the footer, invisible to piped consumers.
fn emit_skill_nudge(steps: u32) {
    const THRESHOLD: u32 = 5;
    if steps < THRESHOLD || !std::io::stderr().is_terminal() {
        return;
    }
    // Rotate tip text based on step count so repeated heavy tasks vary slightly.
    let tip = match steps % 3 {
        0 => "This multi-step workflow could become a skill — ask the agent to `create_skill` with a recipe.",
        1 => "Tip: tell the agent to save this pattern as a skill so it finds it faster next time.",
        _ => "That took effort — ask the agent to use `create_skill` to encode the approach for future turns.",
    };
    let line = format!("  \x1b[2m⎿\x1b[0m  \x1b[2m{tip}\x1b[0m\n");
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(line.as_bytes());
    let _ = err.flush();
}

/// `  ⎿  Baked for 4.8s — quip` on stderr — used when no token data is available
/// (errors, suppressed replies, compaction).
fn emit_baked_line(elapsed: std::time::Duration) {
    if !std::io::stderr().is_terminal() {
        return;
    }
    let line = format!(
        "  \x1b[2m⎿\x1b[0m  \x1b[2m{}\x1b[0m\n",
        flavor::baked_message(elapsed, fmt_elapsed)
    );
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(line.as_bytes());
    let _ = err.flush();
}

/// Full turn footer with elapsed time, token counts, model, and think flag.
/// Shown on stderr after every successful agent turn so the user has
/// per-turn data for optimisation decisions.
///
/// Example:
/// ```text
///   ⎿  Baked for 4.8s — chef's kiss · in:1,234 out:567 · qwen3:1.5b · think:on
/// ```
fn emit_turn_footer(elapsed: std::time::Duration, turn: &agent::TurnResult) {
    if !std::io::stderr().is_terminal() {
        return;
    }
    let baked = flavor::baked_message(elapsed, fmt_elapsed);

    // Always show token counts. Use "?" when Ollama didn't return the field
    // (e.g. model doesn't report counts, or prompt was fully KV-cached giving in:0).
    let in_s = if turn.prompt_tokens > 0 {
        fmt_num(turn.prompt_tokens)
    } else {
        "?".to_string()
    };
    let out_s = if turn.eval_tokens > 0 {
        fmt_num(turn.eval_tokens)
    } else {
        "?".to_string()
    };
    let tokens = format!(" · in:{in_s} out:{out_s}");

    let model = if !turn.model.is_empty() {
        // Trim off `:latest` suffix — it adds noise without signal.
        let m = turn.model.trim_end_matches(":latest");
        format!(" · {m}")
    } else {
        String::new()
    };

    let think = if turn.think_enabled {
        " · think:on"
    } else {
        ""
    };

    let steps = if turn.steps > 1 {
        format!(" · {}steps", turn.steps)
    } else {
        String::new()
    };

    let line = format!("  \x1b[2m⎿\x1b[0m  \x1b[2m{baked}{tokens}{model}{think}{steps}\x1b[0m\n");
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(line.as_bytes());
    let _ = err.flush();
}

/// Format a token count with thousands separators: 1234 → "1,234".
fn fmt_num(n: u64) -> String {
    if n < 1_000 {
        return n.to_string();
    }
    if n < 10_000 {
        return format!("{:.1}k", n as f64 / 1_000.0);
    }
    format!("{}k", n / 1_000)
}

/// Split a unified diff into `(display_label, diff_content)` per file.
/// Label is the `b/<path>` portion from the `diff --git` header, stripped to a relative path.
fn diff_file_sections(diff: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current_label = String::new();
    let mut current_buf = String::new();

    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            if !current_label.is_empty() {
                out.push((current_label.clone(), std::mem::take(&mut current_buf)));
            }
            // "a/<path> b/<path>" → take b/ side as the display label
            current_label = rest
                .split_once(" b/")
                .map(|(_, b)| b.to_string())
                .unwrap_or_else(|| rest.to_string());
            current_buf.clear();
        } else {
            current_buf.push_str(line);
            current_buf.push('\n');
        }
    }
    if !current_label.is_empty() {
        out.push((current_label, current_buf));
    }
    out
}

/// Return the sections of `after` that are absent from `before` (new or modified files).
/// Each section starts with a `diff --git …` line.
fn diff_new_sections(before: &str, after: &str) -> String {
    if after == before {
        return String::new();
    }
    fn split_sections(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut start = 0;
        let bytes = s.as_bytes();
        let marker = b"\ndiff --git ";
        let mut i = 0;
        while i + marker.len() <= bytes.len() {
            if bytes[i..].starts_with(marker) {
                if i > start {
                    out.push(&s[start..i]);
                }
                start = i + 1; // skip the leading newline
                i += marker.len();
            } else {
                i += 1;
            }
        }
        if start < s.len() {
            out.push(&s[start..]);
        }
        out
    }

    let before_sections: std::collections::HashSet<&str> = split_sections(before)
        .into_iter()
        .map(str::trim_end)
        .collect();
    let mut out = String::new();
    for section in split_sections(after) {
        if !before_sections.contains(section.trim_end()) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(section);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_num_uses_k_suffix() {
        assert_eq!(fmt_num(0), "0");
        assert_eq!(fmt_num(512), "512");
        assert_eq!(fmt_num(999), "999");
        assert_eq!(fmt_num(1_000), "1.0k");
        assert_eq!(fmt_num(1_711), "1.7k");
        assert_eq!(fmt_num(9_012), "9.0k");
        assert_eq!(fmt_num(10_000), "10k");
        assert_eq!(fmt_num(13_362), "13k");
        assert_eq!(fmt_num(100_000), "100k");
    }

    #[test]
    fn inline_tool_block_rewrites_step_call() {
        let out = inline_tool_block("[0] fetch").unwrap();
        assert!(out.contains("Fetching web content"), "got: {out}");
    }

    #[test]
    fn inline_tool_block_passes_host_auto_fetch() {
        let out = inline_tool_block("[host] auto-fetch https://example.com").unwrap();
        assert!(
            out.contains("[host] auto-fetch https://example.com"),
            "got: {out}"
        );
    }

    #[test]
    fn diff_new_sections_returns_only_added_file() {
        let before =
            "diff --git a/old.rs b/old.rs\n--- a/old.rs\n+++ b/old.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let after = format!(
            "{before}diff --git a/new.rs b/new.rs\n--- a/new.rs\n+++ b/new.rs\n@@ -0,0 +1 @@\n+added\n"
        );
        let delta = diff_new_sections(before, &after);
        assert!(
            delta.contains("new.rs"),
            "expected new.rs in delta: {delta}"
        );
        assert!(
            !delta.contains("old.rs"),
            "old.rs should not appear: {delta}"
        );
    }

    #[test]
    fn diff_new_sections_empty_when_unchanged() {
        let diff = "diff --git a/x.rs b/x.rs\n+something\n";
        assert!(diff_new_sections(diff, diff).is_empty());
    }

    #[test]
    fn inline_tool_block_passes_result_line() {
        let out = inline_tool_block("fetch: 4012 bytes").unwrap();
        assert!(out.contains("Fetching web content · done"), "got: {out}");
    }

    #[test]
    fn inline_tool_block_passes_error_line() {
        let out = inline_tool_block("fetch error: 503 Service Unavailable").unwrap();
        assert!(out.contains("Fetching web content · issue:"), "got: {out}");
        assert!(out.contains("503"), "got: {out}");
    }

    #[test]
    fn inline_tool_block_drops_unsupported_marker() {
        assert!(inline_tool_block("qwen3:0.5b does not support tools").is_none());
    }

    #[test]
    fn format_audit_line_accepts_valid_ndjson() {
        let raw =
            r#"{"timestamp":"2026-04-23T12:34:56.789Z","kind":"cli","message":"pengine status"}"#;
        let out = format_audit_ndjson_line(raw).unwrap();
        assert!(out.contains("[cli]"));
        assert!(out.contains("pengine status"));
    }

    #[test]
    fn format_audit_line_skips_garbage() {
        assert!(format_audit_ndjson_line("not-json").is_none());
        assert!(format_audit_ndjson_line("").is_none());
    }

    #[test]
    fn model_catalog_index_token_parses_one_based() {
        assert_eq!(super::model_catalog_index_token("1", 3), Some(0));
        assert_eq!(super::model_catalog_index_token("3", 3), Some(2));
        assert_eq!(super::model_catalog_index_token("0", 3), None);
        assert_eq!(super::model_catalog_index_token("4", 3), None);
        assert_eq!(super::model_catalog_index_token("02", 3), Some(1));
    }

    #[test]
    fn model_catalog_index_token_rejects_non_digits() {
        assert_eq!(super::model_catalog_index_token("llama3", 3), None);
        assert_eq!(super::model_catalog_index_token("1a", 3), None);
    }
}
