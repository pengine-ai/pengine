//! Optional preferences stored next to `connection.json` as `user_settings.json`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Default cap for the combined skills fragment in the system prompt (bytes, UTF-8).
pub const DEFAULT_SKILLS_HINT_MAX_BYTES: u32 = 10 * 1024;
pub const MIN_SKILLS_HINT_MAX_BYTES: u32 = 4 * 1024;
pub const MAX_SKILLS_HINT_MAX_BYTES: u32 = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct UserSettingsFile {
    #[serde(default)]
    skills_hint_max_bytes: Option<u32>,
}

/// Path to `user_settings.json` (same directory as `connection.json`).
pub fn user_settings_path(connection_json: &Path) -> PathBuf {
    connection_json
        .parent()
        .map(|p| p.join("user_settings.json"))
        .unwrap_or_else(|| PathBuf::from("user_settings.json"))
}

pub fn clamp_skills_hint_max_bytes(v: u32) -> u32 {
    v.clamp(MIN_SKILLS_HINT_MAX_BYTES, MAX_SKILLS_HINT_MAX_BYTES)
}

pub fn load_skills_hint_max_bytes(connection_json: &Path) -> u32 {
    let p = user_settings_path(connection_json);
    let Ok(raw) = std::fs::read_to_string(p) else {
        return DEFAULT_SKILLS_HINT_MAX_BYTES;
    };
    let parsed: UserSettingsFile = serde_json::from_str(&raw).unwrap_or_default();
    clamp_skills_hint_max_bytes(
        parsed
            .skills_hint_max_bytes
            .unwrap_or(DEFAULT_SKILLS_HINT_MAX_BYTES),
    )
}

pub fn save_skills_hint_max_bytes(connection_json: &Path, value: u32) -> Result<u32, String> {
    let v = clamp_skills_hint_max_bytes(value);
    let p = user_settings_path(connection_json);
    let mut parsed: UserSettingsFile = std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    parsed.skills_hint_max_bytes = Some(v);
    let json = serde_json::to_string_pretty(&parsed).map_err(|e| e.to_string())?;
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, json).map_err(|e| e.to_string())?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_skills_hint_respects_bounds() {
        assert_eq!(clamp_skills_hint_max_bytes(100), MIN_SKILLS_HINT_MAX_BYTES);
        assert_eq!(
            clamp_skills_hint_max_bytes(999_999),
            MAX_SKILLS_HINT_MAX_BYTES
        );
        assert_eq!(clamp_skills_hint_max_bytes(12_000), 12_000);
    }
}
