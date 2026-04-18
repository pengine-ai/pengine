import { useEffect, useRef, useState } from "react";
import { fetchToolCatalog, putToolPrivateFolder } from "../../toolengine";
import { workspaceAppContainerMountPaths } from "../../../shared/workspaceMounts";
import {
  fetchMcpConfig,
  putMcpFilesystemPaths,
  type McpTool,
  type ServerEntry,
  type ServerEntryStdio,
} from "..";
import {
  buildEnvMapFromMcpForm,
  envToOtherLinesText,
  extractPrimarySecretEnvKey,
} from "../mcpEnvHelpers";
import { FolderHelper } from "./McpServerCardFolderHelper";
import { TeFileManagerMountPanel, TePrivateDataFolderPanel } from "./McpServerCardTePanels";

/** `pengine/memory` → `te_pengine-memory` (matches Rust `server_key`). */
function teServerKeyForToolId(toolId: string): string {
  return `te_${toolId.replace(/\//g, "-")}`;
}

type Props = {
  name: string;
  entry: ServerEntry;
  tools: McpTool[];
  busy: boolean;
  editingName: string | null;
  onSave: (name: string, entry: ServerEntry) => Promise<void>;
  onDelete: (name: string) => Promise<void>;
  onEditStart: (name: string | null) => void;
  /** After filesystem paths apply (te_ File Manager), refresh server list from API. */
  onReloadServers?: () => Promise<void>;
};

/** Tauri `invoke` rejects with a plain string for command `Err(...)` returns,
 *  not an `Error` — surface that string instead of a generic fallback. */
function pickFolderErrorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "string" && e.trim()) return e;
  return "Could not open folder picker";
}

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
  onReloadServers,
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
      setDeleteError(e instanceof Error ? e.message : "Could not remove tool");
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
          onReloadServers={onReloadServers}
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
            {toolCount} command{toolCount === 1 ? "" : "s"}
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
              aria-label={`Delete tool ${name}`}
              title={`Delete tool ${name}`}
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
  onReloadServers,
}: {
  name: string;
  entry: ServerEntryStdio;
  busy: boolean;
  onSave: (entry: ServerEntry) => Promise<void>;
  onCancel: () => void;
  onReloadServers?: () => Promise<void>;
}) {
  const [command, setCommand] = useState(entry.command);
  const [argsText, setArgsText] = useState(entry.args.join("\n"));
  const primarySecretKey0 = extractPrimarySecretEnvKey(entry.env);
  const baselineSecretRef = useRef<{ name: string; value: string } | null>(
    primarySecretKey0 && entry.env[primarySecretKey0]
      ? { name: primarySecretKey0, value: entry.env[primarySecretKey0] }
      : null,
  );
  const [apiKeyName, setApiKeyName] = useState(primarySecretKey0 ?? "");
  const [apiKeyValue, setApiKeyValue] = useState("");
  const [envOtherLines, setEnvOtherLines] = useState(
    envToOtherLinesText(entry.env, primarySecretKey0),
  );
  const [replacingSecret, setReplacingSecret] = useState(false);
  const [directReturn, setDirectReturn] = useState(entry.direct_return);
  const [pickFolderError, setPickFolderError] = useState<string | null>(null);

  const isTeFileManager = name === "te_pengine-file-manager";
  const [tePaths, setTePaths] = useState<string[]>([]);
  const teAppMounts = isTeFileManager ? workspaceAppContainerMountPaths(tePaths) : [];
  const [tePickError, setTePickError] = useState<string | null>(null);
  const [teApplyError, setTeApplyError] = useState<string | null>(null);
  const [teApplyBusy, setTeApplyBusy] = useState(false);

  /** Catalog tool id (e.g. `pengine/memory`) when this server uses `private_folder`. */
  const [tePrivateToolId, setTePrivateToolId] = useState<string | null>(null);
  const [tePrivatePathInput, setTePrivatePathInput] = useState("");
  const [tePrivatePickError, setTePrivatePickError] = useState<string | null>(null);
  const [tePrivateApplyError, setTePrivateApplyError] = useState<string | null>(null);
  const [tePrivateApplyBusy, setTePrivateApplyBusy] = useState(false);

  useEffect(() => {
    if (!isTeFileManager) return;
    void (async () => {
      const cfg = await fetchMcpConfig(5000);
      if (cfg) setTePaths([...cfg.filesystem_allowed_paths]);
    })();
  }, [isTeFileManager, name]);

  useEffect(() => {
    if (!name.startsWith("te_")) {
      setTePrivateToolId(null);
      setTePrivatePathInput("");
      return;
    }
    let cancelled = false;
    void (async () => {
      const cat = await fetchToolCatalog(5000);
      if (cancelled) return;
      const t = cat?.find((x) => teServerKeyForToolId(x.id) === name && x.private_folder != null);
      if (t) {
        setTePrivateToolId(t.id);
        setTePrivatePathInput(t.private_host_path ?? "");
      } else {
        setTePrivateToolId(null);
        setTePrivatePathInput("");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [name]);

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
        setPickFolderError(pickFolderErrorMessage(invokeErr));
      }
    } catch {
      // Web / non-Tauri: dynamic import of `@tauri-apps/api/core` fails — expected, stay silent
    }
  };

  const addTePath = (p: string) => {
    const t = p.trim();
    if (!t || tePaths.includes(t)) return;
    setTePaths((prev) => [...prev, t]);
  };

  const removeTePath = (p: string) => {
    setTePaths((prev) => prev.filter((x) => x !== p));
  };

  const pickTeFolder = async () => {
    setTePickError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      try {
        const picked = await invoke<string | null>("pick_mcp_filesystem_folder");
        if (picked) addTePath(picked);
      } catch (invokeErr) {
        setTePickError(pickFolderErrorMessage(invokeErr));
      }
    } catch {
      // Web / non-Tauri
    }
  };

  const applyTeFolders = async () => {
    setTeApplyError(null);
    setTeApplyBusy(true);
    const ok = await putMcpFilesystemPaths(tePaths, 60_000);
    setTeApplyBusy(false);
    if (!ok) {
      setTeApplyError("Could not save — is the Pengine API running?");
      return;
    }
    await onReloadServers?.();
    onCancel();
  };

  const pickTePrivateFolder = async () => {
    setTePrivatePickError(null);
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      try {
        const picked = await invoke<string | null>("pick_mcp_filesystem_folder");
        if (picked) setTePrivatePathInput(picked);
      } catch (invokeErr) {
        setTePrivatePickError(pickFolderErrorMessage(invokeErr));
      }
    } catch {
      setTePrivatePickError("Folder picker needs the desktop app (Tauri).");
    }
  };

  const applyTePrivateFolder = async () => {
    if (!tePrivateToolId) return;
    setTePrivateApplyError(null);
    const path = tePrivatePathInput.trim();
    if (!path) {
      setTePrivateApplyError("Enter a host folder path or use Choose folder.");
      return;
    }
    setTePrivateApplyBusy(true);
    const result = await putToolPrivateFolder(tePrivateToolId, path, 120_000);
    setTePrivateApplyBusy(false);
    if (!result.ok) {
      setTePrivateApplyError(result.error ?? "Could not save");
      return;
    }
    await onReloadServers?.();
    onCancel();
  };

  // ── Submit ────────────────────────────────────────────────────────

  const privatePathBaseline = entry.private_host_path ?? "";
  const hasUnsavedPrivate =
    tePrivateToolId != null && tePrivatePathInput.trim() !== privatePathBaseline.trim();

  const handleSubmit = async () => {
    if (hasUnsavedPrivate || tePrivateApplyBusy) return;
    const args = argsText
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const preserved =
      !replacingSecret &&
      baselineSecretRef.current &&
      apiKeyName.trim() === baselineSecretRef.current.name
        ? baselineSecretRef.current.value
        : null;
    const env = buildEnvMapFromMcpForm({
      otherLinesText: envOtherLines,
      apiKeyName,
      apiKeyValue,
      preservedSecretValue: preserved,
      replacingSecret,
    });
    await onSave({
      type: "stdio",
      command: command.trim(),
      args,
      env,
      direct_return: directReturn,
      private_host_path: entry.private_host_path ?? null,
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
        {isTeFileManager && (
          <TeFileManagerMountPanel
            tePaths={tePaths}
            teAppMounts={teAppMounts}
            tePickError={tePickError}
            teApplyError={teApplyError}
            teApplyBusy={teApplyBusy}
            onAddPath={addTePath}
            onRemovePath={removeTePath}
            onPickFolder={() => void pickTeFolder()}
            onApply={() => void applyTeFolders()}
          />
        )}

        {tePrivateToolId && (
          <TePrivateDataFolderPanel
            pathInput={tePrivatePathInput}
            onPathChange={setTePrivatePathInput}
            pickError={tePrivatePickError}
            applyError={tePrivateApplyError}
            applyBusy={tePrivateApplyBusy}
            onPickFolder={() => void pickTePrivateFolder()}
            onApply={() => void applyTePrivateFolder()}
          />
        )}

        {/* Filesystem folder helper (npx server-filesystem) */}
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
        <div className="rounded-lg border border-white/10 bg-black/15 p-3">
          <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
            API key / secret (optional)
          </label>
          <p className="mb-2 text-[10px] leading-snug text-white/40">
            Variable name plus value. After you save and reopen edit, the value stays hidden until
            you choose Replace.
          </p>
          {baselineSecretRef.current && !replacingSecret ? (
            <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border border-white/10 bg-black/25 px-2 py-2">
              <div className="min-w-0">
                <p className="font-mono text-[9px] text-white/45">
                  {(baselineSecretRef.current?.name ?? apiKeyName) || "—"}
                </p>
                <p
                  className="mt-0.5 font-mono text-[11px] tracking-widest text-white/35"
                  aria-hidden
                >
                  ••••••••
                </p>
              </div>
              <button
                type="button"
                disabled={busy}
                onClick={() => {
                  setReplacingSecret(true);
                  setApiKeyValue("");
                }}
                className="shrink-0 rounded border border-white/15 bg-white/5 px-2 py-0.5 font-mono text-[10px] text-white/70 transition hover:bg-white/10 disabled:opacity-40"
              >
                Replace…
              </button>
            </div>
          ) : (
            <div className="grid gap-2 sm:grid-cols-2">
              <div className="sm:col-span-1">
                <label className="mb-0.5 block font-mono text-[9px] text-white/45">
                  Variable name
                </label>
                <input
                  type="text"
                  value={apiKeyName}
                  onChange={(e) => setApiKeyName(e.target.value)}
                  placeholder="BRAVE_API_KEY"
                  autoComplete="off"
                  className={INPUT_CLASS}
                />
              </div>
              <div className="sm:col-span-1">
                <div className="flex items-end justify-between gap-2">
                  <label className="mb-0.5 block font-mono text-[9px] text-white/45">Value</label>
                  {replacingSecret && baselineSecretRef.current ? (
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => {
                        setReplacingSecret(false);
                        setApiKeyName(baselineSecretRef.current?.name ?? "");
                        setApiKeyValue("");
                      }}
                      className="mb-0.5 font-mono text-[9px] text-white/40 underline decoration-white/20 underline-offset-2 hover:text-white/60"
                    >
                      Cancel
                    </button>
                  ) : null}
                </div>
                <input
                  type="password"
                  value={apiKeyValue}
                  onChange={(e) => setApiKeyValue(e.target.value)}
                  placeholder={replacingSecret ? "New value (empty removes)" : "Secret value"}
                  autoComplete="new-password"
                  className={INPUT_CLASS}
                />
              </div>
            </div>
          )}
        </div>
        <div>
          <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
            Other environment (KEY=value per line)
          </label>
          <p className="mb-1.5 text-[10px] leading-snug text-white/40">
            Use for non-secret vars. Do not repeat the API key name here.
          </p>
          <textarea
            value={envOtherLines}
            onChange={(e) => setEnvOtherLines(e.target.value)}
            rows={2}
            placeholder={"NODE_ENV=production"}
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
      <div className="mt-3 flex flex-wrap items-center gap-2">
        <button
          type="button"
          disabled={busy || !command.trim() || hasUnsavedPrivate || tePrivateApplyBusy}
          onClick={() => void handleSubmit()}
          className="rounded-lg border border-white/20 bg-white/10 px-3 py-1.5 text-xs font-medium text-white hover:bg-white/15 disabled:opacity-40"
        >
          Save
        </button>
        {hasUnsavedPrivate ? (
          <p className="font-mono text-[10px] text-amber-200/90">
            Apply data folder first (or revert the path field) before Save.
          </p>
        ) : null}
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
