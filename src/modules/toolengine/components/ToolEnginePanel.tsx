import * as Accordion from "@radix-ui/react-accordion";
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
            <div
              key={tool.id}
              className="rounded-xl border border-white/10 bg-white/5 px-3 py-2.5 sm:rounded-2xl sm:px-4 sm:py-3"
            >
              <div className="flex min-w-0 items-start justify-between gap-3">
                <div className="min-w-0">
                  <p className="truncate text-sm font-semibold text-white" title={tool.id}>
                    {tool.name}
                  </p>
                  <p className="mt-0.5 font-mono text-[11px] text-(--mid)">
                    v{tool.version} — {tool.commands.length} command
                    {tool.commands.length === 1 ? "" : "s"}
                  </p>
                  <p className="mt-1 text-[11px] leading-snug text-(--mid) sm:text-xs">
                    {tool.description}
                  </p>
                  {tool.id === "pengine/fetch" && (
                    <p className="mt-1 font-mono text-[10px] leading-snug text-(--mid)">
                      robots.txt:{" "}
                      {tool.ignore_robots_txt
                        ? "ignored for this install (opt-in)"
                        : "enforced — set catalog ignore_robots_txt or mcp_server_cmd only if you accept bypassing robots for all fetched URLs"}
                      {tool.robots_ignore_allowlist && tool.robots_ignore_allowlist.length > 0
                        ? ` · allowlist (informational): ${tool.robots_ignore_allowlist.join(", ")}`
                        : ""}
                    </p>
                  )}
                  {(tool.passthrough_env?.length ?? 0) > 0 && !tool.installed && (
                    <p className="mt-2 font-mono text-[9px] text-white/40">
                      Install this tool, then set {tool.passthrough_env?.join(", ")} below so the
                      MCP server can start and register all commands.
                    </p>
                  )}
                  {(tool.passthrough_env?.length ?? 0) > 0 && tool.installed && (
                    <div className="mt-2 rounded-lg border border-white/10 bg-black/15 px-2.5 py-2 sm:px-3">
                      <p className="font-mono text-[10px] uppercase tracking-[0.12em] text-(--mid)">
                        Container secrets
                      </p>
                      {tool.passthrough_configured_keys &&
                      tool.passthrough_configured_keys.length > 0 ? (
                        <p className="mt-1 font-mono text-[9px] text-emerald-200/80">
                          Saved: {tool.passthrough_configured_keys.join(", ")}
                        </p>
                      ) : (
                        <p className="mt-1 font-mono text-[9px] text-amber-200/70">
                          Required for this tool to start. Stored locally in mcp.json (not sent to
                          the model).
                        </p>
                      )}
                      <div className="mt-2 space-y-2">
                        {(tool.passthrough_env ?? []).map((key) => {
                          const configuredKeys = tool.passthrough_configured_keys ?? [];
                          const isSaved = configuredKeys.includes(key);
                          const isReplacing = passthroughReplacing[tool.id]?.[key] === true;
                          if (isSaved && !isReplacing) {
                            return (
                              <div
                                key={key}
                                className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-white/10 bg-black/20 px-2 py-1.5"
                              >
                                <span className="font-mono text-[9px] text-white/45">{key}</span>
                                <div className="flex min-w-0 flex-1 items-center justify-end gap-2">
                                  <span
                                    className="truncate font-mono text-[11px] tracking-widest text-white/35"
                                    aria-hidden
                                  >
                                    ••••••••
                                  </span>
                                  <button
                                    type="button"
                                    disabled={busyTool !== null || passthroughSavingId === tool.id}
                                    onClick={() => {
                                      setPassthroughReplacing((prev) => ({
                                        ...prev,
                                        [tool.id]: { ...(prev[tool.id] ?? {}), [key]: true },
                                      }));
                                      setPassthroughValues((prev) => ({
                                        ...prev,
                                        [tool.id]: { ...(prev[tool.id] ?? {}), [key]: "" },
                                      }));
                                    }}
                                    className="shrink-0 rounded border border-white/15 bg-white/5 px-2 py-0.5 font-mono text-[10px] text-white/70 transition hover:bg-white/10 disabled:opacity-40"
                                  >
                                    Replace…
                                  </button>
                                </div>
                              </div>
                            );
                          }
                          return (
                            <label key={key} className="block">
                              <div className="flex items-baseline justify-between gap-2">
                                <span className="font-mono text-[9px] text-white/45">{key}</span>
                                {isSaved && isReplacing ? (
                                  <button
                                    type="button"
                                    disabled={busyTool !== null || passthroughSavingId === tool.id}
                                    onClick={() => {
                                      setPassthroughReplacing((prev) => {
                                        const row = { ...(prev[tool.id] ?? {}) };
                                        delete row[key];
                                        const next = { ...prev };
                                        if (Object.keys(row).length) next[tool.id] = row;
                                        else delete next[tool.id];
                                        return next;
                                      });
                                      setPassthroughValues((prev) => ({
                                        ...prev,
                                        [tool.id]: { ...(prev[tool.id] ?? {}), [key]: "" },
                                      }));
                                    }}
                                    className="font-mono text-[9px] text-white/40 underline decoration-white/20 underline-offset-2 hover:text-white/60"
                                  >
                                    Cancel
                                  </button>
                                ) : null}
                              </div>
                              <input
                                type={/KEY|SECRET|TOKEN|PASSWORD/i.test(key) ? "password" : "text"}
                                value={passthroughValues[tool.id]?.[key] ?? ""}
                                onChange={(e) =>
                                  setPassthroughValues((prev) => ({
                                    ...prev,
                                    [tool.id]: { ...(prev[tool.id] ?? {}), [key]: e.target.value },
                                  }))
                                }
                                placeholder={
                                  isReplacing ? "New value (empty removes key)" : undefined
                                }
                                className="mt-0.5 w-full rounded-md border border-white/10 bg-black/25 px-2 py-1 font-mono text-[11px] text-white outline-none focus:border-emerald-300/35"
                                autoComplete="off"
                              />
                            </label>
                          );
                        })}
                      </div>
                      <div className="mt-2 flex justify-end">
                        <button
                          type="button"
                          disabled={busyTool !== null || passthroughSavingId === tool.id}
                          onClick={() => void savePassthrough(tool)}
                          className="rounded-lg border border-emerald-300/20 bg-emerald-300/10 px-3 py-1 font-mono text-[11px] text-emerald-300 transition hover:bg-emerald-300/20 disabled:opacity-40"
                        >
                          {passthroughSavingId === tool.id ? "Saving…" : "Save keys"}
                        </button>
                      </div>
                    </div>
                  )}
                </div>

                <button
                  type="button"
                  disabled={busyTool !== null || !runtime?.available}
                  onClick={() =>
                    void (tool.installed ? handleUninstall(tool.id) : handleInstall(tool.id))
                  }
                  className={`shrink-0 rounded-lg border px-3 py-1 font-mono text-[11px] transition disabled:opacity-40 ${
                    tool.installed
                      ? "border-rose-300/20 bg-transparent text-rose-300/70 hover:bg-rose-300/10 hover:text-rose-200"
                      : "border-emerald-300/20 bg-emerald-300/10 text-emerald-300 hover:bg-emerald-300/20"
                  }`}
                >
                  {busyTool === tool.id
                    ? tool.installed
                      ? "Removing…"
                      : "Installing…"
                    : tool.installed
                      ? "Uninstall"
                      : "Install"}
                </button>
              </div>

              {/* MCP tools exposed by the container image (collapsible, same pattern as MCP Tools) */}
              {tool.commands.length > 0 && (
                <Accordion.Root
                  type="single"
                  collapsible
                  className="mt-2 border-t border-white/5 pt-2"
                >
                  <Accordion.Item
                    value={`${tool.id}-commands`}
                    className="overflow-hidden rounded-lg border border-white/10 bg-white/3"
                  >
                    <Accordion.Header>
                      <Accordion.Trigger className="group flex w-full min-w-0 items-center justify-between gap-3 px-2.5 py-2 text-left sm:px-3 sm:py-2.5">
                        <div className="min-w-0">
                          <p className="truncate text-xs font-semibold text-white/90">
                            Container commands
                          </p>
                          <p className="mt-0.5 font-mono text-[10px] uppercase tracking-[0.12em] text-(--mid)">
                            {tool.commands.length} MCP tool
                            {tool.commands.length === 1 ? "" : "s"}
                          </p>
                        </div>
                        <span className="shrink-0 font-mono text-xs text-(--mid) transition group-data-[state=open]:rotate-45">
                          +
                        </span>
                      </Accordion.Trigger>
                    </Accordion.Header>
                    <Accordion.Content className="border-t border-white/10 px-2.5 py-2 sm:px-3 sm:py-2.5">
                      <ul className="grid gap-1.5">
                        {tool.commands.map((cmd) => (
                          <li key={cmd.name}>
                            <p className="break-all font-mono text-xs text-white/80">{cmd.name}</p>
                            {cmd.description ? (
                              <p className="mt-0.5 text-[11px] leading-snug text-(--mid) sm:text-xs">
                                {cmd.description}
                              </p>
                            ) : null}
                          </li>
                        ))}
                      </ul>
                    </Accordion.Content>
                  </Accordion.Item>
                </Accordion.Root>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
