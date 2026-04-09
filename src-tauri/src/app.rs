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
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let path = store_path(app);
            let (mcp_path, mcp_src) = mcp_service::resolve_mcp_config_path(&path);
            let shared_state = AppState::new(path, mcp_path, mcp_src.to_string());

            {
                let handle = app.handle().clone();
                let state = shared_state.clone();
                tauri::async_runtime::spawn(async move {
                    *state.app_handle.lock().await = Some(handle);
                });
            }

            app.manage(shared_state.clone());

            // Load MCP before any bot work so the first Telegram message never sees an empty registry.
            let mcp_path = shared_state.mcp_config_path.clone();
            let mcp_state = shared_state.clone();
            tauri::async_runtime::block_on(async move {
                mcp_state
                    .emit_log("mcp", &format!("loading {}", mcp_path.display()))
                    .await;
                match mcp_service::load_or_init_config(&mcp_path) {
                    Ok(cfg) => {
                        mcp_service::rebuild_registry_into_state(&mcp_state, &cfg).await;
                    }
                    Err(e) => {
                        mcp_state
                            .emit_log("mcp", &format!("mcp.json error: {e}"))
                            .await;
                    }
                }
            });

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
            commands::pick_mcp_filesystem_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
