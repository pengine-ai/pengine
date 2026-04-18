export type Schedule =
  | { kind: "every_minutes"; minutes: number }
  | { kind: "daily_at"; hour: number; minute: number };

export type CronJob = {
  id: string;
  name: string;
  instruction: string;
  condition: string;
  /** Empty = all enabled skills at runtime */
  skill_slugs: string[];
  schedule: Schedule;
  enabled: boolean;
  created_at: string;
  last_run_at: string | null;
};

export type CronListResponse = {
  jobs: CronJob[];
  last_chat_id: number | null;
};

export type CronTestResponse = {
  reply: string;
  condition_met: boolean;
  /** Same reply posted to the last-known Telegram chat when applicable */
  telegram_sent?: boolean;
  telegram_error?: string | null;
};

export type CronDraft = {
  name: string;
  instruction: string;
  condition: string;
  skill_slugs: string[];
  schedule: Schedule;
  enabled: boolean;
};

export function formatSchedule(s: Schedule): string {
  if (s.kind === "every_minutes") {
    const m = s.minutes;
    if (m % 60 === 0 && m >= 60) {
      const h = m / 60;
      return h === 1 ? "every hour" : `every ${h} hours`;
    }
    return `every ${m} min`;
  }
  const hh = String(s.hour).padStart(2, "0");
  const mm = String(s.minute).padStart(2, "0");
  try {
    const tz = Intl.DateTimeFormat().resolvedOptions().timeZone;
    return `daily at ${hh}:${mm} (${tz})`;
  } catch {
    return `daily at ${hh}:${mm} local`;
  }
}
