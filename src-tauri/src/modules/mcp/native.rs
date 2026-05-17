use super::types::ToolDef;
use crate::modules::mcp::service as mcp_service;
use crate::modules::tool_engine::runtime as tool_engine_runtime;
use crate::modules::tool_engine::service as tool_engine_service;
use crate::shared::state::AppState;
use serde_json::{json, Value};
use std::collections::HashSet;

const MAX_SIDES: u64 = 1_000_000;

/// Server key / native id used in `mcp.json` for the built-in tool manager.
pub const TOOL_MANAGER_ID: &str = "tool_manager";

/// Server key / native id used in `mcp.json` for the built-in cron manager.
pub const CRON_MANAGER_ID: &str = "cron_manager";

/// Server key / native id used in `mcp.json` for the built-in task spawner.
pub const TASK_SPAWNER_ID: &str = "task_spawner";

/// Hard cap on `task_spawn` recursion. A value of 1 means: a top-level turn may
/// spawn child tasks, but those child tasks cannot spawn further children.
/// Bumping this allows deeper trees but multiplies cost and latency exponentially.
pub const TASK_SPAWN_MAX_DEPTH: u8 = 1;

enum NativeKind {
    Dice,
    ToolManager(AppState),
    CronManager(AppState),
    /// Task spawner: tool definition only. The actual recursive `run_model_turn`
    /// is invoked by the agent dispatcher, not via [`Provider::call_tool`], so
    /// that `Provider::call_tool`'s future stays `Send` (parallel tool dispatch
    /// uses `tokio::spawn`, which requires `Send`).
    TaskSpawner,
}

pub struct NativeProvider {
    pub server_name: String,
    pub tools: Vec<ToolDef>,
    kind: NativeKind,
}

impl NativeProvider {
    pub async fn call(&self, tool_name: &str, args: &Value) -> Result<String, String> {
        if !self.tools.iter().any(|t| t.name == tool_name) {
            return Err(format!("unknown native tool: {tool_name}"));
        }
        match &self.kind {
            NativeKind::Dice => handle_dice(tool_name, args),
            NativeKind::ToolManager(state) => handle_tool_manager(tool_name, args, state).await,
            NativeKind::CronManager(state) => handle_cron_manager(tool_name, args, state).await,
            NativeKind::TaskSpawner => Err(
                "task_spawn is dispatched by the agent loop, not via Provider::call_tool. \
                 If you see this error, the dispatcher missed an interception point."
                    .into(),
            ),
        }
    }
}

// ── Dice ────────────────────────────────────────────────────────────

pub fn dice_named(server_key: &str) -> NativeProvider {
    NativeProvider {
        server_name: server_key.to_string(),
        tools: vec![{
            let mut t = ToolDef {
                server_name: server_key.to_string(),
                name: "roll_dice".to_string(),
                description: Some(
                    "Roll a die with the given number of sides and return the result.".to_string(),
                ),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "sides": {
                            "type": "integer",
                            "description": "Number of sides (default 6, max 1 000 000)"
                        }
                    }
                }),
                direct_return: true,
                category: None,
                risk: super::types::ToolRisk::Low,
            };
            super::tool_metadata::apply(&mut t);
            t
        }],
        kind: NativeKind::Dice,
    }
}

pub fn dice() -> NativeProvider {
    dice_named("dice")
}

fn handle_dice(_tool_name: &str, args: &Value) -> Result<String, String> {
    let sides = args
        .get("sides")
        .and_then(|v| v.as_u64())
        .unwrap_or(6)
        .clamp(2, MAX_SIDES);

    let result = fastrand::u64(1..=sides);
    Ok(format!("Rolled a d{sides}: {result}"))
}

// ── Tool Manager ────────────────────────────────────────────────────

pub fn tool_manager_named(server_key: &str, state: AppState) -> NativeProvider {
    NativeProvider {
        server_name: server_key.to_string(),
        tools: vec![{
            let mut t = ToolDef {
                server_name: server_key.to_string(),
                name: "manage_tools".to_string(),
                description: Some(
                    "Manage container-based tools from the catalog. All catalog tools (e.g. File Manager) \
                     are user-managed and can be freely installed or uninstalled on request. \
                     Use action 'list' to see all available catalog tools and their install status. \
                     Use action 'install' with a tool_id to install one tool. \
                     Use action 'install_all' (no tool_id) to install every catalog tool not yet installed — \
                     prefer this when the user asks to install all tools. Never use 'uninstall_all' for that. \
                     Use action 'uninstall' with a tool_id to remove one installed tool. \
                     Use action 'uninstall_all' (no tool_id) only when the user asks to remove every catalog tool. \
                     Always call this tool when the user asks to install, uninstall, or list tools."
                        .to_string(),
                ),
                input_schema: json!({
                    "type": "object",
                    "required": ["action"],
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["list", "install", "install_all", "uninstall", "uninstall_all"],
                            "description": "The operation: 'list'; 'install' / 'uninstall' for one tool; 'install_all' / 'uninstall_all' for every catalog tool at once"
                        },
                        "tool_id": {
                            "type": "string",
                            "description": "Required for install and uninstall only. Omit for list, install_all, and uninstall_all. Use the exact id from the 'list' output (e.g. 'pengine/file-manager')."
                        }
                    }
                }),
                direct_return: false,
                category: None,
                risk: super::types::ToolRisk::Low,
            };
            super::tool_metadata::apply(&mut t);
            t
        }],
        kind: NativeKind::ToolManager(state),
    }
}

async fn handle_tool_manager(
    _tool_name: &str,
    args: &Value,
    state: &AppState,
) -> Result<String, String> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("missing 'action' parameter")?;

    match action {
        "list" => handle_list_tools(state).await,
        "install" => {
            let tool_id = args
                .get("tool_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'tool_id' for install")?;
            handle_install_tool(tool_id, state).await
        }
        "install_all" => handle_install_all_tools(state).await,
        "uninstall" => {
            let tool_id = args
                .get("tool_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'tool_id' for uninstall")?;
            handle_uninstall_tool(tool_id, state).await
        }
        "uninstall_all" => handle_uninstall_all_tools(state).await,
        _ => Err(format!("unknown action: {action}")),
    }
}

async fn handle_list_tools(state: &AppState) -> Result<String, String> {
    let catalog = tool_engine_service::load_catalog().await?;
    let installed = {
        let _cfg_guard = state.mcp_config_mutex.lock().await;
        tool_engine_service::installed_tool_ids(&state.mcp_config_path)
    };
    let installed_set: HashSet<&str> = installed.iter().map(|s| s.as_str()).collect();

    let mut lines = Vec::new();
    for tool in &catalog.tools {
        let status = if installed_set.contains(tool.id.as_str()) {
            "installed"
        } else {
            "not installed"
        };
        lines.push(format!(
            "- {} (id: {}, v{}): {} [{}]",
            tool.name, tool.id, tool.current, tool.description, status
        ));
    }

    if lines.is_empty() {
        Ok("No tools available in the catalog.".to_string())
    } else {
        Ok(format!("Available tools:\n{}", lines.join("\n")))
    }
}

async fn handle_install_tool(tool_id: &str, state: &AppState) -> Result<String, String> {
    run_tool_mutation(tool_id, state, "install", ToolAction::Install).await?;
    Ok(format!(
        "Tool '{tool_id}' installed successfully and is now available."
    ))
}

async fn handle_install_all_tools(state: &AppState) -> Result<String, String> {
    let runtime = tool_engine_runtime::detect_runtime().await.ok_or(
        "No container runtime (Docker/Podman) found. Please install Docker or Podman first.",
    )?;

    let summary = {
        let _te_guard = state.tool_engine_mutex.lock().await;
        state
            .emit_log(
                "toolengine",
                "installing all missing catalog tools via chat…",
            )
            .await;
        let log_state = state.clone();
        let log_fn: tool_engine_service::LogFn = Box::new(move |msg: &str| {
            let s = log_state.clone();
            let m = msg.to_string();
            tokio::spawn(async move { s.emit_log("toolengine", &m).await });
        });
        let out = tool_engine_service::install_all_catalog_tools(
            &runtime,
            &state.mcp_config_path,
            &state.mcp_config_mutex,
            &log_fn,
        )
        .await;
        state
            .emit_log("toolengine", "catalog install-all finished via chat")
            .await;
        out
    }?;

    if let Err(e) = mcp_service::rebuild_registry_into_state(state).await {
        state
            .emit_log(
                "mcp",
                &format!("registry rebuild after install_all failed: {e}"),
            )
            .await;
        return Err(e);
    }

    Ok(summary)
}

async fn handle_uninstall_tool(tool_id: &str, state: &AppState) -> Result<String, String> {
    run_tool_mutation(tool_id, state, "uninstall", ToolAction::Uninstall).await?;
    Ok(format!("Tool '{tool_id}' uninstalled successfully."))
}

async fn handle_uninstall_all_tools(state: &AppState) -> Result<String, String> {
    let runtime = tool_engine_runtime::detect_runtime().await.ok_or(
        "No container runtime (Docker/Podman) found. Please install Docker or Podman first.",
    )?;

    let summary = {
        let _te_guard = state.tool_engine_mutex.lock().await;
        state
            .emit_log("toolengine", "uninstalling all catalog tools via chat…")
            .await;
        let out = tool_engine_service::uninstall_all_catalog_tools(
            &runtime,
            &state.mcp_config_path,
            &state.mcp_config_mutex,
        )
        .await;
        state
            .emit_log("toolengine", "catalog uninstall-all finished via chat")
            .await;
        out
    }?;

    if let Err(e) = mcp_service::rebuild_registry_into_state(state).await {
        state
            .emit_log(
                "mcp",
                &format!("registry rebuild after uninstall_all failed: {e}"),
            )
            .await;
        return Err(e);
    }

    Ok(summary)
}

enum ToolAction {
    Install,
    Uninstall,
}

/// Shared sequence for install / uninstall: detect runtime, lock, log, act, log, rebuild.
async fn run_tool_mutation(
    tool_id: &str,
    state: &AppState,
    verb: &str,
    action: ToolAction,
) -> Result<(), String> {
    let runtime = tool_engine_runtime::detect_runtime().await.ok_or(
        "No container runtime (Docker/Podman) found. Please install Docker or Podman first.",
    )?;

    {
        let _te_guard = state.tool_engine_mutex.lock().await;
        state
            .emit_log("toolengine", &format!("{verb}ing {tool_id} via chat…"))
            .await;
        match action {
            ToolAction::Install => {
                let log_state = state.clone();
                let log_fn: tool_engine_service::LogFn = Box::new(move |msg: &str| {
                    let s = log_state.clone();
                    let m = msg.to_string();
                    tokio::spawn(async move { s.emit_log("toolengine", &m).await });
                });
                tool_engine_service::install_tool(
                    tool_id,
                    &runtime,
                    &state.mcp_config_path,
                    &state.mcp_config_mutex,
                    &log_fn,
                )
                .await?;
            }
            ToolAction::Uninstall => {
                tool_engine_service::uninstall_tool(
                    tool_id,
                    &runtime,
                    &state.mcp_config_path,
                    &state.mcp_config_mutex,
                )
                .await?;
            }
        }
        state
            .emit_log("toolengine", &format!("{tool_id} {verb}ed via chat"))
            .await;
    }

    if let Err(e) = mcp_service::rebuild_registry_into_state(state).await {
        state
            .emit_log("mcp", &format!("registry rebuild after {verb} failed: {e}"))
            .await;
        return Err(e);
    }
    Ok(())
}

// ── Cron Manager ────────────────────────────────────────────────────

pub fn cron_manager_named(server_key: &str, state: AppState) -> NativeProvider {
    NativeProvider {
        server_name: server_key.to_string(),
        tools: vec![{
            let mut t = ToolDef {
                server_name: server_key.to_string(),
                name: "manage_crons".to_string(),
                description: Some(
                    "Manage scheduled cron jobs. \
                     'list' returns every job (id, name, schedule, enabled, last_run_at). \
                     'enable' / 'disable' with a job_id toggle a job on/off. \
                     'create' schedules a new job (provide `name`, `instruction`, and either \
                     `every_minutes` OR `daily_at_hour` + `daily_at_minute`); the model can use this \
                     to self-schedule recurring follow-ups. 'delete' removes a job by id. \
                     Confirm with the user before creating or deleting jobs that affect them."
                        .to_string(),
                ),
                input_schema: json!({
                    "type": "object",
                    "required": ["action"],
                    "properties": {
                        "action": {
                            "type": "string",
                            "enum": ["list", "enable", "disable", "create", "delete"],
                            "description": "'list' returns every job; 'enable'/'disable'/'delete' need job_id; 'create' needs name + instruction + a schedule"
                        },
                        "job_id": {
                            "type": "string",
                            "description": "Required for 'enable', 'disable', 'delete'. Use the exact id from 'list'."
                        },
                        "name": {
                            "type": "string",
                            "description": "Human-readable name for 'create'."
                        },
                        "instruction": {
                            "type": "string",
                            "description": "Prompt the agent runs each time the job fires (for 'create')."
                        },
                        "condition": {
                            "type": "string",
                            "description": "Optional: only deliver a message to the user when this condition is met (for 'create')."
                        },
                        "every_minutes": {
                            "type": "integer",
                            "minimum": 1,
                            "maximum": 10080,
                            "description": "Recurring schedule in minutes (1..=10080). Mutually exclusive with daily_at_*."
                        },
                        "daily_at_hour": {
                            "type": "integer",
                            "minimum": 0,
                            "maximum": 23,
                            "description": "Local-time hour for a daily schedule. Pair with daily_at_minute."
                        },
                        "daily_at_minute": {
                            "type": "integer",
                            "minimum": 0,
                            "maximum": 59,
                            "description": "Local-time minute for a daily schedule."
                        }
                    }
                }),
                direct_return: false,
                category: None,
                risk: super::types::ToolRisk::Medium,
            };
            super::tool_metadata::apply(&mut t);
            t
        }],
        kind: NativeKind::CronManager(state),
    }
}

async fn handle_cron_manager(
    _tool_name: &str,
    args: &Value,
    state: &AppState,
) -> Result<String, String> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or("missing 'action' parameter")?;

    match action {
        "list" => Ok(format_cron_list(state).await),
        "enable" => {
            let job_id = args
                .get("job_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'job_id' for enable")?;
            set_cron_enabled(state, job_id, true).await
        }
        "disable" => {
            let job_id = args
                .get("job_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'job_id' for disable")?;
            set_cron_enabled(state, job_id, false).await
        }
        "create" => create_cron(state, args).await,
        "delete" => {
            let job_id = args
                .get("job_id")
                .and_then(|v| v.as_str())
                .ok_or("missing 'job_id' for delete")?;
            delete_cron(state, job_id).await
        }
        _ => Err(format!("unknown action: {action}")),
    }
}

fn parse_schedule_from_args(args: &Value) -> Result<crate::modules::cron::types::Schedule, String> {
    use crate::modules::cron::types::Schedule;
    let every = args.get("every_minutes").and_then(|v| v.as_u64());
    let hour = args.get("daily_at_hour").and_then(|v| v.as_u64());
    let minute = args.get("daily_at_minute").and_then(|v| v.as_u64());

    match (every, hour, minute) {
        (Some(_), Some(_), _) | (Some(_), _, Some(_)) => {
            Err("provide either every_minutes OR daily_at_*; not both".into())
        }
        (Some(m), None, None) => {
            let m = u32::try_from(m).map_err(|_| "every_minutes out of range".to_string())?;
            Ok(Schedule::EveryMinutes { minutes: m })
        }
        (None, Some(h), Some(m)) => Ok(Schedule::DailyAt {
            hour: h as u8,
            minute: m as u8,
        }),
        (None, Some(_), None) | (None, None, Some(_)) => {
            Err("daily_at requires both daily_at_hour and daily_at_minute".into())
        }
        (None, None, None) => Err("missing schedule: provide every_minutes or daily_at_*".into()),
    }
}

async fn create_cron(state: &AppState, args: &Value) -> Result<String, String> {
    use crate::modules::cron::{repository, service as cron_service, types::CronJob};

    let name = args
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or("missing 'name' for create")?
        .trim()
        .to_string();
    let instruction = args
        .get("instruction")
        .and_then(|v| v.as_str())
        .ok_or("missing 'instruction' for create")?
        .trim()
        .to_string();
    let condition = args
        .get("condition")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    let schedule = parse_schedule_from_args(args)?;
    cron_service::validate(&name, &instruction, &schedule)?;

    let job = CronJob {
        id: cron_service::new_job_id(),
        name,
        instruction,
        condition,
        skill_slugs: Vec::new(),
        schedule,
        enabled: true,
        created_at: chrono::Utc::now(),
        last_run_at: None,
    };

    let _save_guard = state.cron_save_mutex.lock().await;
    let snapshot = {
        let mut jobs = state.cron_jobs.write().await;
        jobs.push(job.clone());
        jobs.clone()
    };
    let last_chat_id = *state.last_chat_id.read().await;
    let file = crate::modules::cron::types::CronFile {
        jobs: snapshot,
        last_chat_id,
    };
    let path = state.cron_path.clone();
    let save_result = tokio::task::spawn_blocking(move || repository::save(&path, &file))
        .await
        .map_err(|e| format!("cron save task: {e}"))?;
    if let Err(e) = save_result {
        // Roll back the in-memory insertion so disk and memory stay in sync.
        let mut jobs = state.cron_jobs.write().await;
        if let Some(pos) = jobs.iter().position(|j| j.id == job.id) {
            jobs.remove(pos);
        }
        return Err(e);
    }
    state.cron_notify.notify_waiters();
    let schedule_desc = match &job.schedule {
        crate::modules::cron::types::Schedule::EveryMinutes { minutes } => {
            format!("every {minutes} min")
        }
        crate::modules::cron::types::Schedule::DailyAt { hour, minute } => {
            format!("daily at {hour:02}:{minute:02} local")
        }
    };
    Ok(format!(
        "Created job '{}' (id: {}, {schedule_desc}).",
        job.name, job.id
    ))
}

async fn delete_cron(state: &AppState, job_id: &str) -> Result<String, String> {
    use crate::modules::cron::repository;

    let _save_guard = state.cron_save_mutex.lock().await;
    let removed = {
        let mut jobs = state.cron_jobs.write().await;
        match jobs.iter().position(|j| j.id == job_id) {
            Some(pos) => jobs.remove(pos),
            None => return Err(format!("unknown job_id: {job_id}")),
        }
    };
    let snapshot = state.cron_jobs.read().await.clone();
    let last_chat_id = *state.last_chat_id.read().await;
    let file = crate::modules::cron::types::CronFile {
        jobs: snapshot,
        last_chat_id,
    };
    let path = state.cron_path.clone();
    let save_result = tokio::task::spawn_blocking(move || repository::save(&path, &file))
        .await
        .map_err(|e| format!("cron save task: {e}"))?;
    if let Err(e) = save_result {
        // Re-insert so disk and memory stay in sync.
        state.cron_jobs.write().await.push(removed);
        return Err(e);
    }
    state.cron_notify.notify_waiters();
    Ok(format!("Deleted job '{}' (id: {job_id}).", removed.name))
}

async fn format_cron_list(state: &AppState) -> String {
    let jobs = state.cron_jobs.read().await.clone();
    if jobs.is_empty() {
        return "No cron jobs configured. Add one from the Dashboard → Cron Jobs panel."
            .to_string();
    }
    let mut lines = Vec::with_capacity(jobs.len());
    for j in &jobs {
        let schedule = match &j.schedule {
            crate::modules::cron::types::Schedule::EveryMinutes { minutes } => {
                format!("every {minutes} min")
            }
            crate::modules::cron::types::Schedule::DailyAt { hour, minute } => {
                format!("daily at {hour:02}:{minute:02} (local)")
            }
        };
        let last = j
            .last_run_at
            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "never".to_string());
        let status = if j.enabled { "enabled" } else { "disabled" };
        let skills = if j.skill_slugs.is_empty() {
            String::new()
        } else {
            format!(" — skills: {}", j.skill_slugs.join(", "))
        };
        lines.push(format!(
            "- {name} (id: {id}, {schedule}) [{status}] — last_run: {last}{skills}",
            name = j.name,
            id = j.id,
        ));
    }
    format!("Cron jobs:\n{}", lines.join("\n"))
}

async fn set_cron_enabled(state: &AppState, job_id: &str, enabled: bool) -> Result<String, String> {
    let _save_guard = state.cron_save_mutex.lock().await;
    let updated = {
        let mut jobs = state.cron_jobs.write().await;
        let Some(job) = jobs.iter_mut().find(|j| j.id == job_id) else {
            return Err(format!("unknown job_id: {job_id}"));
        };
        if job.enabled == enabled {
            let verb = if enabled { "enabled" } else { "disabled" };
            return Ok(format!("Job '{}' is already {verb}.", job.name));
        }
        job.enabled = enabled;
        job.clone()
    };
    let snapshot = state.cron_jobs.read().await.clone();
    let last_chat_id = *state.last_chat_id.read().await;
    let file = crate::modules::cron::types::CronFile {
        jobs: snapshot,
        last_chat_id,
    };
    let path = state.cron_path.clone();
    let save_result =
        tokio::task::spawn_blocking(move || crate::modules::cron::repository::save(&path, &file))
            .await
            .map_err(|e| format!("cron save task: {e}"))?;
    if let Err(e) = save_result {
        let mut jobs = state.cron_jobs.write().await;
        if let Some(j) = jobs.iter_mut().find(|j| j.id == job_id) {
            j.enabled = !enabled;
        }
        return Err(e);
    }
    state.cron_notify.notify_waiters();
    let verb = if enabled { "enabled" } else { "disabled" };
    Ok(format!("Job '{}' {verb}.", updated.name))
}

// ── Task Spawner ────────────────────────────────────────────────────

pub fn task_spawner_named(server_key: &str) -> NativeProvider {
    NativeProvider {
        server_name: server_key.to_string(),
        tools: vec![{
            let mut t = ToolDef {
                server_name: server_key.to_string(),
                name: "task_spawn".to_string(),
                description: Some(
                    "Run an isolated sub-agent on a focused sub-task and return its single \
                     final reply as a string. Use for parallelizable research, scoped explorations, \
                     or work whose intermediate output would bloat the parent context. \
                     The sub-agent has access to the same tools, but starts with NO history of \
                     this conversation — restate the relevant context in `prompt`. Recursion is \
                     capped: sub-agents cannot spawn further sub-tasks."
                        .to_string(),
                ),
                input_schema: json!({
                    "type": "object",
                    "required": ["description", "prompt"],
                    "properties": {
                        "description": {
                            "type": "string",
                            "description": "Short (3–7 word) label describing the sub-task; surfaced in logs."
                        },
                        "prompt": {
                            "type": "string",
                            "description": "Self-contained prompt for the sub-agent. Include any context the parent already has."
                        }
                    }
                }),
                direct_return: false,
                category: None,
                risk: super::types::ToolRisk::Medium,
            };
            super::tool_metadata::apply(&mut t);
            t
        }],
        kind: NativeKind::TaskSpawner,
    }
}

// ── Registry ────────────────────────────────────────────────────────

/// Resolve `id` from `mcp.json` (`type: native`) into a provider under `server_key`.
/// `app_state` is required for stateful natives like `tool_manager`.
pub fn native_for(
    server_key: &str,
    id: &str,
    app_state: Option<&AppState>,
) -> Result<NativeProvider, String> {
    match id {
        "dice" => Ok(dice_named(server_key)),
        TOOL_MANAGER_ID => {
            let state = app_state.ok_or_else(|| format!("{TOOL_MANAGER_ID} requires AppState"))?;
            Ok(tool_manager_named(server_key, state.clone()))
        }
        CRON_MANAGER_ID => {
            let state = app_state.ok_or_else(|| format!("{CRON_MANAGER_ID} requires AppState"))?;
            Ok(cron_manager_named(server_key, state.clone()))
        }
        TASK_SPAWNER_ID => Ok(task_spawner_named(server_key)),
        _ => Err(format!("unknown native id: {id}")),
    }
}
