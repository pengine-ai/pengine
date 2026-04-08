use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{Mutex, Notify};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionData {
    pub bot_token: String,
    pub bot_id: String,
    pub bot_username: String,
    pub connected_at: DateTime<Utc>,
}

#[derive(Clone)]
pub struct AppState {
    pub connection: Arc<Mutex<Option<ConnectionData>>>,
    pub shutdown_notify: Arc<Notify>,
    pub bot_running: Arc<Mutex<bool>>,
    pub log_tx: Arc<Mutex<Option<tokio::sync::broadcast::Sender<LogEntry>>>>,
    pub store_path: PathBuf,
    pub app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub kind: String,
    pub message: String,
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

    pub fn persist(&self, data: &ConnectionData) -> Result<(), String> {
        let json = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&self.store_path, json).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_persisted(&self) -> Option<ConnectionData> {
        let json = std::fs::read_to_string(&self.store_path).ok()?;
        serde_json::from_str(&json).ok()
    }

    pub fn clear_persisted(&self) -> Result<(), String> {
        if self.store_path.exists() {
            std::fs::remove_file(&self.store_path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
