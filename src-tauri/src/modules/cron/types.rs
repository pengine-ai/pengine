use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Schedule {
    EveryMinutes { minutes: u32 },
    DailyAt { hour: u8, minute: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub instruction: String,
    #[serde(default)]
    pub condition: String,
    #[serde(default)]
    pub skill_slugs: Vec<String>,
    pub schedule: Schedule,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_run_at: Option<DateTime<Utc>>,
}

fn default_true() -> bool {
    true
}

/// Root shape of `cron.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CronFile {
    #[serde(default)]
    pub jobs: Vec<CronJob>,
    /// Chat id of the most recent Telegram message, used as the delivery target
    /// for scheduled jobs. Persisted so restarts still have somewhere to send to.
    #[serde(default)]
    pub last_chat_id: Option<i64>,
}
