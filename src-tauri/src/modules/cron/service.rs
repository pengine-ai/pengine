use super::types::{CronJob, Schedule};
use chrono::{DateTime, Duration, Local, LocalResult, TimeZone, Utc};

/// Sentinel the model is asked to emit when a job's condition isn't met.
/// Kept lowercase + punctuated so common phrasings ("no message", "NO-MESSAGE")
/// also suppress delivery.
pub const NO_MESSAGE_SENTINEL: &str = "<no-message>";

pub fn new_job_id() -> String {
    let ts = Utc::now().timestamp_millis();
    let rand = fastrand::u64(..);
    format!("job-{ts:x}{rand:012x}")
}

pub fn validate(name: &str, instruction: &str, schedule: &Schedule) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("name is required".into());
    }
    if instruction.trim().is_empty() {
        return Err("instruction is required".into());
    }
    match schedule {
        Schedule::EveryMinutes { minutes } => {
            if *minutes < 1 {
                return Err("minutes must be at least 1".into());
            }
            // one week upper bound keeps scheduler math bounded
            if *minutes > 60 * 24 * 7 {
                return Err("minutes must be at most 10080 (one week)".into());
            }
        }
        Schedule::DailyAt { hour, minute } => {
            if *hour >= 24 {
                return Err("hour must be 0-23".into());
            }
            if *minute >= 60 {
                return Err("minute must be 0-59".into());
            }
        }
    }
    Ok(())
}

pub fn compose_prompt(job: &CronJob) -> String {
    let instruction = job.instruction.trim();
    let condition = job.condition.trim();
    if condition.is_empty() {
        format!(
            "[Scheduled task '{name}'] {instruction}\n\nReply with a concise message for the user.",
            name = job.name
        )
    } else {
        format!(
            "[Scheduled task '{name}'] {instruction}\n\nCondition for sending a message to the user: {condition}\n\nIf the condition is NOT satisfied, reply with exactly \"{NO_MESSAGE_SENTINEL}\" and nothing else. Otherwise, respond with a short message the user will receive.",
            name = job.name
        )
    }
}

/// True when the scheduler should skip delivery for this reply.
pub fn is_no_message_reply(text: &str) -> bool {
    let t = text.trim().trim_matches(|c: char| c == '"' || c == '\'');
    if t.is_empty() {
        return true;
    }
    let t_lower = t.to_ascii_lowercase();
    t_lower == NO_MESSAGE_SENTINEL
        || t_lower == "no-message"
        || t_lower == "no message"
        || t_lower == "<no_message>"
        || t_lower == "no_message"
}

/// Next time this schedule should fire, given the last time it ran.
pub fn next_due(
    schedule: &Schedule,
    last_run: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> DateTime<Utc> {
    match schedule {
        Schedule::EveryMinutes { minutes } => match last_run {
            Some(t) => t + Duration::minutes(*minutes as i64),
            None => now,
        },
        Schedule::DailyAt { hour, minute } => {
            let h = *hour as u32;
            let m = *minute as u32;
            let now_local = now.with_timezone(&Local);
            let today_local = now_local.date_naive();
            let today_due_naive = today_local
                .and_hms_opt(h, m, 0)
                .expect("valid hours from validate()");
            let today_due_local = match Local.from_local_datetime(&today_due_naive) {
                LocalResult::Single(dt) => dt,
                LocalResult::Ambiguous(earliest, _) => earliest,
                LocalResult::None => {
                    // Non-existent local time (DST gap): advance one hour and retry once.
                    let adjusted = today_due_naive + Duration::hours(1);
                    Local
                        .from_local_datetime(&adjusted)
                        .single()
                        .expect("adjusted daily_at time should exist in local TZ")
                }
            };
            let today_due = today_due_local.with_timezone(&Utc);
            let already_ran_today = last_run.is_some_and(|t| t >= today_due);
            if already_ran_today {
                let next_naive = today_local + Duration::days(1);
                let next_due_naive = next_naive
                    .and_hms_opt(h, m, 0)
                    .expect("valid hours from validate()");
                let next_due_local = match Local.from_local_datetime(&next_due_naive) {
                    LocalResult::Single(dt) => dt,
                    LocalResult::Ambiguous(earliest, _) => earliest,
                    LocalResult::None => {
                        let adjusted = next_due_naive + Duration::hours(1);
                        Local
                            .from_local_datetime(&adjusted)
                            .single()
                            .expect("adjusted next daily_at should exist")
                    }
                };
                next_due_local.with_timezone(&Utc)
            } else {
                today_due
            }
        }
    }
}

pub fn is_due(schedule: &Schedule, last_run: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    now >= next_due(schedule, last_run, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Once;

    static UTC_TZ: Once = Once::new();

    /// `DailyAt` tests assume local time == UTC (`TZ=UTC`).
    fn ensure_daily_tests_use_utc() {
        UTC_TZ.call_once(|| {
            #[cfg(unix)]
            std::env::set_var("TZ", "UTC");
        });
    }

    fn dt(y: i32, m: u32, d: u32, h: u32, mi: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, h, mi, 0).unwrap()
    }

    #[test]
    fn every_minutes_fires_on_first_tick() {
        let s = Schedule::EveryMinutes { minutes: 10 };
        assert!(is_due(&s, None, dt(2026, 4, 18, 12, 0)));
    }

    #[test]
    fn every_minutes_waits_interval_when_created_at_is_reference() {
        let s = Schedule::EveryMinutes { minutes: 10 };
        let created = dt(2026, 4, 18, 12, 0);
        assert!(!is_due(&s, Some(created), dt(2026, 4, 18, 12, 5)));
        assert!(is_due(&s, Some(created), dt(2026, 4, 18, 12, 10)));
    }

    #[test]
    fn every_minutes_respects_interval() {
        let s = Schedule::EveryMinutes { minutes: 10 };
        let last = dt(2026, 4, 18, 12, 0);
        assert!(!is_due(&s, Some(last), dt(2026, 4, 18, 12, 5)));
        assert!(is_due(&s, Some(last), dt(2026, 4, 18, 12, 10)));
    }

    #[test]
    fn daily_at_waits_until_target_time() {
        ensure_daily_tests_use_utc();
        let s = Schedule::DailyAt { hour: 9, minute: 0 };
        assert!(!is_due(&s, None, dt(2026, 4, 18, 8, 0)));
        assert!(is_due(&s, None, dt(2026, 4, 18, 9, 0)));
    }

    #[test]
    fn daily_at_rolls_to_tomorrow_after_firing() {
        ensure_daily_tests_use_utc();
        let s = Schedule::DailyAt { hour: 9, minute: 0 };
        let last = dt(2026, 4, 18, 9, 0);
        assert!(!is_due(&s, Some(last), dt(2026, 4, 18, 23, 59)));
        assert!(is_due(&s, Some(last), dt(2026, 4, 19, 9, 0)));
    }

    #[test]
    fn sentinel_matches_common_phrasings() {
        assert!(is_no_message_reply("<no-message>"));
        assert!(is_no_message_reply("  \"<no-message>\"  "));
        assert!(is_no_message_reply("no-message"));
        assert!(is_no_message_reply("NO_MESSAGE"));
        assert!(!is_no_message_reply("price is $46000"));
    }

    #[test]
    fn compose_prompt_with_condition_mentions_sentinel() {
        let job = CronJob {
            id: "x".into(),
            name: "btc".into(),
            instruction: "fetch bitcoin price".into(),
            condition: "price above 45000".into(),
            skill_slugs: vec![],
            schedule: Schedule::EveryMinutes { minutes: 60 },
            enabled: true,
            created_at: Utc::now(),
            last_run_at: None,
        };
        let out = compose_prompt(&job);
        assert!(out.contains("fetch bitcoin price"));
        assert!(out.contains("price above 45000"));
        assert!(out.contains(NO_MESSAGE_SENTINEL));
    }

    #[test]
    fn validate_rejects_zero_minutes() {
        assert!(validate("n", "do thing", &Schedule::EveryMinutes { minutes: 0 }).is_err());
    }

    #[test]
    fn validate_rejects_bad_hour() {
        assert!(validate(
            "n",
            "do thing",
            &Schedule::DailyAt {
                hour: 24,
                minute: 0
            }
        )
        .is_err());
    }
}
