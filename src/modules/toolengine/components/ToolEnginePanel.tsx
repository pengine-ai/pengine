import * as Accordion from "@radix-ui/react-accordion";
import { useCallback, useEffect, useRef, useState } from "react";
import { notifyMcpRegistryChanged } from "../../../shared/mcpEvents";
import {
  fetchRuntimeStatus,
  fetchToolCatalog,
  installTool,
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
  const [notice, setNotice] = useState<string | null>(null);

  const cancelledRef = useRef(false);
  const seqRef = useRef(0);

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

  const handleInstall = async (toolId: string) => {
    setBusyTool(toolId);
    setNotice(null);
    setActionError(null);
    try {
      const result = await installTool(toolId);
      if (cancelledRef.current) return;
      if (result.ok) {
        setNotice(`"${toolId}" installed`);
        notifyMcpRegistryChanged();
      } else {
        setActionError(result.error ?? "Install failed");
      }
      await loadData();
    } finally {
      if (!cancelledRef.current) setBusyTool(null);
    }
  };

  const handleUninstall = async (toolId: string) => {
    setBusyTool(toolId);
    setNotice(null);
    setActionError(null);
    try {
      const result = await uninstallTool(toolId);
      if (cancelledRef.current) return;
      if (result.ok) {
        setNotice(`"${toolId}" uninstalled`);
        notifyMcpRegistryChanged();
      } else {
        setActionError(result.error ?? "Uninstall failed");
      }
      await loadData();
    } finally {
      if (!cancelledRef.current) setBusyTool(null);
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
                            Docker command list
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
