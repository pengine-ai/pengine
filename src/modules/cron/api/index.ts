import { fetchErrorMessage, PENGINE_API_BASE } from "../../../shared/api/config";
import type { CronDraft, CronJob, CronListResponse, CronTestResponse } from "../types";

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

export async function fetchCronJobs(timeoutMs = 5000): Promise<CronListResponse | null> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/cron`, { signal });
    if (!resp.ok) return null;
    return (await resp.json()) as CronListResponse;
  } catch {
    return null;
  } finally {
    cleanup();
  }
}

export async function createCronJob(
  draft: CronDraft,
  timeoutMs = 5000,
): Promise<{ ok: boolean; job?: CronJob; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/cron`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(draft),
      signal,
    });
    if (resp.ok) return { ok: true, job: (await resp.json()) as CronJob };
    return { ok: false, error: await parseApiError(resp) };
  } catch (e) {
    return { ok: false, error: fetchErrorMessage(e) };
  } finally {
    cleanup();
  }
}

export async function updateCronJob(
  id: string,
  draft: CronDraft,
  timeoutMs = 5000,
): Promise<{ ok: boolean; job?: CronJob; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/cron/${encodeURIComponent(id)}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(draft),
      signal,
    });
    if (resp.ok) return { ok: true, job: (await resp.json()) as CronJob };
    return { ok: false, error: await parseApiError(resp) };
  } catch (e) {
    return { ok: false, error: fetchErrorMessage(e) };
  } finally {
    cleanup();
  }
}

export async function deleteCronJob(
  id: string,
  timeoutMs = 5000,
): Promise<{ ok: boolean; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/cron/${encodeURIComponent(id)}`, {
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

export async function setCronJobEnabled(
  id: string,
  enabled: boolean,
  timeoutMs = 5000,
): Promise<{ ok: boolean; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/cron/${encodeURIComponent(id)}/enabled`, {
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

export async function testCronJob(
  id: string,
  timeoutMs = 120_000,
): Promise<{ ok: boolean; result?: CronTestResponse; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/cron/${encodeURIComponent(id)}/test`, {
      method: "POST",
      signal,
    });
    if (resp.ok) return { ok: true, result: (await resp.json()) as CronTestResponse };
    return { ok: false, error: await parseApiError(resp) };
  } catch (e) {
    return { ok: false, error: fetchErrorMessage(e) };
  } finally {
    cleanup();
  }
}
