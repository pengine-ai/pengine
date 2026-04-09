use crate::infrastructure::http_server;
use crate::modules::bot::{commands, repository, service as bot_service};
use crate::modules::mcp::service as mcp_service;
use crate::shared::state::AppState;
use std::path::PathBuf;
use tauri::Manager;

fn store_path(app: &tauri::App) -> PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .expect("failed to resolve app data dir");
    base.join("connection.json")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let path = store_path(app);
            let shared_state = AppState::new(path);

            {
                let handle = app.handle().clone();
                let state = shared_state.clone();
                tauri::async_runtime::spawn(async move {
                    *state.app_handle.lock().await = Some(handle);
                });
            }

            app.manage(shared_state.clone());

            // Resume persisted Telegram connection if present.
            let resume_state = shared_state.clone();
            tauri::async_runtime::spawn(async move {
                let Some(conn) = repository::load(&resume_state.store_path) else {
                    return;
                };
                resume_state
                    .emit_log("ok", &format!("Resuming bot @{}…", conn.bot_username))
                    .await;
                let token = conn.bot_token.clone();
                *resume_state.connection.lock().await = Some(conn);
                let shutdown = resume_state.shutdown_notify.clone();
                bot_service::start_bot(resume_state, token, shutdown).await;
            });

            // Native tools are instant — no subprocess, no async I/O.
            let mcp_state = shared_state.clone();
            tauri::async_runtime::spawn(async move {
                let registry = mcp_service::build_default_registry();
                let n = registry.tool_names().len();
                *mcp_state.mcp.write().await = registry;
                mcp_state
                    .emit_log(
                        "mcp",
                        &format!("{n} native tool{}", if n == 1 { "" } else { "s" }),
                    )
                    .await;
            });

            // Start localhost HTTP API.
            let server_state = shared_state.clone();
            tauri::async_runtime::spawn(async move {
                http_server::start_server(server_state).await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_connection_status,
            commands::disconnect_bot,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
