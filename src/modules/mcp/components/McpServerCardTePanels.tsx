import { FolderHelper } from "./McpServerCardFolderHelper";

type TeFileManagerMountPanelProps = {
  tePaths: string[];
  teAppMounts: string[];
  tePickError: string | null;
  teApplyError: string | null;
  teApplyBusy: boolean;
  onAddPath: (p: string) => void;
  onRemovePath: (p: string) => void;
  onPickFolder: () => void;
  onApply: () => void;
};

export function TeFileManagerMountPanel({
  tePaths,
  teAppMounts,
  tePickError,
  teApplyError,
  teApplyBusy,
  onAddPath,
  onRemovePath,
  onPickFolder,
  onApply,
}: TeFileManagerMountPanelProps) {
  return (
    <div className="rounded-lg border border-emerald-300/15 bg-emerald-300/5 p-3">
      <p className="mb-2 font-mono text-[10px] uppercase tracking-wider text-emerald-200/80">
        Shared folders (File Manager container mounts)
      </p>
      <p className="mb-2 text-[11px] leading-snug text-(--mid)">
        After File Manager is installed, add paths here (or install it first from Tool Engine with
        an empty list). Each folder mounts as{" "}
        <code className="text-white/70">/app/&lt;name&gt;</code>. Apply updates{" "}
        <code className="text-white/70">workspace_roots</code> in{" "}
        <code className="text-white/70">mcp.json</code> and closes the editor.
      </p>
      {tePaths.length > 0 && (
        <ul className="mb-2 list-inside list-disc font-mono text-[10px] leading-relaxed text-emerald-100/80">
          {tePaths.map((p, i) => (
            <li key={`${p}-${i}`}>
              <span className="text-white/90">{teAppMounts[i] ?? ""}</span>
              <span className="text-(--mid)"> ← </span>
              <span className="break-all text-white/70">{p}</span>
            </li>
          ))}
        </ul>
      )}
      <FolderHelper
        paths={tePaths}
        pickError={tePickError}
        onAdd={onAddPath}
        onRemove={onRemovePath}
        onPickFolder={onPickFolder}
      />
      {teApplyError && (
        <p className="mt-2 font-mono text-[11px] text-rose-300" role="alert">
          {teApplyError}
        </p>
      )}
      <button
        type="button"
        disabled={teApplyBusy}
        onClick={onApply}
        className="mt-3 rounded-lg border border-emerald-300/30 bg-emerald-300/15 px-3 py-1.5 font-mono text-[11px] text-emerald-100 hover:bg-emerald-300/25 disabled:opacity-40"
      >
        {teApplyBusy ? "Applying…" : "Apply folders"}
      </button>
    </div>
  );
}

type TePrivateDataFolderPanelProps = {
  pathInput: string;
  onPathChange: (v: string) => void;
  pickError: string | null;
  applyError: string | null;
  applyBusy: boolean;
  onPickFolder: () => void;
  onApply: () => void;
};

export function TePrivateDataFolderPanel({
  pathInput,
  onPathChange,
  pickError,
  applyError,
  applyBusy,
  onPickFolder,
  onApply,
}: TePrivateDataFolderPanelProps) {
  return (
    <div className="rounded-lg border border-fuchsia-300/15 bg-fuchsia-300/5 p-3">
      <p className="mb-2 font-mono text-[10px] uppercase tracking-wider text-fuchsia-200/80">
        Private data folder (host)
      </p>
      <p className="mb-2 text-[11px] leading-snug text-(--mid)">
        This tool keeps state on disk in a single host directory (bind-mounted into the container).
        Use Choose folder or paste a path, then Apply — same idea as File Manager&apos;s shared
        folders, but only for this tool&apos;s data file(s).
      </p>
      {pickError && (
        <p className="mb-2 font-mono text-[11px] text-rose-300/90" role="alert">
          {pickError}
        </p>
      )}
      <div className="flex gap-1.5">
        <input
          type="text"
          value={pathInput}
          onChange={(e) => onPathChange(e.target.value)}
          placeholder="/path/to/memory-data"
          className="min-w-0 flex-1 rounded-md border border-white/15 bg-white/5 px-2 py-1.5 font-mono text-[11px] text-white outline-none placeholder:text-white/20 focus:border-white/30"
        />
        <button
          type="button"
          onClick={onPickFolder}
          className="shrink-0 rounded-md border border-fuchsia-300/25 bg-fuchsia-300/10 px-2 py-1.5 font-mono text-[10px] text-fuchsia-100/90 hover:bg-fuchsia-300/20"
          title="Choose folder (desktop only)"
        >
          Choose folder
        </button>
      </div>
      {applyError && (
        <p className="mt-2 font-mono text-[11px] text-rose-300" role="alert">
          {applyError}
        </p>
      )}
      <button
        type="button"
        disabled={applyBusy}
        onClick={onApply}
        className="mt-3 rounded-lg border border-fuchsia-300/30 bg-fuchsia-300/15 px-3 py-1.5 font-mono text-[11px] text-fuchsia-100 hover:bg-fuchsia-300/25 disabled:opacity-40"
      >
        {applyBusy ? "Applying…" : "Apply data folder"}
      </button>
    </div>
  );
}
