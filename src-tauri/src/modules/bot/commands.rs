use crate::infrastructure::bot_lifecycle;
use crate::modules::bot::repository;
use crate::shared::state::AppState;

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
