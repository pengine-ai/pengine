use crate::modules::mcp::registry::ToolRegistry;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{Mutex, Notify, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionData {
    pub bot_token: String,
    pub bot_id: String,
    pub bot_username: String,
    pub connected_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub kind: String,
    pub message: String,
}

#[derive(Clone)]
pub struct AppState {
    pub connection: Arc<Mutex<Option<ConnectionData>>>,
    pub shutdown_notify: Arc<Notify>,
    pub bot_running: Arc<Mutex<bool>>,
    pub log_tx: Arc<Mutex<Option<tokio::sync::broadcast::Sender<LogEntry>>>>,
    pub store_path: PathBuf,
    pub app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    pub mcp: Arc<RwLock<ToolRegistry>>,
}

impl AppState {
    pub fn new(store_path: PathBuf) -> Self {
        let (log_tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            connection: Arc::new(Mutex::new(None)),
            shutdown_notify: Arc::new(Notify::new()),
            bot_running: Arc::new(Mutex::new(false)),
            log_tx: Arc::new(Mutex::new(Some(log_tx))),
            store_path,
            app_handle: Arc::new(Mutex::new(None)),
            mcp: Arc::new(RwLock::new(ToolRegistry::default())),
        }
    }

    pub async fn emit_log(&self, kind: &str, message: &str) {
        let entry = LogEntry {
            timestamp: Utc::now().format("%H:%M:%S").to_string(),
            kind: kind.to_string(),
            message: message.to_string(),
        };

        if let Some(tx) = self.log_tx.lock().await.as_ref() {
            let _ = tx.send(entry.clone());
        }

        if let Some(handle) = self.app_handle.lock().await.as_ref() {
            let _ = handle.emit("pengine-log", &entry);
        }
    }
}
