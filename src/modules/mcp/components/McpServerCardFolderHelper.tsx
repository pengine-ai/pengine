import { useState } from "react";

export function FolderHelper({
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
