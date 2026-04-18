import { PENGINE_API_BASE } from "../../../shared/api/config";

const SETTINGS_URL = `${PENGINE_API_BASE}/v1/settings`;

export type UserSettings = {
  skills_hint_max_bytes: number;
  skills_hint_max_bytes_min: number;
  skills_hint_max_bytes_max: number;
  skills_hint_max_bytes_default: number;
};

export async function fetchUserSettings(timeoutMs: number): Promise<UserSettings | null> {
  try {
    const resp = await fetch(SETTINGS_URL, { signal: AbortSignal.timeout(timeoutMs) });
    if (!resp.ok) return null;
    return (await resp.json()) as UserSettings;
  } catch {
    return null;
  }
}

export async function putUserSettings(
  skills_hint_max_bytes: number,
): Promise<{ ok: true; settings: UserSettings } | { ok: false; error: string }> {
  try {
    const resp = await fetch(SETTINGS_URL, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ skills_hint_max_bytes }),
      signal: AbortSignal.timeout(8000),
    });
    const data = (await resp.json()) as UserSettings & { error?: string };
    if (!resp.ok) {
      return { ok: false, error: data.error ?? `HTTP ${resp.status}` };
    }
    return { ok: true, settings: data as UserSettings };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { ok: false, error: message };
  }
}
