import { PENGINE_API_BASE } from "../../shared/api/config";

export type McpTool = {
  server: string;
  name: string;
  description: string | null;
};

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
