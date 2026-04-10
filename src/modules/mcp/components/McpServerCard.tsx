import { useState } from "react";
import type { McpTool, ServerEntry, ServerEntryStdio } from "..";

type Props = {
  name: string;
  entry: ServerEntry;
  tools: McpTool[];
  busy: boolean;
  editingName: string | null;
  onSave: (name: string, entry: ServerEntry) => Promise<void>;
  onDelete: (name: string) => Promise<void>;
  onEditStart: (name: string | null) => void;
};

/** Detect filesystem MCP package in live args textarea (one token per line). */
function argsTextLooksLikeFilesystem(argsText: string): boolean {
  return argsText
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean)
    .some((a) => a.includes("server-filesystem"));
}

export function McpServerCard({
  name,
  entry,
  tools,
  busy,
  editingName,
  onSave,
  onDelete,
  onEditStart,
}: Props) {
  const isNative = entry.type === "native";
  const isEditing = editingName === name;
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const toolCount = tools.filter((t) => t.server === name).length;
  const commandPreview =
    entry.type === "stdio" ? [entry.command, ...entry.args].join(" ") : `native:${entry.id}`;

  const handleToggleDirect = async () => {
    if (entry.type !== "stdio") return;
    await onSave(name, { ...entry, direct_return: !entry.direct_return });
  };

  const handleDelete = async () => {
    setDeleteError(null);
    try {
      await onDelete(name);
      setConfirmDelete(false);
    } catch (e) {
      setDeleteError(e instanceof Error ? e.message : "Could not remove server");
    }
  };

  // ── Editing: form replaces the card content ────────────────────────
  if (isEditing && entry.type === "stdio") {
    return (
      <div className="rounded-xl border border-white/10 bg-white/5">
        <InlineEditForm
          name={name}
          entry={entry}
          busy={busy}
          onSave={(updated) => onSave(name, updated)}
          onCancel={() => onEditStart(null)}
        />
      </div>
    );
  }

  // ── Read-only card ─────────────────────────────────────────────────
  return (
    <div className="min-w-0 rounded-xl border border-white/10 bg-white/5">
      <div className="flex flex-wrap items-start justify-between gap-3 px-4 py-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <p className="break-all text-sm font-semibold text-white">{name}</p>
            <span
              className={`rounded-full px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider ${
                isNative ? "bg-sky-300/10 text-sky-300" : "bg-fuchsia-300/10 text-fuchsia-300"
              }`}
            >
              {entry.type}
            </span>
            {entry.type === "stdio" && entry.direct_return && (
              <span className="rounded-full bg-emerald-300/10 px-2 py-0.5 font-mono text-[10px] uppercase tracking-wider text-emerald-300">
                direct
              </span>
            )}
          </div>
          <p className="mt-1 break-all font-mono text-[11px] text-white/50" title={commandPreview}>
            {commandPreview}
          </p>
          <p className="mt-1 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
            {toolCount} tool{toolCount === 1 ? "" : "s"}
          </p>
        </div>

        {!isNative && (
          <div className="flex w-full flex-wrap items-center justify-end gap-1.5 pt-0.5 sm:w-auto sm:shrink-0">
            {entry.type === "stdio" && (
              <button
                type="button"
                disabled={busy}
                onClick={handleToggleDirect}
                title={entry.direct_return ? "Disable direct return" : "Enable direct return"}
                className={`rounded-lg border px-2 py-1 font-mono text-[10px] uppercase tracking-wider transition disabled:opacity-40 ${
                  entry.direct_return
                    ? "border-emerald-300/30 bg-emerald-300/10 text-emerald-300 hover:bg-emerald-300/20"
                    : "border-white/15 bg-transparent text-(--mid) hover:border-white/25 hover:text-white"
                }`}
              >
                direct
              </button>
            )}
            <button
              type="button"
              disabled={busy}
              onClick={() => onEditStart(name)}
              className="rounded-lg border border-white/15 bg-transparent px-2 py-1 font-mono text-[10px] uppercase tracking-wider text-(--mid) hover:border-white/25 hover:text-white disabled:opacity-40"
            >
              edit
            </button>
            <button
              type="button"
              disabled={busy}
              onClick={() => {
                setDeleteError(null);
                setConfirmDelete(true);
              }}
              aria-label={`Delete server ${name}`}
              title={`Delete server ${name}`}
              className="rounded-lg border border-rose-300/20 bg-transparent px-2 py-1 font-mono text-[10px] uppercase tracking-wider text-rose-300/70 hover:bg-rose-300/10 hover:text-rose-200 disabled:opacity-40"
            >
              del
            </button>
          </div>
        )}
      </div>

      {confirmDelete && (
        <div className="border-t border-white/10 px-4 py-3">
          <p className="text-xs text-rose-200">
            Remove <strong>{name}</strong>? Its tools will be disconnected.
          </p>
          {deleteError && (
            <p className="mt-2 font-mono text-[11px] text-rose-300" role="alert">
              {deleteError}
            </p>
          )}
          <div className="mt-2 flex gap-2">
            <button
              type="button"
              disabled={busy}
              onClick={handleDelete}
              className="rounded-lg border border-rose-300/30 bg-rose-300/10 px-3 py-1 text-xs font-medium text-rose-100 hover:bg-rose-300/20 disabled:opacity-40"
            >
              Remove
            </button>
            <button
              type="button"
              onClick={() => {
                setConfirmDelete(false);
                setDeleteError(null);
              }}
              className="rounded-lg border border-white/15 bg-transparent px-3 py-1 text-xs text-(--mid) hover:text-white"
            >
              Cancel
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

// ── Unified edit form (all servers, with filesystem folder helper) ───

const INPUT_CLASS =
  "w-full rounded-lg border border-white/15 bg-white/5 px-2.5 py-2 font-mono text-xs text-white outline-none placeholder:text-white/25 focus:border-white/30";

function InlineEditForm({
  name,
  entry,
  busy,
  onSave,
  onCancel,
}: {
  name: string;
  entry: ServerEntryStdio;
  busy: boolean;
  onSave: (entry: ServerEntry) => Promise<void>;
  onCancel: () => void;
}) {
  const [command, setCommand] = useState(entry.command);
  const [argsText, setArgsText] = useState(entry.args.join("\n"));
  const [envText, setEnvText] = useState(
    Object.entries(entry.env)
      .map(([k, v]) => `${k}=${v}`)
      .join("\n"),
  );
  const [directReturn, setDirectReturn] = useState(entry.direct_return);
  const [pickFolderError, setPickFolderError] = useState<string | null>(null);

  const isFs = argsTextLooksLikeFilesystem(argsText);

  // ── Filesystem folder helpers (read/write the args textarea) ──────

  const parsePaths = (): string[] => {
    const lines = argsText
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const pkgIdx = lines.findIndex((a) => a.includes("server-filesystem"));
    if (pkgIdx < 0) return [];
    return lines.slice(pkgIdx + 1);
  };

  const updatePaths = (paths: string[]) => {
    const lines = argsText
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const pkgIdx = lines.findIndex((a) => a.includes("server-filesystem"));
    if (pkgIdx < 0) return;
    const prefix = lines.slice(0, pkgIdx + 1);
    setArgsText([...prefix, ...paths].join("\n"));
  };

  const addPath = (p: string) => {
    const trimmed = p.trim();
    if (!trimmed) return;
    const current = parsePaths();
    if (!current.includes(trimmed)) {
      updatePaths([...current, trimmed]);
    }
  };

  const removePath = (path: string) => {
    updatePaths(parsePaths().filter((p) => p !== path));
  };

  const pickFolder = async () => {
    setPickFolderError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      try {
        const picked = await invoke<string | null>("pick_mcp_filesystem_folder");
        if (picked) addPath(picked);
      } catch (invokeErr) {
        setPickFolderError(
          invokeErr instanceof Error ? invokeErr.message : "Could not open folder picker",
        );
      }
    } catch {
      // Web / non-Tauri: dynamic import of `@tauri-apps/api/core` fails — expected, stay silent
    }
  };

  // ── Submit ────────────────────────────────────────────────────────

  const handleSubmit = async () => {
    const args = argsText
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const env: Record<string, string> = {};
    for (const line of envText.split("\n")) {
      const eq = line.indexOf("=");
      if (eq > 0) {
        const key = line.slice(0, eq).trim();
        if (key !== "") env[key] = line.slice(eq + 1).trim();
      }
    }
    await onSave({
      type: "stdio",
      command: command.trim(),
      args,
      env,
      direct_return: directReturn,
    });
  };

  const fsPaths = isFs ? parsePaths() : [];

  return (
    <div className="px-4 py-3">
      {/* Header with name + cancel */}
      <div className="mb-3 flex items-center justify-between">
        <p className="text-sm font-semibold text-white">{name}</p>
        <button
          type="button"
          onClick={onCancel}
          className="font-mono text-[10px] uppercase tracking-wider text-(--mid) hover:text-white"
        >
          cancel
        </button>
      </div>

      <div className="grid gap-3">
        {/* Filesystem folder helper */}
        {isFs && (
          <FolderHelper
            paths={fsPaths}
            pickError={pickFolderError}
            onAdd={addPath}
            onRemove={removePath}
            onPickFolder={pickFolder}
          />
        )}

        <div>
          <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
            Command
          </label>
          <input
            type="text"
            value={command}
            onChange={(e) => setCommand(e.target.value)}
            placeholder="npx"
            className={INPUT_CLASS}
          />
        </div>
        <div>
          <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
            Args (one per line)
          </label>
          <textarea
            value={argsText}
            onChange={(e) => setArgsText(e.target.value)}
            rows={Math.max(3, argsText.split("\n").length + 1)}
            placeholder={"-y\n@modelcontextprotocol/server-something"}
            className={`${INPUT_CLASS} resize-y`}
          />
        </div>
        <div>
          <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
            Env (KEY=value per line)
          </label>
          <textarea
            value={envText}
            onChange={(e) => setEnvText(e.target.value)}
            rows={2}
            placeholder={"API_KEY=sk-..."}
            className={`${INPUT_CLASS} resize-y`}
          />
        </div>
        <label className="flex items-center gap-2 text-xs text-white/80">
          <input
            type="checkbox"
            checked={directReturn}
            onChange={(e) => setDirectReturn(e.target.checked)}
            className="accent-emerald-400"
          />
          Direct return (skip model summary)
        </label>
      </div>
      <div className="mt-3 flex gap-2">
        <button
          type="button"
          disabled={busy || !command.trim()}
          onClick={handleSubmit}
          className="rounded-lg border border-white/20 bg-white/10 px-3 py-1.5 text-xs font-medium text-white hover:bg-white/15 disabled:opacity-40"
        >
          Save
        </button>
        <button
          type="button"
          onClick={onCancel}
          className="rounded-lg border border-white/15 bg-transparent px-3 py-1.5 text-xs text-(--mid) hover:text-white"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

// ── Folder path helper (visual add/remove for filesystem paths) ─────

function FolderHelper({
  paths,
  pickError,
  onAdd,
  onRemove,
  onPickFolder,
}: {
  paths: string[];
  pickError: string | null;
  onAdd: (p: string) => void;
  onRemove: (path: string) => void;
  onPickFolder: () => void;
}) {
  const [newPath, setNewPath] = useState("");

  const handleAdd = () => {
    if (newPath.trim()) {
      onAdd(newPath);
      setNewPath("");
    }
  };

  return (
    <div className="rounded-lg border border-white/10 bg-black/20 p-3">
      <p className="mb-2 font-mono text-[10px] uppercase tracking-wider text-(--mid)">
        Allowed folders
      </p>

      {paths.length === 0 && <p className="mb-2 text-xs text-white/30 italic">No folders yet</p>}

      {pickError && (
        <p className="mb-2 font-mono text-[11px] text-rose-300/90" role="alert">
          {pickError}
        </p>
      )}

      {paths.length > 0 && (
        <div className="mb-2 grid gap-1">
          {paths.map((p, i) => (
            <div
              key={`${p}-${i}`}
              className="flex items-center gap-2 rounded-md border border-white/8 bg-white/5 px-2 py-1"
            >
              <p className="min-w-0 flex-1 truncate font-mono text-[11px] text-white/80" title={p}>
                {p}
              </p>
              <button
                type="button"
                onClick={() => onRemove(p)}
                aria-label={`Remove allowed folder ${p}`}
                title="Remove folder"
                className="shrink-0 font-mono text-[10px] text-rose-300/50 hover:text-rose-200"
              >
                x
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="flex gap-1.5">
        <input
          type="text"
          value={newPath}
          onChange={(e) => setNewPath(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && newPath.trim()) {
              e.preventDefault();
              handleAdd();
            }
          }}
          placeholder="/path/to/folder"
          className="min-w-0 flex-1 rounded-md border border-white/15 bg-white/5 px-2 py-1.5 font-mono text-[11px] text-white outline-none placeholder:text-white/20 focus:border-white/30"
        />
        <button
          type="button"
          disabled={!newPath.trim()}
          onClick={handleAdd}
          className="shrink-0 rounded-md border border-white/15 bg-white/8 px-2 py-1.5 font-mono text-[10px] text-white/70 hover:bg-white/15 hover:text-white disabled:opacity-30"
        >
          add
        </button>
        <button
          type="button"
          onClick={onPickFolder}
          className="shrink-0 rounded-md border border-white/15 bg-transparent px-2 py-1.5 font-mono text-[10px] text-(--mid) hover:border-white/25 hover:text-white"
          title="Choose folder (desktop only)"
        >
          browse
        </button>
      </div>
    </div>
  );
}
