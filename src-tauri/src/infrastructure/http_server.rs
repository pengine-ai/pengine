use crate::infrastructure::bot_lifecycle;
use crate::modules::bot::{repository, service as bot_service};
use crate::shared::state::{AppState, ConnectionData};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Json, Sse};
use axum::routing::{delete, get, post};
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
        .route("/v1/mcp/tools", get(handle_mcp_tools))
        .layer(cors)
        .with_state(state.clone());

    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], DEFAULT_PORT));

    let listener = retry_bind(addr, &state).await;
    state
        .emit_log("ok", &format!("HTTP API listening on http://{addr}"))
        .await;

    axum::serve(listener, app).await.expect("axum serve failed");
}

/// Bind with `SO_REUSEADDR` so a quick restart can reclaim the port after the old socket
/// enters `TIME_WAIT`. Falls back to the same error as plain bind if another process still listens.
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
