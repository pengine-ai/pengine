/** Dispatched when MCP registry (e.g. mcp.json) may have changed from outside the MCP panel. */
export const PENGINE_MCP_REGISTRY_CHANGED = "pengine:mcp-registry-changed";

export function notifyMcpRegistryChanged(): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event(PENGINE_MCP_REGISTRY_CHANGED));
}
