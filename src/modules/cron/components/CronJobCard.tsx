import type { Skill } from "../../skills/types";
import { formatSchedule, type CronJob, type CronTestResponse } from "../types";

function formatLastRun(iso: string | null): string {
  if (!iso) return "never";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

export type CronJobCardProps = {
  job: CronJob;
  testResult: CronTestResponse | undefined;
  busy: boolean;
  toggleDisabled: boolean;
  testing: boolean;
  deleting: boolean;
  skillsCatalog: Skill[] | null;
  onToggle: () => void;
  onTest: () => void;
  onEdit: () => void;
  onDelete: () => void;
};

export function CronJobCard({
  job,
  testResult,
  busy,
  toggleDisabled,
  testing,
  deleting,
  skillsCatalog,
  onToggle,
  onTest,
  onEdit,
  onDelete,
}: CronJobCardProps) {
  return (
    <div className="rounded-lg border border-white/10 bg-white/[0.03] p-3">
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="font-mono text-[12px] text-white">{job.name}</p>
          <p className="mt-0.5 font-mono text-[10px] text-white/50">
            {formatSchedule(job.schedule)} · last run: {formatLastRun(job.last_run_at)}
          </p>
        </div>
        <div className="flex shrink-0 flex-wrap items-center gap-1.5">
          <label className="inline-flex items-center gap-1">
            <input
              type="checkbox"
              checked={job.enabled}
              disabled={toggleDisabled}
              onChange={onToggle}
            />
            <span className="font-mono text-[10px] text-white/60">enabled</span>
          </label>
          <button
            type="button"
            disabled={busy}
            onClick={onTest}
            className="rounded-md border border-fuchsia-300/25 bg-fuchsia-300/10 px-2 py-0.5 font-mono text-[10px] text-fuchsia-200 transition hover:bg-fuchsia-300/15 disabled:pointer-events-none disabled:opacity-35"
          >
            {testing ? "Testing…" : "Test"}
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={onEdit}
            className="rounded-md border border-white/12 bg-transparent px-2 py-0.5 font-mono text-[10px] text-white/70 transition hover:border-white/20 hover:text-white"
          >
            Edit
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={onDelete}
            className="rounded-md border border-rose-300/25 bg-rose-300/10 px-2 py-0.5 font-mono text-[10px] text-rose-200 transition hover:bg-rose-300/15 disabled:pointer-events-none disabled:opacity-35"
          >
            {deleting ? "…" : "Delete"}
          </button>
        </div>
      </div>

      <p className="mt-2 whitespace-pre-wrap font-mono text-[11px] text-white/70">
        {job.instruction}
      </p>
      {job.skill_slugs.length > 0 && (
        <p className="mt-1 font-mono text-[10px] text-cyan-200/75">
          Skills: {job.skill_slugs.join(", ")}
        </p>
      )}
      {skillsCatalog &&
        job.skill_slugs.some(
          (slug) => !skillsCatalog.some((s) => s.slug === slug && s.enabled),
        ) && (
          <p className="mt-1 font-mono text-[10px] text-amber-200/85">
            Some selected skills are disabled or removed — this job may receive no skill hints until
            you fix the list.
          </p>
        )}
      {job.condition && (
        <p className="mt-1 whitespace-pre-wrap font-mono text-[10px] text-white/45">
          <span className="uppercase tracking-[0.12em] text-(--mid)">if:</span> {job.condition}
        </p>
      )}

      {testResult && (
        <div className="mt-2 rounded-md border border-white/8 bg-black/20 p-2">
          <p className="font-mono text-[9px] uppercase tracking-[0.14em] text-(--mid)">
            Test result ·{" "}
            {testResult.condition_met ? (
              <span className="text-emerald-300">condition met</span>
            ) : (
              <span className="text-amber-200/80">condition not met — no message</span>
            )}
            {testResult.condition_met && testResult.reply.trim().length > 0 && (
              <>
                {" · "}
                {testResult.telegram_sent ? (
                  <span className="text-emerald-300/90">Telegram sent</span>
                ) : testResult.telegram_error ? (
                  <span className="text-rose-300/90">
                    Telegram failed: {testResult.telegram_error}
                  </span>
                ) : (
                  <span className="text-white/45">Telegram not sent</span>
                )}
              </>
            )}
          </p>
          <p className="mt-1 whitespace-pre-wrap font-mono text-[11px] text-white/75">
            {testResult.reply || "(empty reply)"}
          </p>
        </div>
      )}
    </div>
  );
}
