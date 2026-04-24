use crate::infrastructure::audit_log;
use crate::infrastructure::http_server;
use crate::modules::bot::{commands, repository, service as bot_service};
use crate::modules::cli::bootstrap as cli_bootstrap;
use crate::modules::cron::{repository as cron_repository, scheduler as cron_scheduler};
use crate::modules::mcp::service as mcp_service;
use crate::modules::ollama::cloud as ollama_cloud;
use crate::modules::secure_store;
use crate::shared::state::{AppState, ConnectionData};
use std::path::PathBuf;
use tauri::Manager;

/// Main window is created here (not in `tauri.conf.json`) so CLI invocations
/// like bare `pengine` in CLI mode never instantiate a webview — only the GUI path runs this.
fn open_main_window(app: &tauri::App) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
        .title("pengine")
        .inner_size(800.0, 600.0)
        .build()?;
    Ok(())
}

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
        .plugin(tauri_plugin_cli::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // CLI mode short-circuits UI startup (`process::exit`) or returns early
            // for a GUI child (`PENGINE_OPEN_GUI=1` from `pengine app`). Otherwise
            // setup continues and `open_main_window` runs at the end.
            cli_bootstrap::handle_cli_or_continue(app);
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

            let (shared_state, audit_rx) = AppState::new(path, mcp_path, mcp_src.to_string());

            {
                let handle = app.handle().clone();
                let state = shared_state.clone();
                tauri::async_runtime::block_on(async move {
                    *state.app_handle.lock().await = Some(handle);
                });
            }

            app.manage(shared_state.clone());

            let audit_store = shared_state.store_path.clone();
            tauri::async_runtime::spawn(async move {
                audit_log::run_audit_writer(audit_store, audit_rx).await;
            });

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

            // Pre-warm the Ollama cloud catalog so the first dashboard refresh
            // returns cloud entries without the user waiting on ollama.com.
            tauri::async_runtime::spawn(async move {
                let _ = ollama_cloud::list_cloud_models().await;
            });

            open_main_window(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_connection_status,
            commands::disconnect_bot,
            commands::pick_mcp_filesystem_folder,
            commands::list_keyword_groups,
            commands::audit_list_files,
            commands::audit_read_file,
            commands::audit_delete_file,
            commands::cli_shim_status,
            commands::cli_shim_install,
            commands::cli_shim_remove,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
