import * as Accordion from "@radix-ui/react-accordion";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  deleteMcpServer,
  fetchMcpServers,
  fetchMcpTools,
  upsertMcpServer,
  type McpTool,
  type ServerEntry,
} from "..";
import { PENGINE_MCP_REGISTRY_CHANGED } from "../../../shared/mcpEvents";
import { AddServerForm } from "./AddServerForm";
import { McpServerCard } from "./McpServerCard";

/**
 * Dashboard panel: MCP tools (config entries), CRUD, and commands grouped per tool.
 */
export function McpToolsPanel() {
  const [tools, setTools] = useState<McpTool[] | null>(null);
  const [servers, setServers] = useState<Record<string, ServerEntry> | null>(null);
  const [toolsError, setToolsError] = useState<string | null>(null);
  const [serversError, setServersError] = useState<string | null>(null);
  const [toolsLoading, setToolsLoading] = useState(true);
  const [serversLoading, setServersLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [editingName, setEditingName] = useState<string | null>(null);

  const toolsSeqRef = useRef(0);
  const serversSeqRef = useRef(0);
  const cancelledRef = useRef(false);
  const pollTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scheduleToolsPollRef = useRef<(delay: number) => void>(() => {});
  const toolsRef = useRef<McpTool[] | null>(null);
  toolsRef.current = tools;

  const reload = useCallback(async () => {
    setServersLoading(true);
    setToolsLoading(true);
    const sId = ++serversSeqRef.current;
    const tId = ++toolsSeqRef.current;
    const [t, s] = await Promise.all([fetchMcpTools(), fetchMcpServers()]);
    if (cancelledRef.current) return;

    if (sId === serversSeqRef.current) {
      setServersLoading(false);
      if (s !== null) {
        setServers(s);
        setServersError(null);
      } else {
        setServersError("Could not load MCP tools");
      }
    }

    if (tId === toolsSeqRef.current) {
      setToolsLoading(false);
      if (t !== null) {
        setTools(t);
        setToolsError(null);
      } else {
        setToolsError("Could not load MCP commands");
      }
      const next = t !== null && t.length > 0 ? 10_000 : 30_000;
      scheduleToolsPollRef.current(next);
    }
  }, []);

  useEffect(() => {
    cancelledRef.current = false;

    const schedulePoll = (delay: number) => {
      if (pollTimerRef.current) clearTimeout(pollTimerRef.current);
      pollTimerRef.current = setTimeout(runPoll, delay);
    };
    scheduleToolsPollRef.current = schedulePoll;

    const runPoll = async () => {
      pollTimerRef.current = null;
      const tId = ++toolsSeqRef.current;
      if (toolsRef.current === null) setToolsLoading(true);
      const data = await fetchMcpTools();
      if (cancelledRef.current) return;
      if (tId !== toolsSeqRef.current) return;

      setToolsLoading(false);
      if (data !== null) {
        setTools(data);
        setToolsError(null);
      } else {
        setToolsError("Could not load MCP commands");
      }
      const next = data !== null && data.length > 0 ? 10_000 : 30_000;
      schedulePoll(next);
    };

    const loadServersOnce = async () => {
      const sId = ++serversSeqRef.current;
      setServersLoading(true);
      const s = await fetchMcpServers();
      if (cancelledRef.current) return;
      if (sId !== serversSeqRef.current) return;
      setServersLoading(false);
      if (s !== null) {
        setServers(s);
        setServersError(null);
      } else {
        setServersError("Could not load MCP tools");
      }
    };

    void loadServersOnce();
    schedulePoll(0);

    return () => {
      cancelledRef.current = true;
      if (pollTimerRef.current) clearTimeout(pollTimerRef.current);
      pollTimerRef.current = null;
      scheduleToolsPollRef.current = () => {};
    };
  }, []);

  useEffect(() => {
    const onRegistryChanged = () => {
      void reload();
    };
    window.addEventListener(PENGINE_MCP_REGISTRY_CHANGED, onRegistryChanged);
    return () => window.removeEventListener(PENGINE_MCP_REGISTRY_CHANGED, onRegistryChanged);
  }, [reload]);

  // ── Server CRUD handlers ───────────────────────────────────────────

  const handleSaveServer = async (name: string, entry: ServerEntry): Promise<boolean> => {
    setBusy(true);
    setNotice(null);
    const ok = await upsertMcpServer(name, entry);
    if (!ok) {
      setNotice(`Could not save "${name}"`);
      setBusy(false);
      return false;
    }
    setEditingName(null);
    await reload();
    setBusy(false);
    setNotice(`Tool "${name}" saved — commands reloaded`);
    return true;
  };

  const handleAddServer = async (name: string, entry: ServerEntry) => {
    const ok = await handleSaveServer(name, entry);
    if (!ok) {
      throw new Error(`Could not save "${name}"`);
    }
  };

  const handleDeleteServer = async (name: string) => {
    setBusy(true);
    setNotice(null);
    const ok = await deleteMcpServer(name);
    if (!ok) {
      setBusy(false);
      throw new Error(`Could not remove "${name}"`);
    }
    await reload();
    setBusy(false);
    setNotice(`Tool "${name}" removed`);
  };

  // ── Derived data ───────────────────────────────────────────────────

  const serverEntries = useMemo(() => {
    if (!servers) return null;
    return Object.entries(servers).sort(([a], [b]) => a.localeCompare(b));
  }, [servers]);

  const groups = useMemo(() => {
    if (!tools) return null;
    const map = new Map<string, McpTool[]>();
    for (const tool of tools) {
      const list = map.get(tool.server) ?? [];
      list.push(tool);
      map.set(tool.server, list);
    }
    return [...map.entries()]
      .sort(([a], [b]) => a.localeCompare(b))
      .map(([server, items]) => ({ server, items }));
  }, [tools]);

  return (
    <div className="panel p-4 sm:p-6">
      {notice && (
        <p
          className="mb-3 font-mono text-[11px] text-fuchsia-200/90"
          role="status"
          aria-live="polite"
          aria-atomic="true"
        >
          {notice}
        </p>
      )}

      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        {/* ── MCP tools (mcp.json server entries) ─────────────────── */}
        <div className="min-w-0">
          <p className="mono-label">Tools</p>

          {serversError && servers !== null && (
            <p className="mt-2 font-mono text-[11px] text-amber-200/90" role="alert">
              {serversError}
            </p>
          )}

          {serversLoading && servers === null && !serversError && (
            <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
              Loading…
            </p>
          )}

          {serversError && servers === null && !serversLoading && (
            <p className="mt-3 font-mono text-[11px] text-rose-300" role="alert">
              {serversError}
            </p>
          )}

          {serverEntries !== null && (
            <div className="mt-3 grid gap-2">
              {serverEntries.map(([name, entry]) => (
                <McpServerCard
                  key={name}
                  name={name}
                  entry={entry}
                  tools={tools ?? []}
                  busy={busy}
                  editingName={editingName}
                  onSave={async (serverName, serverEntry) => {
                    await handleSaveServer(serverName, serverEntry);
                  }}
                  onDelete={handleDeleteServer}
                  onEditStart={setEditingName}
                  onReloadServers={reload}
                />
              ))}
            </div>
          )}

          <AddServerForm busy={busy} onAdd={handleAddServer} />
        </div>

        {/* ── Commands exposed by each tool ───────────────────────── */}
        <div className="min-w-0">
          <p className="mono-label">Commands</p>

          {toolsError && tools !== null && (
            <p className="mt-2 font-mono text-[11px] text-amber-200/90" role="alert">
              {toolsError}
            </p>
          )}

          {toolsLoading && tools === null && !toolsError && (
            <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
              Loading…
            </p>
          )}

          {toolsError && tools === null && !toolsLoading && (
            <p className="mt-3 font-mono text-[11px] text-rose-300" role="alert">
              {toolsError}
            </p>
          )}

          {groups !== null && groups.length === 0 && (
            <p className="mt-3 subtle-copy">No MCP commands available.</p>
          )}

          {groups !== null && groups.length > 0 && (
            <Accordion.Root type="multiple" defaultValue={[]} className="mt-3 grid gap-2">
              {groups.map((group) => (
                <Accordion.Item
                  key={group.server}
                  value={group.server}
                  className="overflow-hidden rounded-xl border border-white/10 bg-white/5 sm:rounded-2xl"
                >
                  <Accordion.Header>
                    <Accordion.Trigger className="group flex w-full min-w-0 items-center justify-between gap-4 px-3 py-2.5 text-left sm:px-4 sm:py-3">
                      <div className="min-w-0">
                        <p
                          className="truncate text-sm font-semibold text-white"
                          title={group.server}
                        >
                          {group.server}
                        </p>
                        <p className="mt-0.5 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
                          {group.items.length} command
                          {group.items.length === 1 ? "" : "s"}
                        </p>
                      </div>
                      <span className="font-mono text-xs text-(--mid) transition group-data-[state=open]:rotate-45">
                        +
                      </span>
                    </Accordion.Trigger>
                  </Accordion.Header>
                  <Accordion.Content className="border-t border-white/10 px-3 py-2.5 sm:px-4 sm:py-3">
                    <ul className="grid gap-1.5">
                      {group.items.map((tool) => (
                        <li key={tool.name}>
                          <p className="break-all font-mono text-xs text-white">{tool.name}</p>
                          {tool.description && (
                            <p className="mt-0.5 wrap-break-word text-[11px] leading-snug text-(--mid) sm:text-xs">
                              {tool.description}
                            </p>
                          )}
                        </li>
                      ))}
                    </ul>
                  </Accordion.Content>
                </Accordion.Item>
              ))}
            </Accordion.Root>
          )}
        </div>
      </div>
    </div>
  );
}
