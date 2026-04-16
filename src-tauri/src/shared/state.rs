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

/// Active "remember this session" recording state. While set, every completed turn is
/// appended as an observation on the session entity in the Memory server.
#[derive(Debug, Clone)]
pub struct MemorySession {
    /// Entity name in the knowledge graph, e.g. `session-20260416T183000Z`.
    pub entity_name: String,
    pub started_at: DateTime<Utc>,
    pub turn_count: u32,
    /// When true (`record` command), only user lines are saved — no model reply or tool loop.
    pub diary_only: bool,
}

#[derive(Clone)]
pub struct AppState {
    pub connection: Arc<Mutex<Option<ConnectionData>>>,
    pub shutdown_notify: Arc<Notify>,
    pub bot_running: Arc<Mutex<bool>>,
    pub log_tx: Arc<Mutex<Option<tokio::sync::broadcast::Sender<LogEntry>>>>,
    pub store_path: PathBuf,
    pub mcp_config_path: PathBuf,
    pub mcp_config_source: String,
    pub app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    pub mcp: Arc<RwLock<ToolRegistry>>,
    pub mcp_config_mutex: Arc<Mutex<()>>,
    /// Ensures only one MCP registry rebuild (stdio connects) runs at a time.
    pub mcp_rebuild_mutex: Arc<Mutex<()>>,
    pub preferred_ollama_model: Arc<RwLock<Option<String>>>,
    pub cached_filesystem_paths: Arc<RwLock<Vec<String>>>,
    pub tool_engine_mutex: Arc<Mutex<()>>,
    /// Active memory-session recording (toggled by keyword commands; see `bot::agent`).
    pub memory_session: Arc<RwLock<Option<MemorySession>>>,
}

impl AppState {
    pub fn new(store_path: PathBuf, mcp_config_path: PathBuf, mcp_config_source: String) -> Self {
        let (log_tx, _) = tokio::sync::broadcast::channel(256);
        Self {
            connection: Arc::new(Mutex::new(None)),
            shutdown_notify: Arc::new(Notify::new()),
            bot_running: Arc::new(Mutex::new(false)),
            log_tx: Arc::new(Mutex::new(Some(log_tx))),
            store_path,
            mcp_config_path,
            mcp_config_source,
            app_handle: Arc::new(Mutex::new(None)),
            mcp: Arc::new(RwLock::new(ToolRegistry::default())),
            mcp_config_mutex: Arc::new(Mutex::new(())),
            mcp_rebuild_mutex: Arc::new(Mutex::new(())),
            preferred_ollama_model: Arc::new(RwLock::new(None)),
            cached_filesystem_paths: Arc::new(RwLock::new(Vec::new())),
            tool_engine_mutex: Arc::new(Mutex::new(())),
            memory_session: Arc::new(RwLock::new(None)),
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
