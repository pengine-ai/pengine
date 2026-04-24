import { invoke } from "@tauri-apps/api/core";
import { makeTimeoutSignal, PENGINE_API_BASE } from "../../../shared/api/config";
import type { AuditLogFileInfo, PengineHealth } from "../types";

/** Loopback HTTP API paths (Tauri `connection_server`). */
export const PENGINE = {
  connect: `${PENGINE_API_BASE}/v1/connect`,
  health: `${PENGINE_API_BASE}/v1/health`,
  logs: `${PENGINE_API_BASE}/v1/logs`,
} as const;

export async function auditListFiles(): Promise<AuditLogFileInfo[] | null> {
  try {
    return await invoke<AuditLogFileInfo[]>("audit_list_files");
  } catch {
    return null;
  }
}

export async function auditReadFile(date: string): Promise<string | null> {
  try {
    return await invoke<string>("audit_read_file", { date });
  } catch {
    return null;
  }
}

export async function auditDeleteFile(date: string): Promise<boolean> {
  try {
    await invoke("audit_delete_file", { date });
    return true;
  } catch {
    return false;
  }
}

/** GET `/v1/health`; JSON on 200, otherwise `null` (offline / error). */
export async function getPengineHealth(timeoutMs: number): Promise<PengineHealth | null> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(PENGINE.health, { signal });
    if (!resp.ok) return null;
    return (await resp.json()) as PengineHealth;
  } catch {
    return null;
  } finally {
    cleanup();
  }
}

export async function postConnect(botToken: string) {
  const { signal, cleanup } = makeTimeoutSignal(15_000);
  try {
    const resp = await fetch(PENGINE.connect, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ bot_token: botToken.trim() }),
      signal,
    });
    const data = (await resp.json()) as { bot_id?: string; bot_username?: string; error?: string };
    return { ok: resp.ok, data };
  } finally {
    cleanup();
  }
}

export async function deleteConnect() {
  const { signal, cleanup } = makeTimeoutSignal(5000);
  try {
    const resp = await fetch(PENGINE.connect, {
      method: "DELETE",
      signal,
    });
    if (!resp.ok) {
      const detail = await resp.text().catch(() => "");
      throw new Error(detail || `Disconnect failed (HTTP ${resp.status})`);
    }
  } finally {
    cleanup();
  }
}
