use crate::infrastructure::bot_lifecycle;
use crate::modules::bot::{repository, service as bot_service};
use crate::modules::mcp::service as mcp_service;
use crate::modules::ollama::service as ollama_service;
use crate::modules::tool_engine::{runtime as te_runtime, service as te_service};
use crate::shared::state::{AppState, ConnectionData};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{Json, Sse};
use axum::routing::{delete, get, post, put};
use axum::Router;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use socket2::{Domain, Socket, Type};
use std::convert::Infallible;
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
        if let Err(e) = te_service::sync_workspace_mounted_tools_if_installed(&mut cfg, &paths) {
            note = Some(e);
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

    {
        let _guard = state.mcp_config_mutex.lock().await;
        let mut cfg = mcp_service::load_or_init_config(&state.mcp_config_path).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;

        cfg.servers.insert(name.clone(), entry);

        mcp_service::save_config(&state.mcp_config_path, &cfg).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse { error: e }),
            )
        })?;
    }

    state
        .emit_log("mcp", &format!("server '{name}' saved"))
        .await;

    let bg = state.clone();
    tokio::spawn(async move {
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
    let catalog = te_service::load_catalog().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    let installed_ids = te_service::installed_tool_ids(&state.mcp_config_path);

    let tools: Vec<serde_json::Value> = catalog
        .tools
        .iter()
        .map(|t| {
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
            serde_json::json!({
                "id": t.id,
                "name": t.name,
                "version": t.version,
                "description": t.description,
                "installed": installed_ids.contains(&t.id),
                "commands": commands,
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

        if let Err(e) = te_service::install_tool(
            &tool_id,
            &runtime,
            &state.mcp_config_path,
            &state.mcp_config_mutex,
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
