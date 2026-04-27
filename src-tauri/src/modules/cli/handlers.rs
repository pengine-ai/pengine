//! PR 1 handlers — one function per native command.
//!
//! Rules:
//! - Each handler returns a [`CliReply`]; sinks render it.
//! - Handlers reuse existing module services (bot, mcp, ollama, skills,
//!   user_settings). No duplicated business logic.

use super::commands::{self, NativeCommand};
use super::doctor;
use super::mentions;
use super::output::{fmt_elapsed, CliReply, Progress, ProgressStatus};
use super::session::{self, CliSession};
use crate::build_info;
use crate::infrastructure::audit_log;
use crate::infrastructure::bot_lifecycle;
use crate::modules::agent;
use crate::modules::bot::{repository as bot_repo, token_verify};
use crate::modules::mcp::service as mcp_service;
use crate::modules::ollama::service::{self as ollama, ChatOptions, ModelInfo, ModelKind};
use crate::modules::secure_store;
use crate::modules::skills::service as skills_service;
use crate::shared::state::{AppState, ConnectionData, ConnectionMetadata, LogEntry};
use crate::shared::user_settings;
use chrono::Utc;
use serde::Deserialize;
use serde_json::json;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

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
         -V, --version                Print version and exit\n  \
         --no-terminal                Reserved for future sink routing\n  \
         --no-telegram                Reserved for future sink routing\n\n\
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

/// `/clear` outside a REPL is a no-op error. The REPL itself intercepts the
/// command before dispatch, so this handler only fires from the Telegram
/// bridge or one-shot execution.
pub fn clear() -> CliReply {
    CliReply::error("clear: only available inside the interactive REPL")
}

pub async fn doctor(state: &AppState) -> CliReply {
    let checks = doctor::run(state).await;
    let any_fail = checks
        .iter()
        .any(|c| matches!(c.status, doctor::Status::Fail));
    let body = doctor::format_report(&checks);
    if any_fail {
        CliReply::error(format!("pengine doctor — issues found:\n\n{body}"))
    } else {
        CliReply::code("bash", format!("pengine doctor — all good\n\n{body}"))
    }
}

/// `/plan [on|off|toggle]` — toggles plan mode on the AppState.
pub async fn plan(state: &AppState, action: Option<&str>) -> CliReply {
    let action = action.map(str::trim).unwrap_or("toggle");
    let mut guard = state.plan_mode.write().await;
    let new_value = match action {
        "on" | "enable" | "true" | "1" => true,
        "off" | "disable" | "false" | "0" => false,
        "toggle" | "" => !*guard,
        other => {
            return CliReply::error(format!(
                "plan: unknown action `{other}` (use on | off | toggle)"
            ))
        }
    };
    *guard = new_value;
    if new_value {
        CliReply::code(
            "bash",
            "plan mode: ON\n  · agent will produce a markdown plan\n  · write tools (memory writes, fs writes, edits) are stripped from the catalog",
        )
    } else {
        CliReply::code("bash", "plan mode: OFF")
    }
}

/// `/cost` — show token usage + rough cost estimate for the current session.
pub async fn cost(state: &AppState) -> CliReply {
    let session = state.cli_session.read().await.clone();
    let Some(s) = session else {
        return CliReply::code(
            "bash",
            "no active session — token totals available after the first /ask",
        );
    };
    let model = state
        .preferred_ollama_model
        .read()
        .await
        .clone()
        .unwrap_or_else(|| "<unset>".to_string());
    let kind = ollama::classify_model(&model);
    let cost_line = match kind {
        ModelKind::Local => "  est_cost:  $0.00 (local model)".to_string(),
        ModelKind::Cloud => {
            // Conservative blended estimate: $1 / 1M prompt + $3 / 1M completion.
            // Pengine doesn't have per-model pricing; this is an upper-bound hint.
            let in_cost = (s.prompt_tokens_total as f64) * 1.0e-6;
            let out_cost = (s.eval_tokens_total as f64) * 3.0e-6;
            format!(
                "  est_cost:  ~${:.4} (cloud, rough $1/$3 per M in/out)",
                in_cost + out_cost
            )
        }
    };
    let body = format!(
        "session:    {}\n  turns:      {}\n  tokens_in:  {}\n  tokens_out: {}\n  model:      {}\n{}",
        s.id,
        s.turns.len(),
        s.prompt_tokens_total,
        s.eval_tokens_total,
        model,
        cost_line
    );
    CliReply::code("bash", body)
}

/// `/resume` — load the most recent saved session into AppState.
pub async fn resume(state: &AppState) -> CliReply {
    match session::load_last(&state.store_path) {
        Ok(Some(s)) => {
            let summary_line = if s.summary.is_some() {
                "  summary:    present (set by /compact)\n"
            } else {
                ""
            };
            let body = format!(
                "resumed session: {}\n  started:    {}\n  turns:      {}\n{}  tokens_in:  {}\n  tokens_out: {}",
                s.id, s.started_at, s.turns.len(), summary_line, s.prompt_tokens_total, s.eval_tokens_total
            );
            *state.cli_session.write().await = Some(s);
            CliReply::code("bash", body)
        }
        Ok(None) => CliReply::code("bash", "no saved session to resume"),
        Err(e) => CliReply::error(format!("resume: {e}")),
    }
}

/// `/compact` — summarize the current session and reset turns. The summary is
/// kept on the session and prefixed to future user messages.
pub async fn compact(state: &AppState) -> CliReply {
    let snapshot = state.cli_session.read().await.clone();
    let Some(mut s) = snapshot else {
        return CliReply::code("bash", "no active session to compact");
    };
    if s.turns.is_empty() && s.summary.is_none() {
        return CliReply::code("bash", "session has no turns yet — nothing to compact");
    }

    let mut transcript = String::new();
    if let Some(prev) = s.summary.as_deref() {
        transcript.push_str("Prior summary:\n");
        transcript.push_str(prev);
        transcript.push_str("\n\n");
    }
    for t in &s.turns {
        transcript.push_str(&format!(
            "[user] {}\n[assistant] {}\n",
            t.user.trim(),
            t.assistant.trim()
        ));
    }

    let mut model = state
        .preferred_ollama_model
        .read()
        .await
        .clone()
        .unwrap_or_else(String::new);
    if model.is_empty() {
        model = match ollama::active_model().await {
            Ok(m) => m,
            Err(e) => return CliReply::error(format!("compact: ollama: {e}")),
        };
    }

    let messages = json!([
        {
            "role": "system",
            "content": "You compress a chat transcript. Output a tight markdown summary covering: (1) topics, (2) decisions, (3) outstanding tasks. Max 250 words. No chain-of-thought."
        },
        {
            "role": "user",
            "content": format!("Compress this transcript:\n\n{transcript}")
        }
    ]);
    let opts = ChatOptions {
        think: Some(false),
        num_predict: Some(512),
        temperature: Some(0.3),
        ..ChatOptions::default()
    };
    let result = match ollama::chat_with_tools(&model, &messages, &json!([]), &opts).await {
        Ok(r) => r,
        Err(e) => return CliReply::error(format!("compact: model: {e}")),
    };
    let summary_text = result
        .message
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if summary_text.is_empty() {
        return CliReply::error("compact: model returned empty summary");
    }

    let prior_turn_count = s.turns.len();
    let summary_chars = summary_text.chars().count();
    s.summary = Some(summary_text);
    s.turns.clear();
    if let Err(e) = session::save(&state.store_path, &s) {
        return CliReply::error(format!("compact: save: {e}"));
    }
    let id = s.id.clone();
    *state.cli_session.write().await = Some(s);

    CliReply::code(
        "bash",
        format!(
            "compacted: {prior_turn_count} turn(s) → summary ({summary_chars} chars), session `{id}` saved"
        ),
    )
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

    let body = format!(
        "{bot_line}\n\
         ollama:    active={active}  preferred={preferred}\n\
         mcp:       {mcp_tools} tool(s) connected\n\
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

/// `bot connect <token>` — verify, persist, save keychain. Does NOT spawn the
/// bot (the CLI one-shot process would exit). The running desktop app or a
/// REPL session picks up the stored metadata + keychain token.
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

/// `tools [search]` — list MCP tools from the live registry.
/// The registry is assumed warmed by the caller (bootstrap / REPL startup).
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

/// `logs` — three modes:
/// - `--follow` (live) subscribes to the in-memory broadcast and prints each event.
/// - `--tail N` reads the newest N lines from the audit NDJSON files on disk,
///   walking back day-by-day until N is reached or no older file remains.
/// - default (no flag) tails the last 50 lines — the common "what just happened" case.
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

/// `ask` variant that lets callers (one-shot CLI vs REPL vs Telegram) decide
/// whether to extend the persistent session.
pub async fn ask_in_session(state: &AppState, text: &str, persist_session: bool) -> CliReply {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return CliReply::error("ask: prompt is empty");
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
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

    let context_prefix = if persist_session {
        let snap = state.cli_session.read().await.clone();
        snap.map(|s| s.context_prefix()).unwrap_or_default()
    } else {
        String::new()
    };

    let prompt_for_agent = if context_prefix.is_empty() {
        expanded.message.clone()
    } else {
        format!("{context_prefix}## New user message\n{}", expanded.message)
    };

    let progress = Progress::start("Thinking");
    let forwarder = spawn_status_forwarder(state, progress.status_sender()).await;
    let result = agent::run_turn(state, &prompt_for_agent).await;
    if let Some(h) = forwarder {
        h.abort();
    }
    let elapsed = progress.finish().await;
    emit_baked_line(elapsed);

    match result {
        Ok(turn) if turn.suppress_telegram_reply => CliReply::text("(no reply)"),
        Ok(turn) => {
            if turn.text.trim().is_empty() {
                return CliReply::text("(no reply)");
            }
            if persist_session {
                let mut guard = state.cli_session.write().await;
                let session = guard.get_or_insert_with(CliSession::fresh);
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
            }
            let mut body = turn.text;
            if !expanded.errors.is_empty() {
                body.push_str("\n\n_Note: ");
                body.push_str(&expanded.errors.join("; "));
                body.push('_');
            }
            CliReply::text(body)
        }
        Err(e) => CliReply::error(format!("agent error: {e}")),
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

/// Render a `"tool"` log message as a persistent REPL block (matches the
/// reply `  ⎿  ` prefix style). Returns `None` for noise we don't echo.
///
/// Shapes from `modules/agent`:
/// - `[N] name`                 → `called name (step N)`
/// - `name: <n> bytes`          → pass-through (e.g. `fetch: 4012 bytes`)
/// - `name error: <...>`        → pass-through
/// - `[host] auto-fetch <url>`  → pass-through
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
                format!("called {name} (step {step})")
            }
        } else {
            msg.to_string()
        }
    } else {
        msg.to_string()
    };
    const MAX: usize = 100;
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
        // Self-echo + the final reply — the user is already about to see it.
        "cli" | "reply" | "msg" | "auth" | "ok" => None,
        _ => {
            const MAX: usize = 60;
            let msg = ev.message.trim();
            let msg: String = msg.chars().take(MAX).collect();
            let ellipsed = if msg.chars().count() == MAX {
                format!("{msg}…")
            } else {
                msg
            };
            Some(format!("{}: {}", ev.kind, ellipsed))
        }
    }
}

/// `  ⎿  Baked for 4.8s` on stderr once the spinner has been cleared.
/// Only emitted when stderr is a TTY, matching the spinner gate.
fn emit_baked_line(elapsed: std::time::Duration) {
    if !std::io::stderr().is_terminal() {
        return;
    }
    let line = format!(
        "  \x1b[2m⎿\x1b[0m  \x1b[2mBaked for {}\x1b[0m\n",
        fmt_elapsed(elapsed)
    );
    let mut err = std::io::stderr().lock();
    let _ = err.write_all(line.as_bytes());
    let _ = err.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_tool_block_rewrites_step_call() {
        let out = inline_tool_block("[0] fetch").unwrap();
        assert!(out.contains("called fetch (step 0)"), "got: {out}");
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
    fn inline_tool_block_passes_result_line() {
        let out = inline_tool_block("fetch: 4012 bytes").unwrap();
        assert!(out.ends_with("fetch: 4012 bytes"), "got: {out}");
    }

    #[test]
    fn inline_tool_block_passes_error_line() {
        let out = inline_tool_block("fetch error: 503 Service Unavailable").unwrap();
        assert!(out.contains("error: 503"), "got: {out}");
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
