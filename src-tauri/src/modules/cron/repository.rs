use super::types::CronFile;
use std::path::{Path, PathBuf};

/// `$APP_DATA/cron.json`, next to `connection.json` / `mcp.json`.
pub fn cron_path(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .map(|p| p.join("cron.json"))
        .unwrap_or_else(|| PathBuf::from("cron.json"))
}

pub fn load(path: &Path) -> Result<CronFile, String> {
    if !path.exists() {
        return Ok(CronFile::default());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read cron.json: {e}"))?;
    if raw.trim().is_empty() {
        return Ok(CronFile::default());
    }
    serde_json::from_str(&raw).map_err(|e| format!("parse cron.json: {e}"))
}

pub fn save(path: &Path, file: &CronFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create parent dirs for cron.json: {e}"))?;
    }
    let pretty =
        serde_json::to_string_pretty(file).map_err(|e| format!("encode cron.json: {e}"))?;
    std::fs::write(path, pretty).map_err(|e| format!("write cron.json: {e}"))
}
