import * as Accordion from "@radix-ui/react-accordion";
import type { Dispatch, SetStateAction } from "react";
import type { CatalogTool } from "..";

type Props = {
  tool: CatalogTool;
  runtimeAvailable: boolean;
  busyTool: string | null;
  passthroughValues: Record<string, Record<string, string>>;
  passthroughReplacing: Record<string, Record<string, boolean>>;
  passthroughSavingId: string | null;
  setPassthroughValues: Dispatch<SetStateAction<Record<string, Record<string, string>>>>;
  setPassthroughReplacing: Dispatch<SetStateAction<Record<string, Record<string, boolean>>>>;
  onSavePassthrough: (tool: CatalogTool) => void;
  onInstall: (toolId: string) => void;
  onUninstall: (toolId: string) => void;
};

export function ToolEngineCatalogToolCard({
  tool,
  runtimeAvailable,
  busyTool,
  passthroughValues,
  passthroughReplacing,
  passthroughSavingId,
  setPassthroughValues,
  setPassthroughReplacing,
  onSavePassthrough,
  onInstall,
  onUninstall,
}: Props) {
  return (
    <div className="rounded-xl border border-white/10 bg-white/5 px-3 py-2.5 sm:rounded-2xl sm:px-4 sm:py-3">
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
              Install this tool, then set {tool.passthrough_env?.join(", ")} below so the MCP server
              can start and register all commands.
            </p>
          )}
          {(tool.passthrough_env?.length ?? 0) > 0 && tool.installed && (
            <div className="mt-2 rounded-lg border border-white/10 bg-black/15 px-2.5 py-2 sm:px-3">
              <p className="font-mono text-[10px] uppercase tracking-[0.12em] text-(--mid)">
                Container secrets
              </p>
              {tool.passthrough_configured_keys && tool.passthrough_configured_keys.length > 0 ? (
                <p className="mt-1 font-mono text-[9px] text-emerald-200/80">
                  Saved: {tool.passthrough_configured_keys.join(", ")}
                </p>
              ) : (
                <p className="mt-1 font-mono text-[9px] text-amber-200/70">
                  Required for this tool to start. Stored locally in mcp.json (not sent to the
                  model).
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
                        placeholder={isReplacing ? "New value (empty removes key)" : undefined}
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
                  onClick={() => void onSavePassthrough(tool)}
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
          disabled={busyTool !== null || !runtimeAvailable}
          onClick={() => void (tool.installed ? onUninstall(tool.id) : onInstall(tool.id))}
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

      {tool.commands.length > 0 && (
        <Accordion.Root type="single" collapsible className="mt-2 border-t border-white/5 pt-2">
          <Accordion.Item
            value={`${tool.id}-commands`}
            className="overflow-hidden rounded-lg border border-white/10 bg-white/3"
          >
            <Accordion.Header>
              <Accordion.Trigger className="group flex w-full min-w-0 items-center justify-between gap-3 px-2.5 py-2 text-left sm:px-3 sm:py-2.5">
                <div className="min-w-0">
                  <p className="truncate text-xs font-semibold text-white/90">Container commands</p>
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
  );
}
