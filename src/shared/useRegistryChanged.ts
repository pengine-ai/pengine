import { useEffect } from "react";
import { PENGINE_MCP_REGISTRY_CHANGED } from "./mcpEvents";

/** Re-run `callback` whenever the MCP registry changes (install, uninstall, config edit). */
export function useRegistryChanged(callback: () => void): void {
  useEffect(() => {
    window.addEventListener(PENGINE_MCP_REGISTRY_CHANGED, callback);
    return () => window.removeEventListener(PENGINE_MCP_REGISTRY_CHANGED, callback);
  }, [callback]);
}
