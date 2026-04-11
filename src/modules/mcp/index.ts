import { PENGINE_API_BASE } from "../../shared/api/config";

export type McpTool = {
  server: string;
  name: string;
  description: string | null;
};

export type McpConfigInfo = {
  config_path: string;
  source: string;
  filesystem_allowed_paths: string[];
};

export type ServerEntryStdio = {
  type: "stdio";
  command: string;
  args: string[];
  env: Record<string, string>;
  direct_return: boolean;
};

export type ServerEntryNative = {
  type: "native";
  id: string;
};

export type ServerEntry = ServerEntryStdio | ServerEntryNative;

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

/** GET `/v1/mcp/config` — active `mcp.json` path and filesystem allow-list. */
export async function fetchMcpConfig(timeoutMs = 3000): Promise<McpConfigInfo | null> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/mcp/config`, {
      signal,
    });
    if (!resp.ok) return null;
    return (await resp.json()) as McpConfigInfo;
  } catch {
    return null;
  } finally {
    cleanup();
  }
}

/** PUT `/v1/mcp/filesystem` — set `workspace_roots` (File Manager bind mounts) and reload MCP. */
export async function putMcpFilesystemPaths(paths: string[], timeoutMs = 15000): Promise<boolean> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/mcp/filesystem`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ paths }),
      signal,
    });
    return resp.ok;
  } catch {
    return false;
  } finally {
    cleanup();
  }
}

/** GET `/v1/mcp/tools` — flat list of tools across all connected MCP servers. `null` = request failed. */
export async function fetchMcpTools(timeoutMs = 3000): Promise<McpTool[] | null> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/mcp/tools`, {
      signal,
    });
    if (!resp.ok) return null;
    return (await resp.json()) as McpTool[];
  } catch {
    return null;
  } finally {
    cleanup();
  }
}

/** GET `/v1/mcp/servers` — full server config map from mcp.json. */
export async function fetchMcpServers(
  timeoutMs = 5000,
): Promise<Record<string, ServerEntry> | null> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/mcp/servers`, {
      signal,
    });
    if (!resp.ok) return null;
    const body = (await resp.json()) as { servers: Record<string, ServerEntry> };
    return body.servers;
  } catch {
    return null;
  } finally {
    cleanup();
  }
}

/** PUT `/v1/mcp/servers/{name}` — create or update a server entry, then rebuild tools. */
export async function upsertMcpServer(
  name: string,
  entry: ServerEntry,
  timeoutMs = 20000,
): Promise<boolean> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/mcp/servers/${encodeURIComponent(name)}`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(entry),
      signal,
    });
    return resp.ok;
  } catch {
    return false;
  } finally {
    cleanup();
  }
}

/** DELETE `/v1/mcp/servers/{name}` — remove a server and rebuild tools. */
export async function deleteMcpServer(name: string, timeoutMs = 20000): Promise<boolean> {
  const { signal, cleanup } = makeTimeoutSignal(timeoutMs);
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/mcp/servers/${encodeURIComponent(name)}`, {
      method: "DELETE",
      signal,
    });
    return resp.ok;
  } catch {
    return false;
  } finally {
    cleanup();
  }
}
