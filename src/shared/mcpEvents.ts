/** Dispatched when MCP registry (e.g. mcp.json) may have changed from outside the MCP panel. */
export const PENGINE_MCP_REGISTRY_CHANGED = "pengine:mcp-registry-changed";

export function notifyMcpRegistryChanged(): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new Event(PENGINE_MCP_REGISTRY_CHANGED));
}

/** Tauri event name — must match `REGISTRY_CHANGED_EVENT` in `mcp/service.rs`. */
const TAURI_REGISTRY_CHANGED = "pengine-registry-changed";

let tauriBridgeInitialized = false;
let tauriBridgeListenPromise: Promise<void> | null = null;

/**
 * Bridge backend Tauri event into the browser window event that panels already listen for.
 * Call once at app startup. Safe to call multiple times — only one listener is registered.
 */
export function initTauriRegistryBridge(): void {
  if (typeof window === "undefined") return;
  if (tauriBridgeInitialized) return;
  if (tauriBridgeListenPromise !== null) return;

  tauriBridgeListenPromise = import("@tauri-apps/api/event")
    .then(({ listen }) =>
      listen(TAURI_REGISTRY_CHANGED, () => {
        notifyMcpRegistryChanged();
      }),
    )
    .then(() => {
      tauriBridgeInitialized = true;
    })
    .catch(() => {
      tauriBridgeListenPromise = null;
      // Not running inside Tauri shell — no bridge needed.
    });
}
