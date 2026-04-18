use crate::modules::cron::types::CronJob;
use crate::modules::mcp::registry::ToolRegistry;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::Emitter;
use tokio::sync::{Mutex, Notify, RwLock};

const RECENT_TOOLS_CAP: usize = 32;
const TOOL_CTX_LATENCY_CAP: usize = 128;

/// In-memory connection record. Holds the plaintext bot token while the bot runs.
/// Not serializable on purpose — the token must never reach disk. Use
/// `ConnectionMetadata` for anything persisted to `connection.json`.
#[derive(Clone)]
pub struct ConnectionData {
    pub bot_token: String,
    pub bot_id: String,
    pub bot_username: String,
    pub connected_at: DateTime<Utc>,
}

impl fmt::Debug for ConnectionData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectionData")
            .field("bot_token", &"<redacted>")
            .field("bot_id", &self.bot_id)
            .field("bot_username", &self.bot_username)
            .field("connected_at", &self.connected_at)
            .finish()
    }
}

/// Persisted shape of `connection.json`. The bot token lives in the OS keychain and
/// is loaded on demand via `modules::secure_store`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionMetadata {
    pub bot_id: String,
    pub bot_username: String,
    pub connected_at: DateTime<Utc>,
}

impl From<&ConnectionData> for ConnectionMetadata {
    fn from(c: &ConnectionData) -> Self {
        Self {
            bot_id: c.bot_id.clone(),
            bot_username: c.bot_username.clone(),
            connected_at: c.connected_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct MemorySession {
    pub entity_name: String,
    pub turn_count: u32,
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
    /// Flat tool names last invoked by the model (FIFO, for routing next turns).
    pub recent_tool_names: Arc<Mutex<Vec<String>>>,
    /// Milliseconds spent in tool subset selection (rolling, for p95 logs).
    pub tool_ctx_latency_ms: Arc<Mutex<Vec<u64>>>,
    /// Max UTF-8 bytes for the combined skills system-prompt fragment (dashboard / `user_settings.json`).
    pub skills_hint_max_bytes: Arc<RwLock<u32>>,
    /// `$APP_DATA/cron.json` — scheduled jobs + last-known Telegram chat id.
    pub cron_path: PathBuf,
    pub cron_jobs: Arc<RwLock<Vec<CronJob>>>,
    /// Wake the scheduler immediately after CRUD / enable / test operations.
    pub cron_notify: Arc<Notify>,
    /// Last Telegram chat id that sent a message to the bot. Scheduled cron jobs
    /// push their replies here. Persisted inside `cron.json`.
    pub last_chat_id: Arc<RwLock<Option<i64>>>,
    /// Rate-limit scheduler logs when jobs are due but `last_chat_id` is still unknown.
    pub cron_no_chat_warned: Arc<AtomicBool>,
    /// Serializes `cron.json` snapshots + disk writes with HTTP / scheduler / MCP callers.
    pub cron_save_mutex: Arc<Mutex<()>>,
}

impl AppState {
    pub fn new(store_path: PathBuf, mcp_config_path: PathBuf, mcp_config_source: String) -> Self {
        let skills_cap = crate::shared::user_settings::load_skills_hint_max_bytes(&store_path);
        let cron_path = crate::modules::cron::repository::cron_path(&store_path);
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
            recent_tool_names: Arc::new(Mutex::new(Vec::new())),
            tool_ctx_latency_ms: Arc::new(Mutex::new(Vec::new())),
            skills_hint_max_bytes: Arc::new(RwLock::new(skills_cap)),
            cron_path,
            cron_jobs: Arc::new(RwLock::new(Vec::new())),
            cron_notify: Arc::new(Notify::new()),
            last_chat_id: Arc::new(RwLock::new(None)),
            cron_no_chat_warned: Arc::new(AtomicBool::new(false)),
            cron_save_mutex: Arc::new(Mutex::new(())),
        }
    }

    /// Snapshot of recently invoked tool names in **insertion order** (oldest
    /// first, newest last). Callers relying on recency weighting (e.g. the
    /// tool router) must treat a larger index as "more recent".
    pub async fn recent_tools_snapshot(&self) -> Vec<String> {
        self.recent_tool_names.lock().await.clone()
    }

    pub async fn note_tools_used(&self, names: &[String]) {
        if names.is_empty() {
            return;
        }
        let mut g = self.recent_tool_names.lock().await;
        for n in names {
            let t = n.trim();
            if t.is_empty() {
                continue;
            }
            g.push(t.to_string());
        }
        while g.len() > RECENT_TOOLS_CAP {
            g.remove(0);
        }
    }

    pub async fn record_tool_selection_ms(&self, ms: u64) {
        let mut buf = self.tool_ctx_latency_ms.lock().await;
        buf.push(ms);
        while buf.len() > TOOL_CTX_LATENCY_CAP {
            buf.remove(0);
        }
        let n = buf.len();
        if n >= 10 && n % 10 == 0 {
            let mut s = buf.clone();
            s.sort_unstable();
            let idx = ((n as f64 * 0.95).ceil() as usize)
                .saturating_sub(1)
                .min(n - 1);
            let p95 = s[idx];
            self.emit_log(
                "tool_ctx",
                &format!("p95_select_ms={p95} samples={n} (routing histogram)"),
            )
            .await;
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
