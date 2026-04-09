import { PENGINE_API_BASE } from "../../../shared/api/config";
import type { PengineHealth } from "../types";

/** Loopback HTTP API paths (Tauri `connection_server`). */
export const PENGINE = {
  connect: `${PENGINE_API_BASE}/v1/connect`,
  health: `${PENGINE_API_BASE}/v1/health`,
  logs: `${PENGINE_API_BASE}/v1/logs`,
} as const;

/** GET `/v1/health`; JSON on 200, otherwise `null` (offline / error). */
export async function getPengineHealth(timeoutMs: number): Promise<PengineHealth | null> {
  try {
    const resp = await fetch(PENGINE.health, { signal: AbortSignal.timeout(timeoutMs) });
    if (!resp.ok) return null;
    return (await resp.json()) as PengineHealth;
  } catch {
    return null;
  }
}

export async function postConnect(botToken: string) {
  const resp = await fetch(PENGINE.connect, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ bot_token: botToken.trim() }),
    signal: AbortSignal.timeout(15_000),
  });
  const data = (await resp.json()) as { bot_id?: string; bot_username?: string; error?: string };
  return { ok: resp.ok, data };
}

export async function deleteConnect() {
  const resp = await fetch(PENGINE.connect, {
    method: "DELETE",
    signal: AbortSignal.timeout(5000),
  });
  if (!resp.ok) {
    const detail = await resp.text().catch(() => "");
    throw new Error(detail || `Disconnect failed (HTTP ${resp.status})`);
  }
}
