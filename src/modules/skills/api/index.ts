import { fetchErrorMessage, PENGINE_API_BASE } from "../../../shared/api/config";
import type { ClawHubPlugin, ClawHubSkill, Skill } from "../types";

export type SkillsListResponse = {
  skills: Skill[];
  custom_dir: string;
};

function makeTimeoutSignal(timeoutMs: number): { signal: AbortSignal; cleanup: () => void } {
  if (typeof AbortSignal !== "undefined" && typeof AbortSignal.timeout === "function") {
    return { signal: AbortSignal.timeout(timeoutMs), cleanup: () => {} };
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  return { signal: controller.signal, cleanup: () => clearTimeout(timer) };
}

async function parseApiError(resp: Response): Promise<string> {
  const raw = await resp.text();
  try {
    const body = JSON.parse(raw) as { error?: string };
    return body.error?.trim() || raw.trim() || `HTTP ${resp.status}`;
  } catch {
    return raw.trim() || `HTTP ${resp.status}`;
  }
}

export async function fetchSkills(timeoutMs = 5000): Promise<SkillsListResponse | null> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/skills`, { signal });
    if (!resp.ok) return null;
    return (await resp.json()) as SkillsListResponse;
  } catch {
    return null;
  } finally {
    cleanup();
  }
}

export async function addSkill(
  slug: string,
  markdown: string,
  timeoutMs = 5000,
): Promise<{ ok: boolean; skill?: Skill; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/skills`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ slug, markdown }),
      signal,
    });
    if (resp.ok) return { ok: true, skill: (await resp.json()) as Skill };
    return { ok: false, error: await parseApiError(resp) };
  } catch (e) {
    return { ok: false, error: fetchErrorMessage(e) };
  } finally {
    cleanup();
  }
}

export async function deleteSkill(
  slug: string,
  timeoutMs = 5000,
): Promise<{ ok: boolean; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/skills/${encodeURIComponent(slug)}`, {
      method: "DELETE",
      signal,
    });
    if (resp.ok) return { ok: true };
    return { ok: false, error: await parseApiError(resp) };
  } catch (e) {
    return { ok: false, error: fetchErrorMessage(e) };
  } finally {
    cleanup();
  }
}

export async function setSkillEnabled(
  slug: string,
  enabled: boolean,
  timeoutMs = 5000,
): Promise<{ ok: boolean; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/skills/${encodeURIComponent(slug)}/enabled`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled }),
      signal,
    });
    if (resp.ok) return { ok: true };
    return { ok: false, error: await parseApiError(resp) };
  } catch (e) {
    return { ok: false, error: fetchErrorMessage(e) };
  } finally {
    cleanup();
  }
}

export type ClawHubSearchFilters = {
  highlighted?: boolean;
  nonSuspicious?: boolean;
  staffPicks?: boolean;
  cleanOnly?: boolean;
  sort?: string;
  limit?: number;
  tag?: string;
  /** When true (default), backend loads each `/openclaw/{slug}` page for author + stats. */
  enrich?: boolean;
};

export async function searchClawHub(
  query: string,
  filters: ClawHubSearchFilters = {},
  timeoutMs?: number,
): Promise<{ results?: ClawHubSkill[]; error?: string }> {
  const lim = filters.limit ?? 30;
  const t = timeoutMs ?? Math.min(120_000, 14_000 + lim * 450);
  const { signal, cleanup } = makeTimeoutSignal(t);
  try {
    const url = new URL(`${PENGINE_API_BASE}/v1/skills/clawhub`);
    url.searchParams.set("q", query);
    url.searchParams.set("highlighted", String(filters.highlighted !== false));
    url.searchParams.set("nonSuspicious", String(filters.nonSuspicious !== false));
    if (filters.staffPicks) url.searchParams.set("staffPicks", "true");
    if (filters.cleanOnly) url.searchParams.set("cleanOnly", "true");
    if (filters.sort?.trim()) url.searchParams.set("sort", filters.sort.trim());
    if (filters.limit != null && filters.limit > 0)
      url.searchParams.set("limit", String(filters.limit));
    if (filters.tag?.trim()) url.searchParams.set("tag", filters.tag.trim());
    if (filters.enrich === false) url.searchParams.set("enrich", "false");
    const resp = await fetch(url.toString(), { signal });
    if (resp.ok) {
      const body = (await resp.json()) as { results: ClawHubSkill[] };
      return { results: body.results };
    }
    return { error: await parseApiError(resp) };
  } catch (e) {
    return { error: fetchErrorMessage(e) };
  } finally {
    cleanup();
  }
}

export async function searchClawHubPlugins(
  query: string,
  options: {
    limit?: number;
    cursor?: string;
    timeoutMs?: number;
  } = {},
): Promise<{ items?: ClawHubPlugin[]; nextCursor?: string | null; error?: string }> {
  const lim = options.limit ?? 30;
  const t = options.timeoutMs ?? Math.min(120_000, 18_000 + lim * 400);
  const { signal, cleanup } = makeTimeoutSignal(t);
  try {
    const url = new URL(`${PENGINE_API_BASE}/v1/skills/clawhub/plugins`);
    const q = query.trim();
    if (q) url.searchParams.set("q", q);
    url.searchParams.set("limit", String(lim));
    const c = options.cursor?.trim();
    if (c) url.searchParams.set("cursor", c);
    const resp = await fetch(url.toString(), { signal });
    if (resp.ok) {
      const body = (await resp.json()) as { items: ClawHubPlugin[]; nextCursor?: string | null };
      return { items: body.items, nextCursor: body.nextCursor ?? null };
    }
    return { error: await parseApiError(resp) };
  } catch (e) {
    return { error: fetchErrorMessage(e) };
  } finally {
    cleanup();
  }
}

export async function installClawHubSkill(
  slug: string,
  timeoutMs = 20_000,
): Promise<{ ok: boolean; skill?: Skill; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/skills/clawhub/install`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ slug }),
      signal,
    });
    if (resp.ok) return { ok: true, skill: (await resp.json()) as Skill };
    return { ok: false, error: await parseApiError(resp) };
  } catch (e) {
    return { ok: false, error: fetchErrorMessage(e) };
  } finally {
    cleanup();
  }
}
