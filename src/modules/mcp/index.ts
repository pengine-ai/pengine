import { PENGINE_API_BASE } from "../../shared/api/config";

export type McpTool = {
  server: string;
  name: string;
  description: string | null;
};

export type McpConfigInfo = {
  config_path: string;
  source: string;
  filesystem_allowed_path: string | null;
};

/** GET `/v1/mcp/config` — active `mcp.json` path and filesystem allow-list. */
export async function fetchMcpConfig(timeoutMs = 3000): Promise<McpConfigInfo | null> {
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/mcp/config`, {
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (!resp.ok) return null;
    return (await resp.json()) as McpConfigInfo;
  } catch {
    return null;
  }
}

/** PUT `/v1/mcp/filesystem` — set allowed folder for the `filesystem` stdio server and reload tools. */
export async function putMcpFilesystemPath(path: string, timeoutMs = 15000): Promise<boolean> {
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/mcp/filesystem`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path }),
      signal: AbortSignal.timeout(timeoutMs),
    });
    return resp.ok;
  } catch {
    return false;
  }
}

/** GET `/v1/mcp/tools` — flat list of tools across all connected MCP servers. */
export async function fetchMcpTools(timeoutMs = 3000): Promise<McpTool[]> {
  try {
    const resp = await fetch(`${PENGINE_API_BASE}/v1/mcp/tools`, {
      signal: AbortSignal.timeout(timeoutMs),
    });
    if (!resp.ok) return [];
    return (await resp.json()) as McpTool[];
  } catch {
    return [];
  }
}
