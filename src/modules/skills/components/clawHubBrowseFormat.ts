import type { ClawHubSkill } from "../types";

export const CLAWHUB_PLUGINS_CATALOG_ESTIMATE = 55_561;

export function formatClawHubUpdated(ms: number): string {
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

export function clawHubSkillDetailUrl(slug: string): string {
  return `https://clawhub.ai/openclaw/${encodeURIComponent(slug)}`;
}

/** Match ClawHub list-style compact numbers (e.g. 39.1k). */
function fmtCompact(n: number): string {
  if (!Number.isFinite(n)) return "—";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  if (n >= 100_000) return `${Math.round(n / 1000)}k`;
  if (n >= 1000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k`;
  return String(n);
}

export function formatClawHubStatsPrimary(entry: ClawHubSkill): string {
  const parts: string[] = [];
  if (entry.downloads != null) parts.push(fmtCompact(entry.downloads));
  if (entry.stars != null) parts.push(`★ ${entry.stars}`);
  if (entry.versionCount != null) parts.push(`${entry.versionCount} v`);
  return parts.length > 0 ? parts.join(" · ") : "—";
}

export function formatClawHubStatsInstalls(entry: ClawHubSkill): string | null {
  if (entry.installsCurrent == null && entry.installsAllTime == null) return null;
  const bits: string[] = [];
  if (entry.installsCurrent != null) bits.push(`${fmtCompact(entry.installsCurrent)} cur`);
  if (entry.installsAllTime != null) bits.push(`${fmtCompact(entry.installsAllTime)} all`);
  return bits.join(" · ");
}
