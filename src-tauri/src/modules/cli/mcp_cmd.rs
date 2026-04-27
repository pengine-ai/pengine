//! `pengine mcp` CLI — list/add/remove/import MCP servers from the terminal.
//!
//! Three install paths:
//! - **Docker image** (`add --image <ref>`): registers a `CustomToolEntry` and
//!   pulls the image via the existing Tool Engine flow (podman/docker). The
//!   image is run as a stdio MCP server inside the container.
//! - **HTTP** (`add --url <url> [--header K=V]`): adds an [`ServerEntry::Http`]
//!   for remote streamable-HTTP servers (Claude Code's `"type": "http"` shape).
//! - **stdio** (`add --command <cmd> [--arg <a>]…`): plain child-process MCP
//!   server, no container. Use this for Node `npx` servers when you don't want
//!   the Docker wrap.
//!
//! `import <path>` reads a Claude Code-style `mcp.json` (`mcpServers: {…}`)
//! and merges its servers into pengine's global `mcp.json`.

use super::output::CliReply;
use crate::modules::mcp::service as mcp_service;
use crate::modules::mcp::types::{CustomToolEntry, ServerEntry};
use crate::modules::tool_engine::runtime::detect_runtime;
use crate::modules::tool_engine::service as tool_engine;
use crate::shared::state::AppState;
use std::collections::HashMap;
use std::path::Path;

/// Args parsed from the CLI surface; identical regardless of whether the call
/// came from `pengine mcp add …`, `/mcp add …` in REPL, or the Telegram bridge.
#[derive(Debug, Default)]
pub struct AddArgs {
    pub name: String,
    pub url: Option<String>,
    pub headers: Vec<(String, String)>,
    pub image: Option<String>,
    pub mcp_server_cmd: Vec<String>,
    pub mount_workspace: bool,
    pub mount_read_only: bool,
    pub append_workspace_roots: bool,
    pub command: Option<String>,
    pub stdio_args: Vec<String>,
    pub stdio_env: Vec<(String, String)>,
    pub direct_return: bool,
}

pub async fn list(state: &AppState) -> CliReply {
    let cfg = match mcp_service::load_or_init_config(&state.mcp_config_path) {
        Ok(c) => c,
        Err(e) => return CliReply::error(format!("mcp list: {e}")),
    };
    if cfg.servers.is_empty() {
        return CliReply::code("bash", "(no MCP servers configured)");
    }
    let name_w = cfg.servers.keys().map(String::len).max().unwrap_or(0);
    let mut out = String::new();
    for (name, entry) in &cfg.servers {
        let (kind, detail) = describe_entry(entry);
        out.push_str(&format!(
            "  {kind:<6}  {name:<name_w$}  {detail}\n",
            name = name,
            kind = kind,
            detail = detail,
            name_w = name_w,
        ));
    }
    CliReply::code("bash", out.trim_end())
}

pub async fn add(state: &AppState, args: AddArgs) -> CliReply {
    if args.name.trim().is_empty() {
        return CliReply::error("mcp add: name is required");
    }

    let installed: Vec<&'static str> = [
        args.url.as_ref().map(|_| "url"),
        args.image.as_ref().map(|_| "image"),
        args.command.as_ref().map(|_| "command"),
    ]
    .into_iter()
    .flatten()
    .collect();
    if installed.len() != 1 {
        return CliReply::error(
            "mcp add: pick exactly one of --url, --image, or --command (and --arg/--header/...)",
        );
    }

    if let Some(url) = args.url.clone() {
        return add_http(state, &args.name, url, args.headers, args.direct_return).await;
    }
    if let Some(image) = args.image.clone() {
        return add_docker(state, &args.name, image, &args).await;
    }
    if let Some(command) = args.command.clone() {
        return add_stdio(state, &args.name, command, &args).await;
    }
    CliReply::error("mcp add: nothing to install")
}

pub async fn remove(state: &AppState, name: &str) -> CliReply {
    let name = name.trim();
    if name.is_empty() {
        return CliReply::error("mcp remove: name is required");
    }
    let _guard = state.mcp_config_mutex.lock().await;
    let mut cfg = match mcp_service::load_or_init_config(&state.mcp_config_path) {
        Ok(c) => c,
        Err(e) => return CliReply::error(format!("mcp remove: {e}")),
    };

    // Custom Docker tools live under both `custom_tools[]` and `servers[te_custom_<key>]`.
    let custom_idx = cfg.custom_tools.iter().position(|t| t.key == name);
    let direct_key_present = cfg.servers.contains_key(name);
    let custom_server_key = format!("te_custom_{name}");
    let custom_present = cfg.servers.contains_key(&custom_server_key);

    if !direct_key_present && custom_idx.is_none() && !custom_present {
        return CliReply::error(format!("mcp remove: `{name}` not found"));
    }

    if let Some(i) = custom_idx {
        cfg.custom_tools.remove(i);
    }
    cfg.servers.remove(name);
    cfg.servers.remove(&custom_server_key);

    if let Err(e) = mcp_service::save_config(&state.mcp_config_path, &cfg) {
        return CliReply::error(format!("mcp remove: save: {e}"));
    }
    CliReply::code("bash", format!("removed `{name}` (mcp.json updated)"))
}

pub async fn import(state: &AppState, path: &str) -> CliReply {
    let path = Path::new(path.trim());
    if !path.exists() {
        return CliReply::error(format!("mcp import: file not found: {}", path.display()));
    }
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) => return CliReply::error(format!("mcp import: read: {e}")),
    };
    let value: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(e) => return CliReply::error(format!("mcp import: parse: {e}")),
    };
    let new_servers = match mcp_service::parse_claude_mcp_servers(&value) {
        Ok(s) => s,
        Err(e) => return CliReply::error(format!("mcp import: {e}")),
    };

    let _guard = state.mcp_config_mutex.lock().await;
    let mut cfg = match mcp_service::load_or_init_config(&state.mcp_config_path) {
        Ok(c) => c,
        Err(e) => return CliReply::error(format!("mcp import: load: {e}")),
    };

    let mut added: Vec<String> = Vec::new();
    let mut overwritten: Vec<String> = Vec::new();
    for (name, entry) in new_servers {
        if cfg.servers.contains_key(&name) {
            overwritten.push(name.clone());
        } else {
            added.push(name.clone());
        }
        cfg.servers.insert(name, entry);
    }

    if added.is_empty() && overwritten.is_empty() {
        return CliReply::code(
            "bash",
            "import: nothing to add (file contained zero servers)",
        );
    }

    if let Err(e) = mcp_service::save_config(&state.mcp_config_path, &cfg) {
        return CliReply::error(format!("mcp import: save: {e}"));
    }

    let mut body = String::new();
    if !added.is_empty() {
        body.push_str(&format!("added: {}\n", added.join(", ")));
    }
    if !overwritten.is_empty() {
        body.push_str(&format!("overwritten: {}\n", overwritten.join(", ")));
    }
    body.push_str("\nRun `pengine status` after MCP warmup to see new tools.");
    CliReply::code("bash", body.trim().to_string())
}

async fn add_http(
    state: &AppState,
    name: &str,
    url: String,
    headers: Vec<(String, String)>,
    direct_return: bool,
) -> CliReply {
    let header_map: HashMap<String, String> = headers.into_iter().collect();
    let entry = ServerEntry::Http {
        url: url.clone(),
        headers: header_map,
        direct_return,
    };
    if let Err(e) = upsert_and_save(state, name.to_string(), entry).await {
        return CliReply::error(format!("mcp add: {e}"));
    }
    CliReply::code(
        "bash",
        format!("added http server `{name}` → {url}\n  Run `/tools` (or restart the REPL) to refresh the live registry."),
    )
}

async fn add_stdio(state: &AppState, name: &str, command: String, args: &AddArgs) -> CliReply {
    let entry = ServerEntry::Stdio {
        command: command.clone(),
        args: args.stdio_args.clone(),
        env: args.stdio_env.clone().into_iter().collect(),
        direct_return: args.direct_return,
        private_host_path: None,
        catalog_passthrough_keys: Vec::new(),
    };
    if let Err(e) = upsert_and_save(state, name.to_string(), entry).await {
        return CliReply::error(format!("mcp add: {e}"));
    }
    let argv = std::iter::once(command.as_str())
        .chain(args.stdio_args.iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join(" ");
    CliReply::code(
        "bash",
        format!("added stdio server `{name}` → {argv}\n  Run `/tools` (or restart the REPL) to refresh the live registry."),
    )
}

async fn add_docker(state: &AppState, name: &str, image: String, args: &AddArgs) -> CliReply {
    let runtime = match detect_runtime().await {
        Some(r) => r,
        None => {
            return CliReply::error(
                "mcp add --image: no container runtime found (install podman or docker)",
            )
        }
    };
    let entry = CustomToolEntry {
        key: name.to_string(),
        name: name.to_string(),
        image: image.clone(),
        mcp_server_cmd: args.mcp_server_cmd.clone(),
        mount_workspace: args.mount_workspace,
        mount_read_only: args.mount_read_only,
        append_workspace_roots: args.append_workspace_roots,
        direct_return: args.direct_return,
    };
    let log: tool_engine::LogFn = {
        let state = state.clone();
        Box::new(move |line| {
            let state = state.clone();
            let line = line.to_string();
            // emit_log is async; spawn a fire-and-forget so the install thread
            // can keep streaming pull progress.
            tokio::spawn(async move {
                state.emit_log("mcp", &line).await;
            });
        })
    };
    match tool_engine::add_custom_tool(
        entry,
        &runtime,
        &state.mcp_config_path,
        &state.mcp_config_mutex,
        &log,
    )
    .await
    {
        Ok(()) => CliReply::code(
            "bash",
            format!(
                "installed Docker MCP server `{name}` → {image}\n  Run `/tools` (or restart the REPL) to refresh the live registry."
            ),
        ),
        Err(e) => CliReply::error(format!("mcp add --image: {e}")),
    }
}

async fn upsert_and_save(state: &AppState, name: String, entry: ServerEntry) -> Result<(), String> {
    let _guard = state.mcp_config_mutex.lock().await;
    let mut cfg = mcp_service::load_or_init_config(&state.mcp_config_path)?;
    cfg.servers.insert(name, entry);
    mcp_service::save_config(&state.mcp_config_path, &cfg)
}

fn describe_entry(entry: &ServerEntry) -> (&'static str, String) {
    match entry {
        ServerEntry::Native { id } => ("native", format!("id={id}")),
        ServerEntry::Stdio {
            command,
            args,
            direct_return,
            ..
        } => {
            let argv = if args.is_empty() {
                command.clone()
            } else {
                format!("{command} {}", args.join(" "))
            };
            let dr = if *direct_return {
                " [direct_return]"
            } else {
                ""
            };
            ("stdio", format!("{argv}{dr}"))
        }
        ServerEntry::Http {
            url,
            headers,
            direct_return,
        } => {
            let dr = if *direct_return {
                " [direct_return]"
            } else {
                ""
            };
            let h = if headers.is_empty() {
                String::new()
            } else {
                format!(
                    " headers=[{}]",
                    headers.keys().cloned().collect::<Vec<_>>().join(",")
                )
            };
            ("http", format!("{url}{h}{dr}"))
        }
    }
}

/// Slash/native dispatch entry point. Parses `rest` (whitespace-tokenized) into
/// a sub-action + AddArgs and runs the right handler. Kept here so REPL,
/// Telegram bridge, and one-shot CLI all share the same parser.
pub async fn run_from_args(state: &AppState, action: &str, rest: &str) -> CliReply {
    match action {
        "" | "list" => list(state).await,
        "add" => match parse_add_args(rest) {
            Ok(args) => add(state, args).await,
            Err(e) => CliReply::error(format!("mcp add: {e}")),
        },
        "remove" | "rm" => {
            let name = rest.split_whitespace().next().unwrap_or("");
            remove(state, name).await
        }
        "import" => {
            let path = rest.trim();
            if path.is_empty() {
                return CliReply::error("mcp import: path required");
            }
            import(state, path).await
        }
        other => CliReply::error(format!(
            "mcp: unknown action `{other}` (use list | add | remove | import)"
        )),
    }
}

/// Tiny flag parser for `add` — kept here so we never reach for `clap` from a
/// hot dispatch path. Accepts `--flag value`, `--flag=value`, and repeating
/// `--arg`/`--header` for argv/header lists.
pub fn parse_add_args(rest: &str) -> Result<AddArgs, String> {
    let mut out = AddArgs {
        mount_read_only: true,
        ..AddArgs::default()
    };
    let tokens: Vec<String> = shellish_split(rest)?;
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        let (flag, inline_val) = match tok.split_once('=') {
            Some((k, v)) if k.starts_with("--") => (k.to_string(), Some(v.to_string())),
            _ => (tok.clone(), None),
        };
        let take_value = |out_idx: &mut usize, label: &str| -> Result<String, String> {
            if let Some(v) = inline_val.clone() {
                return Ok(v);
            }
            *out_idx += 1;
            if *out_idx >= tokens.len() {
                return Err(format!("{label} requires a value"));
            }
            Ok(tokens[*out_idx].clone())
        };
        match flag.as_str() {
            "--url" => out.url = Some(take_value(&mut i, "--url")?),
            "--image" => out.image = Some(take_value(&mut i, "--image")?),
            "--command" => out.command = Some(take_value(&mut i, "--command")?),
            "--arg" => out.stdio_args.push(take_value(&mut i, "--arg")?),
            "--cmd" => out.mcp_server_cmd.push(take_value(&mut i, "--cmd")?),
            "--header" => {
                let raw = take_value(&mut i, "--header")?;
                let (k, v) = raw
                    .split_once(':')
                    .or_else(|| raw.split_once('='))
                    .ok_or_else(|| {
                        format!("--header `{raw}`: expected `Key: value` or `Key=value`")
                    })?;
                out.headers
                    .push((k.trim().to_string(), v.trim().to_string()));
            }
            "--env" => {
                let raw = take_value(&mut i, "--env")?;
                let (k, v) = raw
                    .split_once('=')
                    .ok_or_else(|| format!("--env `{raw}`: expected `KEY=value`"))?;
                out.stdio_env
                    .push((k.trim().to_string(), v.trim().to_string()));
            }
            "--mount-workspace" => out.mount_workspace = true,
            "--mount-rw" => out.mount_read_only = false,
            "--append-roots" => out.append_workspace_roots = true,
            "--direct-return" => out.direct_return = true,
            other if other.starts_with('-') => return Err(format!("unknown flag `{other}`")),
            // Positional: first non-flag token is the server name.
            _ => {
                if out.name.is_empty() {
                    out.name = tok.clone();
                } else {
                    return Err(format!("unexpected positional `{tok}`"));
                }
            }
        }
        i += 1;
    }
    if out.name.is_empty() {
        return Err("name is required (first positional argument)".to_string());
    }
    Ok(out)
}

/// Minimal shell-style splitter that honours single + double quotes. Avoids a
/// new crate just for this; MCP argv values rarely need anything fancier.
fn shellish_split(input: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut iter = input.chars().peekable();
    while let Some(c) = iter.next() {
        match c {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\\' if !in_single => {
                if let Some(next) = iter.next() {
                    cur.push(next);
                }
            }
            ws if ws.is_whitespace() && !in_single && !in_double => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            other => cur.push(other),
        }
    }
    if in_single || in_double {
        return Err("unbalanced quote".to_string());
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shellish_split_handles_quotes() {
        let v = shellish_split(r#"a "b c" 'd e' f"#).unwrap();
        assert_eq!(v, vec!["a", "b c", "d e", "f"]);
    }

    #[test]
    fn shellish_split_rejects_unbalanced_quote() {
        assert!(shellish_split("\"oops").is_err());
    }

    #[test]
    fn parse_add_url_with_header() {
        let a =
            parse_add_args("gh --url https://x.example/mcp --header \"Authorization: Bearer t\"")
                .unwrap();
        assert_eq!(a.name, "gh");
        assert_eq!(a.url.as_deref(), Some("https://x.example/mcp"));
        assert_eq!(a.headers, vec![("Authorization".into(), "Bearer t".into())]);
    }

    #[test]
    fn parse_add_image_with_flags() {
        let a = parse_add_args(
            "fs --image ghcr.io/example/server-fs:latest --mount-workspace --append-roots",
        )
        .unwrap();
        assert_eq!(a.name, "fs");
        assert_eq!(a.image.as_deref(), Some("ghcr.io/example/server-fs:latest"));
        assert!(a.mount_workspace);
        assert!(a.append_workspace_roots);
    }

    #[test]
    fn parse_add_stdio_with_args_and_env() {
        let a = parse_add_args(
            "echo --command npx --arg -y --arg @scope/server --env FOO=bar --env BAZ=qux",
        )
        .unwrap();
        assert_eq!(a.command.as_deref(), Some("npx"));
        assert_eq!(a.stdio_args, vec!["-y", "@scope/server"]);
        assert_eq!(
            a.stdio_env,
            vec![("FOO".into(), "bar".into()), ("BAZ".into(), "qux".into())]
        );
    }

    #[test]
    fn parse_add_rejects_no_name() {
        let err = parse_add_args("--url https://x").unwrap_err();
        assert!(err.contains("name is required"));
    }

    #[test]
    fn parse_add_accepts_inline_eq() {
        let a = parse_add_args("gh --url=https://x.example/mcp").unwrap();
        assert_eq!(a.url.as_deref(), Some("https://x.example/mcp"));
    }
}
