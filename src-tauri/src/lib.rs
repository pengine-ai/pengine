mod connection_server;
mod state;
mod telegram_service;

use state::AppState;
use std::path::PathBuf;

fn store_path(app: &tauri::App) -> PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir");
    base.join("connection.json")
}

#[tauri::command]
async fn get_connection_status(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.connection.lock().await;
    let running = *state.bot_running.lock().await;
    Ok(serde_json::json!({
        "connected": conn.is_some() && running,
        "bot_username": conn.as_ref().map(|c| &c.bot_username),
        "bot_id": conn.as_ref().map(|c| &c.bot_id),
    }))
}

#[tauri::command]
async fn disconnect_bot(state: tauri::State<'_, AppState>) -> Result<String, String> {
    state.shutdown_notify.notify_waiters();
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    {
        let mut lock = state.connection.lock().await;
        *lock = None;
    }
    state.clear_persisted()?;
    state.emit_log("ok", "Disconnected via Tauri command").await;
    Ok("disconnected".into())
}

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let path = store_path(app);
            let shared_state = AppState::new(path);

            // Store AppHandle for event emission
            {
                let handle = app.handle().clone();
                let state = shared_state.clone();
                tauri::async_runtime::spawn(async move {
                    let mut lock = state.app_handle.lock().await;
                    *lock = Some(handle);
                });
            }

            app.manage(shared_state.clone());

            // Resume persisted connection if present
            let resume_state = shared_state.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(conn) = resume_state.load_persisted() {
                    resume_state
                        .emit_log("ok", &format!("Resuming bot @{}…", conn.bot_username))
                        .await;
                    {
                        let mut lock = resume_state.connection.lock().await;
                        *lock = Some(conn.clone());
                    }
                    let shutdown = resume_state.shutdown_notify.clone();
                    let token = conn.bot_token.clone();
                    telegram_service::start_bot(resume_state, token, shutdown).await;
                }
            });

            // Start localhost HTTP API
            let server_state = shared_state.clone();
            tauri::async_runtime::spawn(async move {
                connection_server::start_server(server_state).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_connection_status,
            disconnect_bot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
