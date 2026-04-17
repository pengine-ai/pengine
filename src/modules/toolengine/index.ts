import { fetchErrorMessage, PENGINE_API_BASE } from "../../shared/api/config";

export type RuntimeStatus = {
  available: boolean;
  kind?: "podman" | "docker";
  version?: string;
  rootless?: boolean;
};

export type CatalogToolCommand = {
  name: string;
  description: string;
};

export type PrivateFolderConfig = {
  container_path: string;
  file_env_var: string;
  file_extension: string;
};

export type CatalogTool = {
  id: string;
  name: string;
  version: string;
  description: string;
  installed: boolean;
  commands: CatalogToolCommand[];
  private_folder?: PrivateFolderConfig | null;
  private_host_path?: string | null;
  /** When true, the runtime adds `--ignore-robots-txt` for this catalog tool (Fetch). Default false in catalog. */
  ignore_robots_txt?: boolean;
  /** Reserved for future per-host policy; informational in the UI today. */
  robots_ignore_allowlist?: string[];
};

function makeTimeoutSignal(timeoutMs: number): { signal: AbortSignal; cleanup: () => void } {
  if (typeof AbortSignal !== "undefined" && typeof AbortSignal.timeout === "function") {
    return { signal: AbortSignal.timeout(timeoutMs), cleanup: () => {} };
  }
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  return {
    signal: controller.signal,
    cleanup: () => clearTimeout(timer),
  };
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function parseApiError(resp: Response): Promise<string> {
  const raw = await resp.text();
  let message = `Request failed (HTTP ${resp.status})`;
  try {
    const body = JSON.parse(raw) as { error?: string };
    message = body.error?.trim() || raw.trim();
  } catch {
    message = raw.trim() || message;
  }
  if (!message) {
    message = `Request failed (HTTP ${resp.status})`;
  }
  return message;
}

async function fetchOkWithRetry(
  url: string,
  init: RequestInit | undefined,
  timeoutMs: number,
  attempts = 6,
  delayMs = 250,
): Promise<Response | null> {
  for (let i = 0; i < attempts; i++) {
    const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
    try {
      const resp = await fetch(url, { ...init, signal });
      cleanup();
      if (resp.ok) return resp;
    } catch {
      cleanup();
    }
    if (i + 1 < attempts) await sleep(delayMs);
  }
  return null;
}

export async function fetchRuntimeStatus(timeoutMs = 3000): Promise<RuntimeStatus | null> {
  const resp = await fetchOkWithRetry(
    `${PENGINE_API_BASE}/v1/toolengine/runtime`,
    undefined,
    timeoutMs,
  );
  if (!resp) return null;
  try {
    return (await resp.json()) as RuntimeStatus;
  } catch {
    return null;
  }
}

export async function fetchToolCatalog(timeoutMs = 5000): Promise<CatalogTool[] | null> {
  const resp = await fetchOkWithRetry(
    `${PENGINE_API_BASE}/v1/toolengine/catalog`,
    undefined,
    timeoutMs,
  );
  if (!resp) return null;
  try {
    const body = (await resp.json()) as { tools: CatalogTool[] };
    return body.tools;
  } catch {
    return null;
  }
}

export async function installTool(
  toolId: string,
  timeoutMs = 900_000,
): Promise<{ ok: boolean; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/toolengine/install`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ tool_id: toolId }),
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

export async function putToolPrivateFolder(
  toolId: string,
  path: string,
  timeoutMs = 120_000,
): Promise<{ ok: boolean; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/toolengine/private-folder`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ tool_id: toolId, path }),
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

export async function uninstallTool(
  toolId: string,
  timeoutMs = 120_000,
): Promise<{ ok: boolean; error?: string }> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/toolengine/uninstall`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ tool_id: toolId }),
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
