import { useCallback, useEffect, useRef, useState } from "react";
import { notifyMcpRegistryChanged } from "../../../shared/mcpEvents";
import { PENGINE_API_BASE } from "../../../shared/api/config";
import { useRegistryChanged } from "../../../shared/useRegistryChanged";
import {
  fetchRuntimeStatus,
  fetchToolCatalog,
  installTool,
  putToolPassthroughEnv,
  uninstallTool,
  type CatalogTool,
  type RuntimeStatus,
} from "..";
import { ToolEngineCatalogToolCard } from "./ToolEngineCatalogToolCard";

export function ToolEnginePanel() {
  const [runtime, setRuntime] = useState<RuntimeStatus | null>(null);
  const [catalog, setCatalog] = useState<CatalogTool[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [catalogError, setCatalogError] = useState<string | null>(null);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [actionError, setActionError] = useState<string | null>(null);
  const [busyTool, setBusyTool] = useState<string | null>(null);
  const [busyKind, setBusyKind] = useState<"install" | "uninstall" | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [progressLines, setProgressLines] = useState<string[]>([]);
  const busyToolRef = useRef<string | null>(null);
  const [passthroughValues, setPassthroughValues] = useState<
    Record<string, Record<string, string>>
  >({});
  /** Per-key "replace secret" mode (only used when the key is already saved). */
  const [passthroughReplacing, setPassthroughReplacing] = useState<
    Record<string, Record<string, boolean>>
  >({});
  const [passthroughSavingId, setPassthroughSavingId] = useState<string | null>(null);

  const cancelledRef = useRef(false);
  const seqRef = useRef(0);

  useEffect(() => {
    if (!catalog) return;
    setPassthroughValues((prev) => {
      const next = { ...prev };
      for (const t of catalog) {
        const keys = t.passthrough_env;
        if (!keys?.length) continue;
        const cur = { ...(next[t.id] ?? {}) };
        for (const k of keys) {
          if (cur[k] === undefined) cur[k] = "";
        }
        next[t.id] = cur;
      }
      return next;
    });
  }, [catalog]);

  // Listen for toolengine log events to show pull progress.
  useEffect(() => {
    let cancelled = false;
    let unlistenTauri: (() => void) | null = null;
    let es: EventSource | null = null;

    const handleLog = (kind: string, message: string) => {
      if (cancelled || kind !== "toolengine") return;
      // Only show lines tagged with the currently busy tool, e.g. "[pengine/file-manager] pulling…"
      const currentBusy = busyToolRef.current;
      if (currentBusy && message.startsWith(`[${currentBusy}]`)) {
        const stripped = message.slice(currentBusy.length + 3); // strip "[id] " prefix
        setProgressLines((prev) => [...prev.slice(-9), stripped]);
      }
    };

    // Try Tauri native events first, fall back to SSE.
    (async () => {
      try {
        const { listen } = await import("@tauri-apps/api/event");
        unlistenTauri = await listen<{ kind: string; message: string }>("pengine-log", (event) =>
          handleLog(event.payload.kind, event.payload.message),
        );
      } catch {
        // Not in Tauri — use SSE fallback.
        try {
          es = new EventSource(`${PENGINE_API_BASE}/v1/logs`);
          es.onmessage = (event) => {
            try {
              const data = JSON.parse(event.data) as { kind: string; message: string };
              handleLog(data.kind, data.message);
            } catch {
              /* ignore parse errors */
            }
          };
        } catch {
          /* SSE not available */
        }
      }
    })();

    return () => {
      cancelled = true;
      unlistenTauri?.();
      es?.close();
    };
  }, []);

  const loadData = useCallback(async () => {
    const id = ++seqRef.current;
    const [rt, cat] = await Promise.all([fetchRuntimeStatus(), fetchToolCatalog()]);
    if (cancelledRef.current || id !== seqRef.current) return;
    setLoading(false);
    if (rt !== null) {
      setRuntime(rt);
      setRuntimeError(null);
    } else {
      setRuntime(null);
      setRuntimeError("Could not load runtime status");
    }
    if (cat !== null) {
      setCatalog(cat);
      setCatalogError(null);
    } else {
      setCatalogError("Could not load tool catalog");
    }
  }, []);

  useEffect(() => {
    cancelledRef.current = false;
    void loadData();
    return () => {
      cancelledRef.current = true;
    };
  }, [loadData]);

  useRegistryChanged(loadData);

  const handleInstall = async (toolId: string) => {
    setBusyTool(toolId);
    busyToolRef.current = toolId;
    setBusyKind("install");
    setNotice(null);
    setActionError(null);
    setProgressLines([]);
    try {
      const result = await installTool(toolId);
      if (cancelledRef.current) return;
      if (result.ok) {
        setNotice(`"${toolId}" installed`);
        notifyMcpRegistryChanged();
      } else {
        setActionError(result.error ?? "Install failed");
        await loadData();
      }
    } finally {
      if (!cancelledRef.current) {
        setBusyTool(null);
        busyToolRef.current = null;
        setBusyKind(null);
      }
    }
  };

  const savePassthrough = async (tool: CatalogTool) => {
    const keys = tool.passthrough_env ?? [];
    if (!keys.length || !tool.installed) return;
    const draft = passthroughValues[tool.id] ?? {};
    const configured = new Set(tool.passthrough_configured_keys ?? []);
    const replacing = passthroughReplacing[tool.id] ?? {};
    const env: Record<string, string> = {};
    for (const k of keys) {
      const trimmed = (draft[k] ?? "").trim();
      if (configured.has(k) && !replacing[k]) {
        // Omit so the server keeps the existing value (empty draft must not clear secrets).
        continue;
      }
      env[k] = trimmed;
    }
    setPassthroughSavingId(tool.id);
    setActionError(null);
    try {
      const result = await putToolPassthroughEnv(tool.id, env);
      if (cancelledRef.current) return;
      if (result.ok) {
        setNotice(`Saved secrets for ${tool.name}`);
        setPassthroughReplacing((prev) => {
          const next = { ...prev };
          delete next[tool.id];
          return next;
        });
        notifyMcpRegistryChanged();
        await loadData();
      } else {
        setActionError(result.error ?? "Could not save API keys");
      }
    } finally {
      setPassthroughSavingId(null);
    }
  };

  const handleUninstall = async (toolId: string) => {
    setBusyTool(toolId);
    busyToolRef.current = toolId;
    setBusyKind("uninstall");
    setNotice(null);
    setActionError(null);
    setProgressLines([]);
    try {
      const result = await uninstallTool(toolId);
      if (cancelledRef.current) return;
      if (result.ok) {
        setNotice(`"${toolId}" uninstalled`);
        notifyMcpRegistryChanged();
      } else {
        setActionError(result.error ?? "Uninstall failed");
        await loadData();
      }
    } finally {
      if (!cancelledRef.current) {
        setBusyTool(null);
        busyToolRef.current = null;
        setBusyKind(null);
      }
    }
  };

  return (
    <div className="panel p-4 sm:p-6">
      <p className="mono-label">Tool Engine</p>

      {/* Runtime status */}
      <div className="mt-3 flex items-center gap-2">
        <span
          className={`h-2 w-2 shrink-0 rounded-full ${
            runtime?.available
              ? "bg-emerald-400 shadow-[0_0_6px_rgba(52,211,153,0.4)]"
              : runtime === null && loading
                ? "bg-yellow-300"
                : "bg-rose-400"
          }`}
        />
        <p className="font-mono text-[11px] text-white/70">
          {runtime?.available
            ? `${runtime.kind} ${runtime.version}${runtime.rootless ? " (rootless)" : ""}`
            : runtime === null && loading
              ? "Detecting container runtime…"
              : "No container runtime found — install Podman or Docker"}
        </p>
      </div>

      {notice && (
        <p
          className="mt-3 font-mono text-[11px] text-fuchsia-200/90"
          role="status"
          aria-live="polite"
        >
          {notice}
        </p>
      )}

      {actionError && (
        <p className="mt-3 font-mono text-[11px] text-rose-300" role="alert">
          {actionError}
        </p>
      )}

      {runtimeError && (
        <p className="mt-3 font-mono text-[11px] text-amber-200/90" role="status">
          {runtimeError}
        </p>
      )}

      {catalogError && (
        <p className="mt-3 font-mono text-[11px] text-rose-300" role="alert">
          {catalogError}
        </p>
      )}

      {busyTool && busyKind && (
        <div
          className="mt-4 rounded-lg border border-white/[0.07] bg-white/[0.02] px-3 py-2.5"
          role="status"
          aria-live="polite"
          aria-busy="true"
          aria-valuetext={
            busyKind === "install"
              ? `Installing container image ${busyTool}`
              : `Removing container image ${busyTool}`
          }
        >
          <div className="flex items-baseline justify-between gap-2">
            <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-(--mid)">
              {busyKind === "install" ? "Pulling image" : "Removing image"}
            </p>
            <p className="truncate font-mono text-[10px] text-white/45" title={busyTool}>
              {busyTool}
            </p>
          </div>
          <div className="install-progress-track mt-2.5">
            <div
              className={
                busyKind === "install"
                  ? "install-progress-sheen"
                  : "install-progress-sheen install-progress-sheen--remove"
              }
            />
          </div>
          {progressLines.length > 0 && (
            <div className="mt-2 max-h-24 overflow-y-auto rounded border border-white/5 bg-black/20 px-2 py-1.5">
              {progressLines.map((line, i) => (
                <p key={i} className="truncate font-mono text-[10px] leading-relaxed text-white/50">
                  {line}
                </p>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Catalog */}
      {loading && catalog === null && (
        <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
          Loading…
        </p>
      )}

      {catalog !== null && catalog.length === 0 && (
        <p className="mt-3 subtle-copy">No tools in catalog.</p>
      )}

      {catalog !== null && catalog.length > 0 && (
        <div className="mt-4 grid gap-2">
          {catalog.map((tool) => (
            <ToolEngineCatalogToolCard
              key={tool.id}
              tool={tool}
              runtimeAvailable={runtime?.available === true}
              busyTool={busyTool}
              passthroughValues={passthroughValues}
              passthroughReplacing={passthroughReplacing}
              passthroughSavingId={passthroughSavingId}
              setPassthroughValues={setPassthroughValues}
              setPassthroughReplacing={setPassthroughReplacing}
              onSavePassthrough={savePassthrough}
              onInstall={handleInstall}
              onUninstall={handleUninstall}
            />
          ))}
        </div>
      )}
    </div>
  );
}
