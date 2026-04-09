import * as Accordion from "@radix-ui/react-accordion";
import { useEffect, useMemo, useState } from "react";
import {
  fetchMcpConfig,
  fetchMcpTools,
  putMcpFilesystemPath,
  type McpConfigInfo,
  type McpTool,
} from "..";

/**
 * Dashboard panel showing MCP tool *groups*. Each accordion item is one tool
 * group (= one MCP server, e.g. "dice"); expanding it reveals the individual
 * commands that group exposes (e.g. `roll_dice`).
 */
export function McpToolsPanel() {
  const [tools, setTools] = useState<McpTool[] | null>(null);
  const [config, setConfig] = useState<McpConfigInfo | null>(null);
  const [pathDraft, setPathDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const syncAfterSave = async () => {
    const [t, c] = await Promise.all([fetchMcpTools(), fetchMcpConfig()]);
    setTools(t);
    setConfig(c);
    if (c?.filesystem_allowed_path) setPathDraft(c.filesystem_allowed_path);
  };

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout>;

    void (async () => {
      const c = await fetchMcpConfig();
      if (cancelled) return;
      setConfig(c);
      if (c?.filesystem_allowed_path) setPathDraft(c.filesystem_allowed_path);
    })();

    const pollTools = async () => {
      const data = await fetchMcpTools();
      if (cancelled) return;
      setTools(data);
      const next = data.length > 0 ? 10_000 : 1_000;
      timer = setTimeout(() => pollTools(), next);
    };

    pollTools();
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, []);

  const applyPath = async (path: string) => {
    const trimmed = path.trim();
    if (!trimmed) {
      setNotice("Enter a folder path");
      return;
    }
    setBusy(true);
    setNotice(null);
    const ok = await putMcpFilesystemPath(trimmed);
    setBusy(false);
    if (!ok) {
      setNotice("Could not save (is the API running?)");
      return;
    }
    await syncAfterSave();
    setNotice("Saved — tools reloaded");
  };

  const pickFolder = async () => {
    setNotice(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const picked = await invoke<string | null>("pick_mcp_filesystem_folder");
      if (picked) await applyPath(picked);
    } catch {
      setNotice("Folder picker needs the desktop app");
    }
  };

  // Bucket tools by their server (group) name. Stable, alphabetical order.
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

  const sourceLabel =
    config == null
      ? "…"
      : config.source === "project"
        ? "Project (src-tauri/mcp.json)"
        : "App data mcp.json";

  return (
    <div className="panel p-6">
      <p className="mono-label">MCP config</p>
      <div className="mt-3 rounded-xl border border-white/10 bg-black/20 px-3 py-3">
        <p className="text-xs text-(--mid)">{sourceLabel}</p>
        <p
          className="mt-1 font-mono text-[11px] text-white/80 break-all"
          title={config?.config_path}
        >
          {config?.config_path ?? "…"}
        </p>
        <p className="mt-2 text-[11px] uppercase tracking-[0.12em] text-(--mid)">
          Filesystem allow folder
        </p>
        <input
          type="text"
          value={pathDraft}
          onChange={(e) => setPathDraft(e.target.value)}
          placeholder="/absolute/path/to/project"
          className="mt-1.5 w-full rounded-lg border border-white/15 bg-white/5 px-2.5 py-2 font-mono text-xs text-white outline-none placeholder:text-white/25 focus:border-white/30"
        />
        <div className="mt-2 flex flex-wrap gap-2">
          <button
            type="button"
            disabled={busy}
            onClick={() => applyPath(pathDraft)}
            className="rounded-lg border border-white/20 bg-white/10 px-3 py-1.5 text-xs font-medium text-white hover:bg-white/15 disabled:opacity-40"
          >
            Apply path
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={pickFolder}
            className="rounded-lg border border-white/15 bg-transparent px-3 py-1.5 text-xs text-(--mid) hover:border-white/25 hover:text-white disabled:opacity-40"
          >
            Choose folder…
          </button>
        </div>
        {notice && <p className="mt-2 font-mono text-[11px] text-fuchsia-200/90">{notice}</p>}
      </div>

      <p className="mono-label mt-8">Available tools</p>

      {groups === null && (
        <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
          Loading…
        </p>
      )}

      {groups !== null && groups.length === 0 && (
        <p className="mt-3 subtle-copy">No MCP tools connected.</p>
      )}

      {groups !== null && groups.length > 0 && (
        <Accordion.Root type="multiple" defaultValue={[]} className="mt-4 grid gap-2">
          {groups.map((group) => (
            <Accordion.Item
              key={group.server}
              value={group.server}
              className="overflow-hidden rounded-2xl border border-white/10 bg-white/5"
            >
              <Accordion.Header>
                <Accordion.Trigger className="group flex w-full items-center justify-between gap-4 px-4 py-3 text-left">
                  <div>
                    <p className="text-sm font-semibold text-white">{group.server}</p>
                    <p className="mt-1 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
                      {group.items.length} command
                      {group.items.length === 1 ? "" : "s"}
                    </p>
                  </div>
                  <span className="font-mono text-xs text-(--mid) transition group-data-[state=open]:rotate-45">
                    +
                  </span>
                </Accordion.Trigger>
              </Accordion.Header>
              <Accordion.Content className="border-t border-white/10 px-4 py-3">
                <ul className="grid gap-2">
                  {group.items.map((tool) => (
                    <li key={tool.name}>
                      <p className="font-mono text-xs text-white">{tool.name}</p>
                      {tool.description && (
                        <p className="mt-0.5 text-xs text-(--mid)">{tool.description}</p>
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
  );
}
