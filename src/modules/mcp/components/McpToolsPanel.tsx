import * as Accordion from "@radix-ui/react-accordion";
import { useEffect, useMemo, useState } from "react";
import {
  deleteMcpServer,
  fetchMcpServers,
  fetchMcpTools,
  upsertMcpServer,
  type McpTool,
  type ServerEntry,
} from "..";
import { AddServerForm } from "./AddServerForm";
import { McpServerCard } from "./McpServerCard";

/**
 * Dashboard panel: filesystem shortcut, server list with CRUD, and tool groups.
 */
export function McpToolsPanel() {
  const [tools, setTools] = useState<McpTool[] | null>(null);
  const [servers, setServers] = useState<Record<string, ServerEntry> | null>(null);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [editingName, setEditingName] = useState<string | null>(null);

  const reload = async () => {
    const [t, s] = await Promise.all([fetchMcpTools(), fetchMcpServers()]);
    setTools(t);
    setServers(s);
  };

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    void (async () => {
      const s = await fetchMcpServers();
      if (cancelled) return;
      setServers(s);
    })();

    const pollTools = async () => {
      const data = await fetchMcpTools();
      if (cancelled) return;
      setTools(data);
      const next = data.length > 0 ? 10_000 : 30_000;
      timer = setTimeout(() => pollTools(), next);
    };

    pollTools();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, []);

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
    setNotice(`Server "${name}" saved — tools reloaded`);
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
    setNotice(`Server "${name}" removed`);
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
      {notice && <p className="mb-3 font-mono text-[11px] text-fuchsia-200/90">{notice}</p>}

      <div className="grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
        {/* ── Servers ─────────────────────────────────────────────── */}
        <div className="min-w-0">
          <p className="mono-label">Servers</p>

          {serverEntries === null && (
            <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
              Loading…
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
                />
              ))}
            </div>
          )}

          <AddServerForm busy={busy} onAdd={handleAddServer} />
        </div>

        {/* ── Available tools ─────────────────────────────────────── */}
        <div className="min-w-0">
          <p className="mono-label">Available tools</p>

          {groups === null && (
            <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
              Loading…
            </p>
          )}

          {groups !== null && groups.length === 0 && (
            <p className="mt-3 subtle-copy">No MCP tools connected.</p>
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
