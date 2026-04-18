import { useCallback, useEffect, useRef, useState } from "react";
import {
  createCronJob,
  deleteCronJob,
  fetchCronJobs,
  setCronJobEnabled,
  testCronJob,
  updateCronJob,
} from "../api";
import { fetchSkills } from "../../skills/api";
import type { Skill } from "../../skills/types";
import { type CronDraft, type CronJob, type CronTestResponse, type Schedule } from "../types";
import { CronDailyLocalTimePicker } from "./CronDailyLocalTimePicker";
import { CronFormPinnedSkills } from "./CronFormPinnedSkills";
import { CronJobCard } from "./CronJobCard";

const MINUTES_PRESETS = [10, 30, 60, 180, 360, 720, 1440] as const;

function emptyDraft(): CronDraft {
  return {
    name: "",
    instruction: "",
    condition: "",
    skill_slugs: [],
    schedule: { kind: "every_minutes", minutes: 60 },
    enabled: true,
  };
}

function jobToDraft(job: CronJob): CronDraft {
  return {
    name: job.name,
    instruction: job.instruction,
    condition: job.condition,
    skill_slugs: job.skill_slugs ?? [],
    schedule: job.schedule,
    enabled: job.enabled,
  };
}

function normalizeCronJob(job: CronJob): CronJob {
  return {
    ...job,
    skill_slugs: Array.isArray(job.skill_slugs) ? job.skill_slugs : [],
  };
}

export function CronPanel() {
  const [jobs, setJobs] = useState<CronJob[] | null>(null);
  const [lastChatId, setLastChatId] = useState<number | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draft, setDraft] = useState<CronDraft>(emptyDraft);
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, CronTestResponse>>({});
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [skillsCatalog, setSkillsCatalog] = useState<Skill[] | null>(null);

  const cancelledRef = useRef(false);

  const load = useCallback(async () => {
    const resp = await fetchCronJobs();
    if (cancelledRef.current) return;
    setLoading(false);
    if (resp) {
      setJobs(resp.jobs.map(normalizeCronJob));
      setLastChatId(resp.last_chat_id);
      setError(null);
    } else {
      setError("Could not load cron jobs");
    }
  }, []);

  useEffect(() => {
    cancelledRef.current = false;
    void load();
    return () => {
      cancelledRef.current = true;
    };
  }, [load]);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const r = await fetchSkills(8000);
      if (cancelled || !r) return;
      setSkillsCatalog(r.skills);
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const openAdd = () => {
    setShowForm(true);
    setEditingId(null);
    setDraft(emptyDraft());
    setFormError(null);
  };

  const openEdit = (job: CronJob) => {
    setShowForm(true);
    setEditingId(job.id);
    setDraft(jobToDraft(job));
    setFormError(null);
  };

  const closeForm = () => {
    setShowForm(false);
    setEditingId(null);
    setFormError(null);
  };

  const handleSave = async () => {
    const trimmedName = draft.name.trim();
    const trimmedInstruction = draft.instruction.trim();
    if (!trimmedName) {
      setFormError("Name is required");
      return;
    }
    if (!trimmedInstruction) {
      setFormError("Instruction is required");
      return;
    }
    if (draft.schedule.kind === "every_minutes") {
      const m = draft.schedule.minutes;
      if (!Number.isFinite(m) || m < 1 || m > 10080) {
        setFormError("Minutes must be between 1 and 10080");
        return;
      }
    } else {
      const { hour, minute } = draft.schedule;
      if (hour < 0 || hour > 23 || minute < 0 || minute > 59) {
        setFormError("Time must be HH:MM in 24-hour UTC");
        return;
      }
    }

    setSaving(true);
    setFormError(null);
    const payload: CronDraft = {
      ...draft,
      name: trimmedName,
      instruction: trimmedInstruction,
      condition: draft.condition.trim(),
    };
    const result = editingId
      ? await updateCronJob(editingId, payload)
      : await createCronJob(payload);
    setSaving(false);
    if (!result.ok) {
      setFormError(result.error ?? "Could not save cron job");
      return;
    }
    setNotice(editingId ? "Cron job updated" : "Cron job created");
    closeForm();
    await load();
  };

  const handleDelete = async (job: CronJob) => {
    if (!window.confirm(`Delete cron job "${job.name}"?`)) return;
    setDeletingId(job.id);
    setError(null);
    const result = await deleteCronJob(job.id);
    setDeletingId(null);
    if (!result.ok) {
      setError(result.error ?? "Could not delete cron job");
      return;
    }
    setNotice(`Deleted "${job.name}"`);
    setTestResults((prev) => {
      const next = { ...prev };
      delete next[job.id];
      return next;
    });
    await load();
  };

  const handleToggle = async (job: CronJob) => {
    const next = !job.enabled;
    setTogglingId(job.id);
    setJobs((prev) =>
      prev ? prev.map((j) => (j.id === job.id ? { ...j, enabled: next } : j)) : prev,
    );
    const result = await setCronJobEnabled(job.id, next);
    setTogglingId(null);
    if (!result.ok) {
      setError(result.error ?? "Could not update cron job");
      void load();
    }
  };

  const handleTest = async (job: CronJob) => {
    setTestingId(job.id);
    setError(null);
    const result = await testCronJob(job.id);
    setTestingId(null);
    if (!result.ok || !result.result) {
      setError(result.error ?? "Test run failed");
      return;
    }
    setTestResults((prev) => ({ ...prev, [job.id]: result.result! }));
  };

  const setScheduleKind = (kind: Schedule["kind"]) => {
    if (kind === draft.schedule.kind) return;
    setDraft((prev) => ({
      ...prev,
      schedule:
        kind === "every_minutes"
          ? { kind: "every_minutes", minutes: 60 }
          : { kind: "daily_at", hour: 9, minute: 0 },
    }));
  };

  const setEveryMinutes = (minutes: number) => {
    setDraft((prev) => ({ ...prev, schedule: { kind: "every_minutes", minutes } }));
  };

  const setDailyAt = (hour: number, minute: number) => {
    setDraft((prev) => ({ ...prev, schedule: { kind: "daily_at", hour, minute } }));
  };

  return (
    <div className="panel p-4 sm:p-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="mono-label">Cron Jobs</p>
        <button
          type="button"
          onClick={showForm ? closeForm : openAdd}
          className="rounded-lg border border-emerald-300/20 bg-emerald-300/10 px-3 py-1 font-mono text-[11px] text-emerald-300 transition hover:bg-emerald-300/20"
        >
          {showForm ? "Cancel" : "Add cron job"}
        </button>
      </div>

      <p className="mt-2 font-mono text-[10px] text-white/40">
        Delivery target:{" "}
        {lastChatId == null ? (
          <span className="text-amber-200/80">not set — send any message to the bot first</span>
        ) : (
          <span className="text-white/70">chat {lastChatId}</span>
        )}
      </p>
      <p className="mt-1 max-w-xl font-mono text-[10px] text-white/38">
        <span className="text-white/50">Test</span> runs the agent and, if there is a message to
        send, delivers it to the chat above (same as a scheduled run). Requires a known chat and a
        connected bot.
      </p>

      {notice && (
        <p className="mt-3 font-mono text-[11px] text-fuchsia-200/90" role="status">
          {notice}
        </p>
      )}
      {error && (
        <p className="mt-3 font-mono text-[11px] text-rose-300" role="alert">
          {error}
        </p>
      )}

      {showForm && (
        <div className="mt-4 rounded-xl border border-white/10 bg-white/5 p-3">
          <div className="grid gap-3">
            <label className="grid gap-1">
              <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
                Name
              </span>
              <input
                type="text"
                value={draft.name}
                onChange={(e) => setDraft((p) => ({ ...p, name: e.target.value }))}
                placeholder="e.g. Morning weather check"
                className="rounded-md border border-white/10 bg-black/30 px-2 py-1 font-mono text-[12px] text-white outline-none focus:border-cyan-300/40"
              />
            </label>

            <label className="grid gap-1">
              <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
                Instruction
              </span>
              <textarea
                value={draft.instruction}
                onChange={(e) => setDraft((p) => ({ ...p, instruction: e.target.value }))}
                placeholder="What should the bot do when this runs?"
                rows={3}
                className="rounded-md border border-white/10 bg-black/30 px-2 py-1 font-mono text-[12px] text-white outline-none focus:border-cyan-300/40"
              />
            </label>

            <label className="grid gap-1">
              <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
                Condition (optional)
              </span>
              <textarea
                value={draft.condition}
                onChange={(e) => setDraft((p) => ({ ...p, condition: e.target.value }))}
                placeholder="Only send a message if…  (leave blank to always send)"
                rows={2}
                className="rounded-md border border-white/10 bg-black/30 px-2 py-1 font-mono text-[12px] text-white outline-none focus:border-cyan-300/40"
              />
            </label>

            {skillsCatalog && skillsCatalog.some((s) => s.enabled) && (
              <CronFormPinnedSkills
                skillsCatalog={skillsCatalog}
                skillSlugs={draft.skill_slugs}
                onSkillSlugsChange={(skill_slugs) => setDraft((p) => ({ ...p, skill_slugs }))}
              />
            )}

            <div className="grid gap-2">
              <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
                Schedule
              </span>
              <div className="flex flex-wrap gap-2">
                <label className="inline-flex items-center gap-1.5">
                  <input
                    type="radio"
                    name="cron-schedule-kind"
                    checked={draft.schedule.kind === "every_minutes"}
                    onChange={() => setScheduleKind("every_minutes")}
                  />
                  <span className="font-mono text-[11px] text-white/80">Every N minutes</span>
                </label>
                <label className="inline-flex items-center gap-1.5">
                  <input
                    type="radio"
                    name="cron-schedule-kind"
                    checked={draft.schedule.kind === "daily_at"}
                    onChange={() => setScheduleKind("daily_at")}
                  />
                  <span className="font-mono text-[11px] text-white/80">Daily at (local time)</span>
                </label>
              </div>

              {draft.schedule.kind === "every_minutes" && (
                <div className="flex flex-wrap items-center gap-1.5">
                  {MINUTES_PRESETS.map((m) => (
                    <button
                      key={m}
                      type="button"
                      onClick={() => setEveryMinutes(m)}
                      className={`rounded-md border px-2 py-0.5 font-mono text-[10px] transition ${
                        draft.schedule.kind === "every_minutes" && draft.schedule.minutes === m
                          ? "border-cyan-300/40 bg-cyan-300/15 text-cyan-100"
                          : "border-white/10 bg-white/5 text-white/60 hover:bg-white/10"
                      }`}
                    >
                      {m < 60 ? `${m}m` : m === 60 ? "1h" : m === 1440 ? "1d" : `${m / 60}h`}
                    </button>
                  ))}
                  <input
                    type="number"
                    min={1}
                    max={10080}
                    value={draft.schedule.minutes}
                    onChange={(e) => {
                      const v = Number.parseInt(e.target.value, 10);
                      if (Number.isFinite(v)) setEveryMinutes(v);
                    }}
                    className="w-20 rounded-md border border-white/10 bg-black/30 px-2 py-0.5 font-mono text-[11px] text-white outline-none focus:border-cyan-300/40"
                  />
                  <span className="font-mono text-[10px] text-white/40">min</span>
                </div>
              )}

              {draft.schedule.kind === "daily_at" && (
                <CronDailyLocalTimePicker
                  hour={draft.schedule.hour}
                  minute={draft.schedule.minute}
                  onChange={setDailyAt}
                />
              )}
            </div>

            <label className="inline-flex items-center gap-1.5">
              <input
                type="checkbox"
                checked={draft.enabled}
                onChange={(e) => setDraft((p) => ({ ...p, enabled: e.target.checked }))}
              />
              <span className="font-mono text-[11px] text-white/80">Enabled</span>
            </label>

            {formError && (
              <p className="font-mono text-[11px] text-rose-300" role="alert">
                {formError}
              </p>
            )}

            <div className="flex items-center gap-2">
              <button
                type="button"
                disabled={saving}
                onClick={() => void handleSave()}
                className="rounded-md border border-cyan-300/25 bg-cyan-300/10 px-3 py-1 font-mono text-[11px] text-cyan-100 transition hover:bg-cyan-300/15 disabled:pointer-events-none disabled:opacity-35"
              >
                {saving ? "Saving…" : editingId ? "Save changes" : "Create"}
              </button>
              <button
                type="button"
                onClick={closeForm}
                className="rounded-md border border-white/12 bg-transparent px-3 py-1 font-mono text-[11px] text-white/60 transition hover:border-white/20 hover:text-white/80"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}

      {loading && jobs === null && (
        <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
          Loading…
        </p>
      )}

      {jobs !== null && jobs.length === 0 && <p className="mt-3 subtle-copy">No cron jobs yet.</p>}

      {jobs !== null && jobs.length > 0 && (
        <div className="mt-4 grid gap-2">
          {jobs.map((job) => {
            const testResult = testResults[job.id];
            const busy = togglingId === job.id || testingId === job.id || deletingId === job.id;
            return (
              <CronJobCard
                key={job.id}
                job={job}
                testResult={testResult}
                busy={busy}
                toggleDisabled={togglingId === job.id}
                testing={testingId === job.id}
                deleting={deletingId === job.id}
                skillsCatalog={skillsCatalog}
                onToggle={() => void handleToggle(job)}
                onTest={() => void handleTest(job)}
                onEdit={() => openEdit(job)}
                onDelete={() => void handleDelete(job)}
              />
            );
          })}
        </div>
      )}
    </div>
  );
}
