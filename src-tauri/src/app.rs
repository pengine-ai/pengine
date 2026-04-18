use crate::infrastructure::http_server;
use crate::modules::bot::{commands, repository, service as bot_service};
use crate::modules::cron::{repository as cron_repository, scheduler as cron_scheduler};
use crate::modules::mcp::service as mcp_service;
use crate::modules::secure_store;
use crate::shared::state::{AppState, ConnectionData};
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

            // Warm unified secrets once before MCP rebuild and bot resume (avoids parallel prompts).
            // If legacy per-bot / per-MCP items still exist, migration may touch the keychain again
            // (a second prompt on first launch after upgrade is normal).
            //
            // Run on a plain `std::thread` (not `tokio::task::spawn_blocking` + `block_on`): during
            // Tao's `did_finish_launching` the Tokio runtime may not accept blocking tasks yet; that
            // combination panicked with `panic_cannot_unwind` on macOS and aborted the process.
            {
                let path_w = path.clone();
                let mcp_path_w = mcp_path.clone();
                match std::thread::Builder::new()
                    .name("pengine-warm-secrets".into())
                    .spawn(move || {
                        let mut warm_mig: Vec<String> = Vec::new();
                        let meta_for_warm = repository::load(&path_w, &mut warm_mig);
                        for line in warm_mig {
                            log::info!("{line}");
                        }
                        let bot_ids: Vec<String> = meta_for_warm
                            .as_ref()
                            .map(|m| vec![m.bot_id.clone()])
                            .unwrap_or_default();
                        let mcp_pairs = match mcp_service::load_or_init_config(&mcp_path_w) {
                            Ok(cfg) => mcp_service::catalog_passthrough_key_pairs(&cfg),
                            Err(e) => {
                                log::warn!("warm_app_secrets: skipped mcp pairs ({e})");
                                Vec::new()
                            }
                        };
                        if let Err(e) = secure_store::warm_app_secrets(&bot_ids, &mcp_pairs) {
                            log::warn!("warm_app_secrets failed: {e}");
                        }
                    }) {
                    Ok(handle) => {
                        if let Err(e) = handle.join() {
                            log::error!("warm_app_secrets thread panicked: {e:?}");
                        }
                    }
                    Err(e) => log::warn!("warm_app_secrets: could not spawn thread ({e})"),
                }
            }

            let shared_state = AppState::new(path, mcp_path, mcp_src.to_string());

            {
                let handle = app.handle().clone();
                let state = shared_state.clone();
                tauri::async_runtime::block_on(async move {
                    *state.app_handle.lock().await = Some(handle);
                });
            }

            app.manage(shared_state.clone());

            // Load persisted cron jobs + last-known Telegram chat id before the scheduler spins up,
            // so a scheduled job can fire on its first tick after restart.
            {
                let cron_state = shared_state.clone();
                tauri::async_runtime::block_on(async move {
                    match cron_repository::load(&cron_state.cron_path) {
                        Ok(file) => {
                            *cron_state.cron_jobs.write().await = file.jobs;
                            *cron_state.last_chat_id.write().await = file.last_chat_id;
                        }
                        Err(e) => {
                            cron_state
                                .emit_log("cron", &format!("load cron.json failed: {e}"))
                                .await;
                        }
                    }
                });
            }

            let scheduler_state = shared_state.clone();
            tauri::async_runtime::spawn(async move {
                cron_scheduler::run(scheduler_state).await;
            });

            // Connect MCP stdio servers in the background so window + HTTP API are not blocked by
            // slow starters (Podman containers, `npx`, etc.). The registry stays empty until connect
            // finishes; early Telegram turns simply omit tools until then.
            let mcp_path = shared_state.mcp_config_path.clone();
            let mcp_state = shared_state.clone();
            tauri::async_runtime::spawn(async move {
                mcp_state
                    .emit_log(
                        "mcp",
                        &format!("connecting servers in background ({})", mcp_path.display()),
                    )
                    .await;
                if let Err(e) = mcp_service::rebuild_registry_into_state(&mcp_state).await {
                    mcp_state
                        .emit_log(
                            "mcp",
                            &format!("ERROR: MCP registry rebuild failed on startup: {e}"),
                        )
                        .await;
                }
            });

            // Resume persisted Telegram connection if present (token is cached after warm_app_secrets).
            let resume_state = shared_state.clone();
            tauri::async_runtime::spawn(async move {
                let mut migration_log: Vec<String> = Vec::new();
                let meta = repository::load(&resume_state.store_path, &mut migration_log);
                for line in migration_log {
                    resume_state.emit_log("auth", &line).await;
                }
                let Some(meta) = meta else {
                    return;
                };
                resume_state
                    .emit_log("auth", "Loading saved bot token…")
                    .await;
                let token = match secure_store::load_token(&meta.bot_id) {
                    Ok(t) => t,
                    Err(e) => {
                        resume_state
                            .emit_log(
                                "auth",
                                &format!(
                                    "Could not unlock stored bot token for @{}: {e}. \
                                     Reconnect in the UI to save a new one.",
                                    meta.bot_username
                                ),
                            )
                            .await;
                        return;
                    }
                };
                resume_state
                    .emit_log("ok", &format!("Resuming bot @{}…", meta.bot_username))
                    .await;
                let conn = ConnectionData {
                    bot_token: token.clone(),
                    bot_id: meta.bot_id,
                    bot_username: meta.bot_username,
                    connected_at: meta.connected_at,
                };
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
            commands::list_keyword_groups,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
