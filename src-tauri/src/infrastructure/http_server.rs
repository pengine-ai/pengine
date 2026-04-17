use crate::infrastructure::bot_lifecycle;
use crate::modules::bot::{repository, service as bot_service};
use crate::modules::mcp::service as mcp_service;
use crate::modules::ollama::service as ollama_service;
use crate::modules::skills::service as skills_service;
use crate::modules::skills::types::{ClawHubSkill, Skill};
use crate::modules::tool_engine::{runtime as te_runtime, service as te_service};
use crate::shared::state::{AppState, ConnectionData};
use axum::extract::Query;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Json, Sse};
use axum::routing::{delete, get, post, put};
use axum::Router;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use socket2::{Domain, Socket, Type};
use std::convert::Infallible;
use std::io::ErrorKind;
use std::time::Duration;
use tokio_stream::{Stream, StreamExt};
use tower_http::cors::{Any, CorsLayer};

pub const DEFAULT_PORT: u16 = 21516;

#[derive(Deserialize)]
pub struct ConnectRequest {
    pub bot_token: String,
}

#[derive(Serialize)]
pub struct ConnectResponse {
    pub status: String,
    pub bot_id: String,
    pub bot_username: String,
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub bot_connected: bool,
    pub bot_username: Option<String>,
    pub bot_id: Option<String>,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Serialize)]
pub struct McpToolDto {
    pub server: String,
    pub name: String,
    pub description: Option<String>,
}

#[derive(Serialize)]
pub struct McpConfigInfoResponse {
    pub config_path: String,
    /// `"project"` or `"app_data"`
    pub source: String,
    pub filesystem_allowed_paths: Vec<String>,
}

#[derive(Deserialize)]
pub struct PutMcpFilesystemBody {
    pub paths: Vec<String>,
}

#[derive(Serialize)]
pub struct OllamaModelsResponse {
    pub reachable: bool,
    pub active_model: Option<String>,
    pub selected_model: Option<String>,
    pub models: Vec<String>,
}

#[derive(Deserialize)]
pub struct PutOllamaModelBody {
    pub model: Option<String>,
}

pub async fn start_server(state: AppState) {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/v1/connect", post(handle_connect))
        .route("/v1/connect", delete(handle_disconnect))
        .route("/v1/health", get(handle_health))
        .route("/v1/logs", get(handle_logs_sse))
        .route("/v1/ollama/models", get(handle_ollama_models))
        .route("/v1/ollama/model", put(handle_ollama_model_put))
        .route("/v1/mcp/tools", get(handle_mcp_tools))
        .route("/v1/mcp/config", get(handle_mcp_config_get))
        .route("/v1/mcp/filesystem", put(handle_mcp_filesystem_put))
        .route("/v1/mcp/servers", get(handle_mcp_servers_list))
        .route("/v1/mcp/servers/{name}", put(handle_mcp_server_upsert))
        .route("/v1/mcp/servers/{name}", delete(handle_mcp_server_delete))
        .route("/v1/toolengine/runtime", get(handle_toolengine_runtime))
        .route("/v1/toolengine/catalog", get(handle_toolengine_catalog))
        .route("/v1/toolengine/installed", get(handle_toolengine_installed))
        .route("/v1/toolengine/install", post(handle_toolengine_install))
        .route(
            "/v1/toolengine/uninstall",
            post(handle_toolengine_uninstall),
        )
        .route(
            "/v1/toolengine/private-folder",
            put(handle_toolengine_private_folder_put),
        )
        .route("/v1/toolengine/custom", get(handle_toolengine_custom_list))
        .route("/v1/toolengine/custom", post(handle_toolengine_custom_add))
        .route(
            "/v1/toolengine/custom/{key}",
            delete(handle_toolengine_custom_remove),
        )
        .route("/v1/skills", get(handle_skills_list))
        .route("/v1/skills", post(handle_skills_add))
        .route("/v1/skills/{slug}", delete(handle_skills_delete))
        .route("/v1/skills/{slug}/enabled", put(handle_skills_set_enabled))
        .route("/v1/skills/clawhub", get(handle_skills_clawhub_search))
        .route(
            "/v1/skills/clawhub/install",
            post(handle_skills_clawhub_install),
        )
        .layer(cors)
        .with_state(state.clone());

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT));

    let listener = retry_bind(addr, &state).await;
    state
        .emit_log("ok", &format!("HTTP API listening on http://{addr}"))
        .await;

    axum::serve(listener, app).await.expect("axum serve failed");
}

fn bind_loopback_reuse(addr: std::net::SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let socket = Socket::new(Domain::for_address(addr), Type::STREAM, None)?;
    socket.set_nonblocking(true)?;
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    let std_listener: std::net::TcpListener = socket.into();
    tokio::net::TcpListener::from_std(std_listener)
}

async fn retry_bind(addr: std::net::SocketAddr, state: &AppState) -> tokio::net::TcpListener {
    const MAX_ATTEMPTS: u32 = 30;
    const RETRY_DELAY: Duration = Duration::from_secs(2);

    let mut attempt = 0u32;
    loop {
        attempt += 1;
        match bind_loopback_reuse(addr) {
            Ok(listener) => return listener,
            Err(e) if attempt < MAX_ATTEMPTS => {
                let log = attempt == 1 || attempt.is_multiple_of(5);
                if log {
                    state
                        .emit_log(
                            "run",
                            &format!(
                                "Port {addr} busy (another instance or stale listener?), retry {attempt}/{MAX_ATTEMPTS} — {e}"
                            ),
                        )
                        .await;
                }
                tokio::time::sleep(RETRY_DELAY).await;
            }
            Err(e) => {
                panic!(
                    "failed to bind HTTP API on {addr} after {MAX_ATTEMPTS} attempts (~{}s): {e}. \
                     Quit other Pengine instances or free the port (e.g. `lsof -i :{}`).",
                    MAX_ATTEMPTS as u64 * RETRY_DELAY.as_secs(),
                    addr.port()
                );
            }
        }
    }
}

async fn handle_connect(
    State(state): State<AppState>,
    Json(body): Json<ConnectRequest>,
) -> Result<(StatusCode, Json<ConnectResponse>), (StatusCode, Json<ErrorResponse>)> {
    let token = body.bot_token.trim().to_string();
    if token.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "bot_token is required".into(),
            }),
        ));
    }

    state
        .emit_log("run", "Verifying token with Telegram…")
        .await;

    let me = bot_service::verify_token(&token)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: e })))?;

    let conn = ConnectionData {
        bot_token: token,
        bot_id: me.id.to_string(),
        bot_username: me.username().to_string(),
        connected_at: Utc::now(),
    };

    bot_lifecycle::stop_and_wait_for_bot(&state).await;

    repository::persist(&state.store_path, &conn).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    let spawn_token = conn.bot_token.clone();
    let response = ConnectResponse {
        status: "connected".into(),
        bot_id: conn.bot_id.clone(),
        bot_username: conn.bot_username.clone(),
    };

    state
        .emit_log("ok", &format!("Bot @{} connected", conn.bot_username))
        .await;

    {
        let mut lock = state.connection.lock().await;
        *lock = Some(conn);
    }

    let shutdown = state.shutdown_notify.clone();
    let spawn_state = state.clone();
    tokio::spawn(async move {
        bot_service::start_bot(spawn_state, spawn_token, shutdown).await;
    });

    Ok((StatusCode::OK, Json(response)))
}

async fn handle_disconnect(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    bot_lifecycle::stop_and_wait_for_bot(&state).await;

    {
        let mut lock = state.connection.lock().await;
        *lock = None;
    }

    repository::clear(&state.store_path).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    state.emit_log("ok", "Disconnected and cleared store").await;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "status": "disconnected" })),
    ))
}

async fn handle_health(State(state): State<AppState>) -> Json<HealthResponse> {
    let conn = state.connection.lock().await;
    Json(HealthResponse {
        status: "ok".into(),
        bot_connected: conn.is_some(),
        bot_username: conn.as_ref().map(|c| c.bot_username.clone()),
        bot_id: conn.as_ref().map(|c| c.bot_id.clone()),
    })
}

async fn handle_ollama_models(State(state): State<AppState>) -> Json<OllamaModelsResponse> {
    let selected_model = state.preferred_ollama_model.read().await.clone();
    match ollama_service::model_catalog(3000).await {
        Ok(catalog) => Json(OllamaModelsResponse {
            reachable: true,
            active_model: catalog.active,
            selected_model,
            models: catalog.models,
        }),
        Err(_) => Json(OllamaModelsResponse {
            reachable: false,
            active_model: None,
            selected_model,
            models: Vec::new(),
        }),
    }
}

async fn handle_ollama_model_put(
    State(state): State<AppState>,
    Json(body): Json<PutOllamaModelBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    let normalized = body
        .model
        .as_ref()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());

    if let Some(ref model) = normalized {
        let catalog = ollama_service::model_catalog(3000)
            .await
            .map_err(|e| (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: e })))?;
        if !catalog.models.iter().any(|m| m == model) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("model '{model}' is not available in Ollama"),
                }),
            ));
        }
    }

    {
        let mut lock = state.preferred_ollama_model.write().await;
        *lock = normalized.clone();
    }

    state
        .emit_log(
            "run",
            &format!(
                "ollama model {}",
                normalized
                    .as_ref()
                    .map(|m| format!("set to '{m}'"))
                    .unwrap_or_else(|| "reset to active".to_string())
            ),
        )
        .await;

    Ok((
        StatusCode::OK,
        Json(serde_json::json!({ "selected_model": normalized })),
    ))
}

async fn handle_mcp_config_get(State(state): State<AppState>) -> Json<McpConfigInfoResponse> {
    let filesystem_allowed_paths = state
        .mcp_config_path
        .exists()
        .then(|| mcp_service::read_config(&state.mcp_config_path).ok())
        .flatten()
        .map(|c| mcp_service::filesystem_allowed_paths(&c))
        .unwrap_or_default();

    Json(McpConfigInfoResponse {
        config_path: state.mcp_config_path.to_string_lossy().into_owned(),
        source: state.mcp_config_source.clone(),
        filesystem_allowed_paths,
    })
}

async fn handle_mcp_filesystem_put(
    State(state): State<AppState>,
    Json(body): Json<PutMcpFilesystemBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    let paths: Vec<String> = body
        .paths
        .iter()
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();

    let catalog_result = te_service::load_catalog().await;

    let sync_note = {
        let _guard = state.mcp_config_mutex.lock().await;

        let mut cfg = if state.mcp_config_path.exists() {
            mcp_service::read_config(&state.mcp_config_path)
                .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))?
        } else {
            mcp_service::load_or_init_config(&state.mcp_config_path).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse { error: e }),
                )
            })?
        };

        mcp_service::set_filesystem_allowed_paths(&mut cfg, &paths);

        let mut note = None::<String>;
        let bot_id = state
            .connection
            .lock()
            .await
            .as_ref()
            .map(|c| c.bot_id.clone());
        match &catalog_result {
            Ok(cat) => {
                if let Err(e) = te_service::sync_workspace_mounted_tools_for_catalog(
                    &mut cfg,
                    &paths,
                    cat,
                    &state.mcp_config_path,
                    bot_id,
                ) {
                    note = Some(e);
                }
            }
            Err(e) => note = Some(e.clone()),
        }

        mcp_service::save_config(&state.mcp_config_path, &cfg).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

        note
    };

    if let Some(msg) = sync_note {
        state
            .emit_log(
                "toolengine",
                &format!("file-manager entry not updated: {msg}"),
            )
            .await;
    }

    state
        .emit_log(
            "mcp",
            &format!(
                "workspace_roots ({}) updated → {}",
                paths.len(),
                state.mcp_config_path.display()
            ),
        )
        .await;

    let bg = state.clone();
    tokio::spawn(async move {
        if let Err(e) = mcp_service::rebuild_registry_into_state(&bg).await {
            bg.emit_log(
                "mcp",
                &format!("ERROR: MCP registry rebuild failed after workspace_roots update: {e}"),
            )
            .await;
        }
    });

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

async fn handle_mcp_tools(State(state): State<AppState>) -> Json<Vec<McpToolDto>> {
    Json(
        state
            .mcp
            .read()
            .await
            .all_tools()
            .into_iter()
            .map(|t| McpToolDto {
                server: t.server_name,
                name: t.name,
                description: t.description,
            })
            .collect(),
    )
}

// ── MCP server CRUD ──────────────────────────────────────────────────

#[derive(Serialize)]
struct McpServersResponse {
    servers: std::collections::BTreeMap<String, crate::modules::mcp::types::ServerEntry>,
}

async fn handle_mcp_servers_list(
    State(state): State<AppState>,
) -> Result<Json<McpServersResponse>, (StatusCode, Json<ErrorResponse>)> {
    let cfg = {
        let _guard = state.mcp_config_mutex.lock().await;
        mcp_service::load_or_init_config(&state.mcp_config_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?
    };
    Ok(Json(McpServersResponse {
        servers: cfg.servers,
    }))
}

fn is_valid_server_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// True when two stdio entries launch the same process/bind mounts; differs only in fields we can
/// patch without spawning a new stdio child (e.g. `direct_return`).
fn mcp_stdio_identity_ignores_direct_return(
    old: &crate::modules::mcp::types::ServerEntry,
    new: &crate::modules::mcp::types::ServerEntry,
) -> bool {
    use crate::modules::mcp::types::ServerEntry;
    match (old, new) {
        (
            ServerEntry::Stdio {
                command: c0,
                args: a0,
                env: e0,
                private_host_path: p0,
                ..
            },
            ServerEntry::Stdio {
                command: c1,
                args: a1,
                env: e1,
                private_host_path: p1,
                ..
            },
        ) => c0 == c1 && a0 == a1 && e0 == e1 && p0 == p1,
        _ => false,
    }
}

async fn handle_mcp_server_upsert(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(entry): Json<crate::modules::mcp::types::ServerEntry>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    if !is_valid_server_name(&name) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "server name must be alphanumeric, hyphens, or underscores (max 64 chars)"
                    .into(),
            }),
        ));
    }

    if let crate::modules::mcp::types::ServerEntry::Stdio { ref command, .. } = entry {
        if command.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "command must not be empty".into(),
                }),
            ));
        }
    }

    let old_entry = {
        let _guard = state.mcp_config_mutex.lock().await;
        let mut cfg = mcp_service::load_or_init_config(&state.mcp_config_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

        let old = cfg.servers.get(&name).cloned();
        if old.as_ref() == Some(&entry) {
            return Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))));
        }

        cfg.servers.insert(name.clone(), entry.clone());

        mcp_service::save_config(&state.mcp_config_path, &cfg).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

        old
    };

    let try_direct_patch = match (&old_entry, &entry) {
        (Some(old_e), new_e) if mcp_stdio_identity_ignores_direct_return(old_e, new_e) => {
            matches!(
                (old_e, new_e),
                (
                    crate::modules::mcp::types::ServerEntry::Stdio {
                        direct_return: a,
                        ..
                    },
                    crate::modules::mcp::types::ServerEntry::Stdio {
                        direct_return: b,
                        ..
                    },
                ) if a != b
            )
        }
        _ => false,
    };

    let patch_direct_return = match &entry {
        crate::modules::mcp::types::ServerEntry::Stdio { direct_return, .. } => *direct_return,
        _ => false,
    };

    state
        .emit_log("mcp", &format!("server '{name}' saved"))
        .await;

    let bg = state.clone();
    let name_bg = name.clone();
    tokio::spawn(async move {
        if try_direct_patch
            && mcp_service::patch_stdio_direct_return_in_registry(
                &bg,
                &name_bg,
                patch_direct_return,
            )
            .await
        {
            return;
        }
        if let Err(e) = mcp_service::rebuild_registry_into_state(&bg).await {
            bg.emit_log(
                "mcp",
                &format!("ERROR: MCP registry rebuild failed after server save: {e}"),
            )
            .await;
        }
    });

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

async fn handle_mcp_server_delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    {
        let _guard = state.mcp_config_mutex.lock().await;

        let mut cfg = mcp_service::load_or_init_config(&state.mcp_config_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

        if cfg.servers.remove(&name).is_none() {
            return Err((
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("server '{name}' not found"),
                }),
            ));
        }

        mcp_service::save_config(&state.mcp_config_path, &cfg).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;
    }

    state
        .emit_log("mcp", &format!("server '{name}' removed"))
        .await;

    let bg = state.clone();
    tokio::spawn(async move {
        if let Err(e) = mcp_service::rebuild_registry_into_state(&bg).await {
            bg.emit_log(
                "mcp",
                &format!("ERROR: MCP registry rebuild failed after server delete: {e}"),
            )
            .await;
        }
    });

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

// ── Tool Engine ─────────────────────────────────────────────────────

async fn handle_toolengine_runtime(State(_state): State<AppState>) -> Json<serde_json::Value> {
    match te_runtime::detect_runtime().await {
        Some(info) => Json(serde_json::json!({
            "available": true,
            "kind": info.kind,
            "version": info.version,
            "rootless": info.rootless,
        })),
        None => Json(serde_json::json!({ "available": false })),
    }
}

async fn handle_toolengine_catalog(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    let catalog = te_service::load_catalog().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    let installed_ids = te_service::installed_tool_ids(&state.mcp_config_path);

    let cfg_snap = state
        .mcp_config_path
        .exists()
        .then(|| mcp_service::read_config(&state.mcp_config_path).ok())
        .flatten();

    let tools: Vec<serde_json::Value> = catalog
        .tools
        .iter()
        .map(|t| {
            let stored_pf = cfg_snap.as_ref().and_then(|c| {
                let k = te_service::server_key(&t.id);
                match c.servers.get(&k)? {
                    crate::modules::mcp::types::ServerEntry::Stdio {
                        private_host_path, ..
                    } => private_host_path.as_deref(),
                    _ => None,
                }
            });
            let commands: Vec<serde_json::Value> = t
                .commands
                .iter()
                .map(|c| {
                    serde_json::json!({
                        "name": c.name,
                        "description": c.description,
                    })
                })
                .collect();
            let private_folder_json = t.private_folder.as_ref().map(|pf| {
                serde_json::json!({
                    "container_path": pf.container_path,
                    "file_env_var": pf.file_env_var,
                    "file_extension": pf.file_extension,
                })
            });
            let private_host_resolved: Option<String> = t.private_folder.as_ref().map(|_| {
                te_service::resolve_private_host_path(&state.mcp_config_path, &t.id, stored_pf)
                    .to_string_lossy()
                    .into_owned()
            });
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "version": t.current,
                "description": t.description,
                "installed": installed_ids.contains(&t.id),
                "commands": commands,
                "private_folder": private_folder_json,
                "private_host_path": private_host_resolved,
                "ignore_robots_txt": t.ignore_robots_txt,
                "robots_ignore_allowlist": t.robots_ignore_allowlist,
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "tools": tools })))
}

async fn handle_toolengine_installed(State(state): State<AppState>) -> Json<serde_json::Value> {
    let installed = te_service::installed_tool_ids(&state.mcp_config_path);
    Json(serde_json::json!({ "installed": installed }))
}

#[derive(Deserialize)]
struct ToolEngineActionBody {
    tool_id: String,
}

#[derive(Deserialize)]
struct PutToolPrivateFolderBody {
    tool_id: String,
    path: String,
}

async fn handle_toolengine_install(
    State(state): State<AppState>,
    Json(body): Json<ToolEngineActionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    let tool_id = body.tool_id;
    let runtime = match te_runtime::detect_runtime().await {
        Some(rt) => rt,
        None => {
            let msg = "no container runtime found (install Podman or Docker)";
            state.emit_log("toolengine", &format!("error: {msg}")).await;
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse { error: msg.into() }),
            ));
        }
    };

    {
        let _guard = state.tool_engine_mutex.lock().await;

        state
            .emit_log("toolengine", &format!("installing {tool_id}…"))
            .await;

        let log_state = state.clone();
        let log_fn: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |msg: &str| {
            let s = log_state.clone();
            let m = msg.to_string();
            tokio::spawn(async move { s.emit_log("toolengine", &m).await });
        });

        if let Err(e) = te_service::install_tool(
            &tool_id,
            &runtime,
            &state.mcp_config_path,
            &state.mcp_config_mutex,
            &log_fn,
        )
        .await
        {
            state
                .emit_log("toolengine", &format!("install failed: {e}"))
                .await;
            return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })));
        }

        state
            .emit_log("toolengine", &format!("{tool_id} installed"))
            .await;
    }

    // Respond immediately; MCP reconnect can take minutes (Podman / npx) and must not block the UI.
    let bg = state.clone();
    tokio::spawn(async move {
        if let Err(e) = mcp_service::rebuild_registry_into_state(&bg).await {
            bg.emit_log(
                "mcp",
                &format!("ERROR: MCP registry rebuild failed after tool install: {e}"),
            )
            .await;
        }
    });

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

async fn handle_toolengine_private_folder_put(
    State(state): State<AppState>,
    Json(body): Json<PutToolPrivateFolderBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    let tool_id = body.tool_id.trim().to_string();
    let path = body.path.trim().to_string();
    if tool_id.is_empty() || path.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "tool_id and path are required".into(),
            }),
        ));
    }

    let catalog = te_service::load_catalog().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    let entry = catalog
        .tools
        .iter()
        .find(|t| t.id == tool_id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: format!("unknown tool '{tool_id}'"),
                }),
            )
        })?;

    if entry.private_folder.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "this catalog tool does not declare private_folder".into(),
            }),
        ));
    }

    if !std::path::Path::new(&path).is_absolute() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "path must be an absolute host directory".into(),
            }),
        ));
    }

    let bot_id = state
        .connection
        .lock()
        .await
        .as_ref()
        .map(|c| c.bot_id.clone());

    {
        let _guard = state.mcp_config_mutex.lock().await;
        let mut cfg = mcp_service::load_or_init_config(&state.mcp_config_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

        let key = te_service::server_key(&tool_id);
        {
            let Some(server_ent) = cfg.servers.get_mut(&key) else {
                return Err((
                    StatusCode::NOT_FOUND,
                    Json(ErrorResponse {
                        error: format!("tool '{tool_id}' is not installed"),
                    }),
                ));
            };
            match server_ent {
                crate::modules::mcp::types::ServerEntry::Stdio {
                    private_host_path, ..
                } => {
                    if let Err(e) = tokio::fs::create_dir_all(&path).await {
                        let status = match e.kind() {
                            ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
                            ErrorKind::AlreadyExists => StatusCode::CONFLICT,
                            _ => StatusCode::INTERNAL_SERVER_ERROR,
                        };
                        return Err((
                            status,
                            Json(ErrorResponse {
                                error: format!("cannot create directory: {e}"),
                            }),
                        ));
                    }
                    *private_host_path = Some(path.clone());
                }
                _ => {
                    return Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: "tool server entry is not stdio".into(),
                        }),
                    ));
                }
            }
        }

        let host_paths = mcp_service::filesystem_allowed_paths(&cfg);
        te_service::sync_workspace_mounted_tools_for_catalog(
            &mut cfg,
            &host_paths,
            &catalog,
            &state.mcp_config_path,
            bot_id,
        )
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

        mcp_service::save_config(&state.mcp_config_path, &cfg).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;
    }

    state
        .emit_log(
            "toolengine",
            &format!("private data folder for {tool_id} set to {path}"),
        )
        .await;

    let bg = state.clone();
    tokio::spawn(async move {
        if let Err(e) = mcp_service::rebuild_registry_into_state(&bg).await {
            bg.emit_log(
                "mcp",
                &format!("ERROR: MCP registry rebuild failed after private-folder update: {e}"),
            )
            .await;
        }
    });

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

async fn handle_toolengine_uninstall(
    State(state): State<AppState>,
    Json(body): Json<ToolEngineActionBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    let tool_id = body.tool_id;
    let runtime = match te_runtime::detect_runtime().await {
        Some(rt) => rt,
        None => {
            let msg = "no container runtime found";
            state.emit_log("toolengine", &format!("error: {msg}")).await;
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse { error: msg.into() }),
            ));
        }
    };

    {
        let _guard = state.tool_engine_mutex.lock().await;

        state
            .emit_log("toolengine", &format!("uninstalling {tool_id}…"))
            .await;

        if let Err(e) = te_service::uninstall_tool(
            &tool_id,
            &runtime,
            &state.mcp_config_path,
            &state.mcp_config_mutex,
        )
        .await
        {
            state
                .emit_log("toolengine", &format!("uninstall failed: {e}"))
                .await;
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            ));
        }

        state
            .emit_log("toolengine", &format!("{tool_id} uninstalled"))
            .await;
    }

    let bg = state.clone();
    tokio::spawn(async move {
        if let Err(e) = mcp_service::rebuild_registry_into_state(&bg).await {
            bg.emit_log(
                "mcp",
                &format!("ERROR: MCP registry rebuild failed after tool uninstall: {e}"),
            )
            .await;
        }
    });

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

// ── Custom tools endpoints ────────────────────────────────────────────

async fn handle_toolengine_custom_list(State(state): State<AppState>) -> Json<serde_json::Value> {
    let tools = te_service::list_custom_tools(&state.mcp_config_path);
    let items: Vec<serde_json::Value> = tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "key": t.key,
                "name": t.name,
                "image": t.image,
                "mount_workspace": t.mount_workspace,
                "mount_read_only": t.mount_read_only,
                "append_workspace_roots": t.append_workspace_roots,
                "direct_return": t.direct_return,
            })
        })
        .collect();
    Json(serde_json::json!({ "custom_tools": items }))
}

#[derive(Deserialize)]
struct CustomToolAddBody {
    key: String,
    name: String,
    image: String,
    #[serde(default)]
    mcp_server_cmd: Vec<String>,
    #[serde(default)]
    mount_workspace: bool,
    #[serde(default = "default_true_serde")]
    mount_read_only: bool,
    #[serde(default)]
    append_workspace_roots: bool,
    #[serde(default)]
    direct_return: bool,
}

fn default_true_serde() -> bool {
    true
}

async fn handle_toolengine_custom_add(
    State(state): State<AppState>,
    Json(body): Json<CustomToolAddBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    let runtime = match te_runtime::detect_runtime().await {
        Some(rt) => rt,
        None => {
            let msg = "no container runtime found (install Podman or Docker)";
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse { error: msg.into() }),
            ));
        }
    };

    let entry = crate::modules::mcp::types::CustomToolEntry {
        key: body.key.clone(),
        name: body.name,
        image: body.image,
        mcp_server_cmd: body.mcp_server_cmd,
        mount_workspace: body.mount_workspace,
        mount_read_only: body.mount_read_only,
        append_workspace_roots: body.append_workspace_roots,
        direct_return: body.direct_return,
    };

    {
        let _guard = state.tool_engine_mutex.lock().await;

        state
            .emit_log("toolengine", &format!("adding custom tool '{}'…", body.key))
            .await;

        let log_state = state.clone();
        let log_fn: Box<dyn Fn(&str) + Send + Sync> = Box::new(move |msg: &str| {
            let s = log_state.clone();
            let m = msg.to_string();
            tokio::spawn(async move { s.emit_log("toolengine", &m).await });
        });

        if let Err(e) = te_service::add_custom_tool(
            entry,
            &runtime,
            &state.mcp_config_path,
            &state.mcp_config_mutex,
            &log_fn,
        )
        .await
        {
            state
                .emit_log("toolengine", &format!("add custom tool failed: {e}"))
                .await;
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            ));
        }

        state
            .emit_log("toolengine", &format!("custom tool '{}' added", body.key))
            .await;
    }

    let bg = state.clone();
    tokio::spawn(async move {
        if let Err(e) = mcp_service::rebuild_registry_into_state(&bg).await {
            bg.emit_log("mcp", &format!("ERROR: registry rebuild failed: {e}"))
                .await;
        }
    });

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

async fn handle_toolengine_custom_remove(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    let runtime = match te_runtime::detect_runtime().await {
        Some(rt) => rt,
        None => {
            let msg = "no container runtime found";
            return Err((
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse { error: msg.into() }),
            ));
        }
    };

    {
        let _guard = state.tool_engine_mutex.lock().await;

        state
            .emit_log("toolengine", &format!("removing custom tool '{key}'…"))
            .await;

        if let Err(e) = te_service::remove_custom_tool(
            &key,
            &runtime,
            &state.mcp_config_path,
            &state.mcp_config_mutex,
        )
        .await
        {
            state
                .emit_log("toolengine", &format!("remove custom tool failed: {e}"))
                .await;
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            ));
        }

        state
            .emit_log("toolengine", &format!("custom tool '{key}' removed"))
            .await;
    }

    let bg = state.clone();
    tokio::spawn(async move {
        if let Err(e) = mcp_service::rebuild_registry_into_state(&bg).await {
            bg.emit_log("mcp", &format!("ERROR: registry rebuild failed: {e}"))
                .await;
        }
    });

    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

#[derive(Serialize)]
pub struct SkillsListResponse {
    pub skills: Vec<Skill>,
    pub custom_dir: String,
}

#[derive(Deserialize)]
pub struct AddSkillBody {
    pub slug: String,
    pub markdown: String,
}

#[derive(Serialize)]
pub struct ClawHubSearchResponseDto {
    pub results: Vec<ClawHubSkill>,
}

#[derive(Deserialize)]
pub struct ClawHubSearchQuery {
    #[serde(default)]
    pub q: Option<String>,
}

#[derive(Deserialize)]
pub struct ClawHubInstallBody {
    pub slug: String,
}

#[derive(Deserialize)]
pub struct SetSkillEnabledBody {
    pub enabled: bool,
}

async fn handle_skills_list(State(state): State<AppState>) -> Json<SkillsListResponse> {
    let skills = skills_service::list_skills(&state.store_path);
    let custom_dir = skills_service::custom_skills_dir(&state.store_path)
        .to_string_lossy()
        .to_string();
    Json(SkillsListResponse { skills, custom_dir })
}

async fn handle_skills_add(
    State(state): State<AppState>,
    Json(body): Json<AddSkillBody>,
) -> Result<(StatusCode, Json<Skill>), (StatusCode, Json<ErrorResponse>)> {
    let skill = skills_service::write_custom_skill(&state.store_path, &body.slug, &body.markdown)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))?;
    state
        .emit_log("skills", &format!("custom skill '{}' saved", skill.slug))
        .await;
    Ok((StatusCode::OK, Json(skill)))
}

async fn handle_skills_delete(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    skills_service::delete_custom_skill(&state.store_path, &slug).map_err(|e| {
        if e.contains("custom skill '") && e.contains("' not found") {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "skill not found".into(),
                }),
            )
        } else {
            (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e }))
        }
    })?;
    state
        .emit_log("skills", &format!("custom skill '{slug}' removed"))
        .await;
    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

async fn handle_skills_clawhub_search(
    Query(params): Query<ClawHubSearchQuery>,
) -> Result<Json<ClawHubSearchResponseDto>, (StatusCode, Json<ErrorResponse>)> {
    let q = params.q.unwrap_or_default();
    let results = skills_service::search_clawhub(&q)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: e })))?;
    Ok(Json(ClawHubSearchResponseDto { results }))
}

async fn handle_skills_clawhub_install(
    State(state): State<AppState>,
    Json(body): Json<ClawHubInstallBody>,
) -> Result<(StatusCode, Json<Skill>), (StatusCode, Json<ErrorResponse>)> {
    let skill = skills_service::install_clawhub_skill(&state.store_path, &body.slug)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: e })))?;
    state
        .emit_log(
            "skills",
            &format!("installed ClawHub skill '{}'", skill.slug),
        )
        .await;
    Ok((StatusCode::OK, Json(skill)))
}

async fn handle_skills_set_enabled(
    State(state): State<AppState>,
    Path(slug): Path<String>,
    Json(body): Json<SetSkillEnabledBody>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    let known = skills_service::list_skills(&state.store_path)
        .iter()
        .any(|s| s.slug == slug);
    if !known {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "skill not found".into(),
            }),
        ));
    }
    skills_service::set_skill_enabled(&state.store_path, &slug, body.enabled)
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))?;
    state
        .emit_log(
            "skills",
            &format!(
                "skill '{slug}' {}",
                if body.enabled { "enabled" } else { "disabled" }
            ),
        )
        .await;
    Ok((StatusCode::OK, Json(serde_json::json!({ "ok": true }))))
}

async fn handle_logs_sse(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<axum::response::sse::Event, Infallible>>> {
    let rx = {
        let lock = state.log_tx.lock().await;
        lock.as_ref()
            .expect("log_tx should always exist")
            .subscribe()
    };

    let stream =
        tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|result| match result {
            Ok(entry) => {
                let json = serde_json::to_string(&entry).unwrap_or_default();
                Some(Ok(axum::response::sse::Event::default().data(json)))
            }
            Err(_) => None,
        });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    )
}
