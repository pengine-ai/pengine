import { fetchErrorMessage, PENGINE_API_BASE } from "../../../shared/api/config";
import type { ClawHubSkill, Skill } from "../types";

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

export async function searchClawHub(
  query: string,
  timeoutMs = 10_000,
): Promise<{ results?: ClawHubSkill[]; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const url = new URL(`${PENGINE_API_BASE}/v1/skills/clawhub`);
    url.searchParams.set("q", query);
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
