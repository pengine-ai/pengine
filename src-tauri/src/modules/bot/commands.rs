use crate::infrastructure::audit_log;
use crate::infrastructure::bot_lifecycle;
use crate::modules::bot::repository;
use crate::modules::keywords::all_keyword_groups;
use crate::modules::secure_store;
use crate::shared::keywords::KeywordGroup;
use crate::shared::state::AppState;
#[cfg(desktop)]
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn get_connection_status(
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let conn = state.connection.lock().await;
    Ok(serde_json::json!({
        "connected": conn.is_some(),
        "bot_username": conn.as_ref().map(|c| &c.bot_username),
        "bot_id": conn.as_ref().map(|c| &c.bot_id),
    }))
}

#[tauri::command]
pub async fn disconnect_bot(state: tauri::State<'_, AppState>) -> Result<String, String> {
    bot_lifecycle::stop_and_wait_for_bot(&state).await;

    let bot_id = {
        let mut lock = state.connection.lock().await;
        let id = lock.as_ref().map(|c| c.bot_id.clone());
        *lock = None;
        id
    };
    repository::clear(&state.store_path)?;
    if let Some(id) = bot_id {
        if let Err(e) = secure_store::delete_token(&id) {
            state
                .emit_log(
                    "auth",
                    &format!("WARN: could not remove bot token from keychain: {e}"),
                )
                .await;
        }
    }
    state.emit_log("ok", "Disconnected via Tauri command").await;
    Ok("disconnected".into())
}

/// Native folder picker for MCP filesystem allow-list (desktop).
///
/// Uses the non-blocking callback API with a oneshot channel so the dialog
/// dispatch to the main thread doesn't deadlock against the async runtime
/// that's awaiting the result (a known issue with `blocking_pick_folder`
/// inside `async` Tauri commands on macOS).
#[cfg(desktop)]
#[tauri::command]
pub async fn pick_mcp_filesystem_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Folder for MCP filesystem tools")
        .pick_folder(move |folder| {
            let _ = tx.send(folder);
        });
    let folder = rx
        .await
        .map_err(|e| format!("folder picker closed unexpectedly: {e}"))?;
    Ok(folder.map(|p| p.to_string()))
}

#[cfg(not(desktop))]
#[tauri::command]
pub async fn pick_mcp_filesystem_folder() -> Result<Option<String>, String> {
    Err("folder picker is only available on desktop".into())
}

/// Dashboard overview of every user-message keyword group the agent reacts to.
/// Each group exposes its id, description, match mode, and phrases grouped by
/// language — making it obvious where to add a translation.
#[tauri::command]
pub fn list_keyword_groups() -> Vec<&'static KeywordGroup> {
    all_keyword_groups()
}

/// List daily audit files on disk (`{store}/logs/audit-*.log`).
#[tauri::command]
pub async fn audit_list_files(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<audit_log::AuditFileEntry>, String> {
    audit_log::list_audit_files(&state.store_path)
        .await
        .map_err(audit_log::command_error_from_io)
}

/// Read one day’s NDJSON audit file.
#[tauri::command]
pub async fn audit_read_file(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<String, String> {
    audit_log::read_audit_file(&state.store_path, date.trim())
        .await
        .map_err(audit_log::command_error_from_io)
}

/// Delete one day’s audit file.
#[tauri::command]
pub async fn audit_delete_file(
    state: tauri::State<'_, AppState>,
    date: String,
) -> Result<(), String> {
    audit_log::remove_audit_file(&state.store_path, date.trim())
        .await
        .map_err(audit_log::command_error_from_io)
}
