import * as Accordion from "@radix-ui/react-accordion";
import { useEffect, useMemo, useState } from "react";
import { fetchMcpTools, type McpTool } from "..";

/**
 * Dashboard panel showing MCP tool *groups*. Each accordion item is one tool
 * group (= one MCP server, e.g. "dice"); expanding it reveals the individual
 * commands that group exposes (e.g. `roll_dice`).
 */
export function McpToolsPanel() {
  const [tools, setTools] = useState<McpTool[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = async () => {
      const data = await fetchMcpTools();
      if (!cancelled) setTools(data);
    };
    load();
    const timer = setInterval(load, 10000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

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

  return (
    <div className="panel p-6">
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
        <Accordion.Root
          type="multiple"
          defaultValue={groups.map((g) => g.server)}
          className="mt-4 grid gap-2"
        >
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
