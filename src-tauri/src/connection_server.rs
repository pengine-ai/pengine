use crate::state::{AppState, ConnectionData};
use crate::telegram_service;
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
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
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

    let me = telegram_service::verify_token(&token)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, Json(ErrorResponse { error: e })))?;

    let bot_id = me.id.to_string();
    let bot_username = me.username().to_string();

    // Stop existing bot if running
    stop_existing_bot(&state).await;

    let conn = ConnectionData {
        bot_token: token.clone(),
        bot_id: bot_id.clone(),
        bot_username: bot_username.clone(),
        connected_at: Utc::now(),
    };

    state.persist(&conn).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: e }),
        )
    })?;

    {
        let mut lock = state.connection.lock().await;
        *lock = Some(conn);
    }

    let shutdown = state.shutdown_notify.clone();
    let spawn_state = state.clone();
    let spawn_token = token.clone();
    tokio::spawn(async move {
        telegram_service::start_bot(spawn_state, spawn_token, shutdown).await;
    });

    state
        .emit_log("ok", &format!("Bot @{bot_username} connected"))
        .await;

    Ok((
        StatusCode::OK,
        Json(ConnectResponse {
            status: "connected".into(),
            bot_id,
            bot_username,
        }),
    ))
}

async fn handle_disconnect(
    State(state): State<AppState>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<ErrorResponse>)> {
    stop_existing_bot(&state).await;

    {
        let mut lock = state.connection.lock().await;
        *lock = None;
    }

    state.clear_persisted().map_err(|e| {
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
    })
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

async fn stop_existing_bot(state: &AppState) {
    let was_running = *state.bot_running.lock().await;
    if was_running {
        state.shutdown_notify.notify_waiters();
        state.emit_log("run", "Stopping existing bot…").await;
        // Give dispatcher a moment to shut down
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
