use crate::infrastructure::bot_lifecycle;
use crate::modules::bot::repository;
use crate::modules::keywords::all_keyword_groups;
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

    {
        let mut lock = state.connection.lock().await;
        *lock = None;
    }
    repository::clear(&state.store_path)?;
    state.emit_log("ok", "Disconnected via Tauri command").await;
    Ok("disconnected".into())
}

/// Native folder picker for MCP filesystem allow-list (desktop).
#[cfg(desktop)]
#[tauri::command]
pub async fn pick_mcp_filesystem_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let folder = app
        .dialog()
        .file()
        .set_title("Folder for MCP filesystem tools")
        .blocking_pick_folder();
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
