use super::repository;
use super::service;
use super::types::{CronFile, CronJob};
use crate::modules::bot::agent;
use crate::shared::state::AppState;
use crate::shared::text::split_by_chars;
use std::time::Duration as StdDuration;
use teloxide::prelude::*;
use teloxide::types::ChatId;

/// Wake-up cadence for the scheduler. The loop also resumes on `state.cron_notify`
/// so CRUD / enable / test operations take effect immediately instead of waiting
/// for the next tick.
const TICK_INTERVAL: StdDuration = StdDuration::from_secs(30);

/// Telegram chunk budget (mirrors bot/service.rs).
const TELEGRAM_CHUNK_BUDGET: usize = 2000;

pub async fn run(state: AppState) {
    state.emit_log("cron", "scheduler started").await;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(TICK_INTERVAL) => {}
            _ = state.cron_notify.notified() => {}
            _ = state.shutdown_notify.notified() => {
                state.emit_log("cron", "scheduler stopping").await;
                return;
            }
        }
        tick(&state).await;
    }
}

async fn tick(state: &AppState) {
    let now = chrono::Utc::now();
    let due: Vec<CronJob> = {
        let jobs = state.cron_jobs.read().await;
        jobs.iter()
            .filter(|j| {
                // For never-run jobs, `created_at` anchors the first-interval wait so a
                // brand-new "every 10 min" job fires 10 minutes after creation, not instantly.
                let reference = j.last_run_at.or(Some(j.created_at));
                j.enabled && service::is_due(&j.schedule, reference, now)
            })
            .cloned()
            .collect()
    };
    if due.is_empty() {
        return;
    }
    // Scheduled jobs have no chat to deliver to until the user messages the bot at
    // least once. Skip the expensive agent turn entirely until that happens.
    if state.last_chat_id.read().await.is_none() {
        state
            .emit_log(
                "cron",
                &format!(
                    "{} job(s) due but no Telegram chat known yet — send any message to the bot first",
                    due.len()
                ),
            )
            .await;
        return;
    }
    for job in due {
        execute_job(state, job).await;
    }
}

/// Runs a job through the agent and (when the condition is met) delivers the
/// reply to the last-known Telegram chat.
pub async fn execute_job(state: &AppState, job: CronJob) {
    state
        .emit_log("cron", &format!("running '{}' ({})", job.name, job.id))
        .await;

    let prompt = service::compose_prompt(&job);
    let skills_filter = if job.skill_slugs.is_empty() {
        None
    } else {
        Some(job.skill_slugs.as_slice())
    };
    let result = agent::run_system_turn(state, &prompt, skills_filter).await;

    if let Err(e) = mark_ran(state, &job.id).await {
        state
            .emit_log("cron", &format!("persist last_run_at failed: {e}"))
            .await;
    }

    match result {
        Ok(turn) => {
            if turn.suppress_telegram_reply {
                state
                    .emit_log(
                        "cron",
                        &format!("'{}' — reply suppressed; not sending to Telegram", job.name),
                    )
                    .await;
                return;
            }
            if turn.text.trim().is_empty() {
                state
                    .emit_log(
                        "cron",
                        &format!(
                            "'{}' — model returned an empty reply; nothing sent to Telegram (check Ollama logs / try Test in Dashboard)",
                            job.name
                        ),
                    )
                    .await;
                return;
            }
            if service::is_no_message_reply(&turn.text) {
                state
                    .emit_log(
                        "cron",
                        &format!(
                            "'{}' — scheduled output is <no-message> (condition not met); not sending to Telegram",
                            job.name
                        ),
                    )
                    .await;
                return;
            }
            if let Err(e) = send_to_last_chat(state, &turn.text).await {
                state
                    .emit_log("cron", &format!("'{}' send failed: {e}", job.name))
                    .await;
            } else {
                state
                    .emit_log("cron", &format!("'{}' sent reply", job.name))
                    .await;
            }
        }
        Err(e) => {
            state
                .emit_log("cron", &format!("'{}' agent error: {e}", job.name))
                .await;
        }
    }
}

async fn mark_ran(state: &AppState, job_id: &str) -> Result<(), String> {
    let snapshot = {
        let mut jobs = state.cron_jobs.write().await;
        if let Some(j) = jobs.iter_mut().find(|j| j.id == job_id) {
            j.last_run_at = Some(chrono::Utc::now());
        }
        jobs.clone()
    };
    let last_chat_id = *state.last_chat_id.read().await;
    let file = CronFile {
        jobs: snapshot,
        last_chat_id,
    };
    repository::save(&state.cron_path, &file)
}

pub async fn send_to_last_chat(state: &AppState, text: &str) -> Result<(), String> {
    let chat_id =
        state.last_chat_id.read().await.ok_or_else(|| {
            "no known Telegram chat — send any message to the bot first".to_string()
        })?;
    let token = {
        let g = state.connection.lock().await;
        g.as_ref()
            .ok_or_else(|| "Telegram bot is not connected".to_string())?
            .bot_token
            .clone()
    };
    let bot = Bot::new(token);
    let chunks = split_by_chars(text, TELEGRAM_CHUNK_BUDGET);
    for chunk in chunks {
        bot.send_message(ChatId(chat_id), chunk)
            .await
            .map_err(|e| format!("telegram send: {e}"))?;
    }
    Ok(())
}
