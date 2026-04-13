import { useState } from "react";
import type { ServerEntry } from "..";

type Props = {
  busy: boolean;
  onAdd: (name: string, entry: ServerEntry) => Promise<void>;
};

type Mode = "paste" | "form";
type ManualKind = "stdio" | "native";

export function AddServerForm({ busy, onAdd }: Props) {
  const [open, setOpen] = useState(false);
  const [mode, setMode] = useState<Mode>("paste");
  const [manualKind, setManualKind] = useState<ManualKind>("stdio");
  const [error, setError] = useState<string | null>(null);

  // Paste mode state
  const [jsonText, setJsonText] = useState("");
  const [pasteName, setPasteName] = useState("");

  // Form mode state
  const [formName, setFormName] = useState("");
  const [nativeId, setNativeId] = useState("");
  const [command, setCommand] = useState("");
  const [argsText, setArgsText] = useState("");
  const [envText, setEnvText] = useState("");
  const [directReturn, setDirectReturn] = useState(false);

  const reset = () => {
    setJsonText("");
    setPasteName("");
    setFormName("");
    setNativeId("");
    setManualKind("stdio");
    setCommand("");
    setArgsText("");
    setEnvText("");
    setDirectReturn(false);
    setError(null);
  };

  const handlePasteSubmit = async () => {
    setError(null);
    let parsed: unknown;
    try {
      parsed = JSON.parse(jsonText);
    } catch {
      setError("Invalid JSON");
      return;
    }

    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
      setError("Expected a JSON object");
      return;
    }

    const obj = parsed as Record<string, unknown>;

    // Detect format: { "type": "stdio", ... } vs { "my-server": { "type": "stdio", ... } }
    let name: string;
    let entry: ServerEntry;

    if ("type" in obj && (obj.type === "stdio" || obj.type === "native")) {
      // Direct entry — need a name from the input
      if (!pasteName.trim()) {
        setError("Enter a tool name (the JSON has no key wrapper)");
        return;
      }
      name = pasteName.trim();
      entry = normalizeEntry(obj);
    } else {
      // Wrapped: { "server-name": { ... } }
      const keys = Object.keys(obj);
      if (keys.length !== 1) {
        setError('Expected either a tool entry or { "name": { ...entry } }');
        return;
      }
      name = keys[0];
      const inner = obj[name];
      if (typeof inner !== "object" || inner === null || Array.isArray(inner)) {
        setError(`Value for "${name}" is not a valid tool entry`);
        return;
      }
      entry = normalizeEntry(inner as Record<string, unknown>);
    }

    try {
      await onAdd(name, entry);
      reset();
      setOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not add tool");
    }
  };

  const handleFormSubmit = async () => {
    setError(null);
    const name = formName.trim();
    if (!name) {
      setError("Tool name is required");
      return;
    }

    if (manualKind === "native") {
      const id = nativeId.trim();
      if (!id) {
        setError("Native id is required (e.g. dice or tool_manager)");
        return;
      }
      try {
        await onAdd(name, { type: "native", id });
        reset();
        setOpen(false);
      } catch (e) {
        setError(e instanceof Error ? e.message : "Could not add tool");
      }
      return;
    }

    if (!command.trim()) {
      setError("Command is required");
      return;
    }

    const args = argsText
      .split("\n")
      .map((l) => l.trim())
      .filter(Boolean);
    const env: Record<string, string> = {};
    for (const line of envText.split("\n")) {
      const eq = line.indexOf("=");
      if (eq > 0) env[line.slice(0, eq).trim()] = line.slice(eq + 1).trim();
    }

    try {
      await onAdd(name, {
        type: "stdio",
        command: command.trim(),
        args,
        env,
        direct_return: directReturn,
      });
      reset();
      setOpen(false);
    } catch (e) {
      setError(e instanceof Error ? e.message : "Could not add tool");
    }
  };

  const inputClass =
    "w-full rounded-lg border border-white/15 bg-white/5 px-2.5 py-2 font-mono text-xs text-white outline-none placeholder:text-white/25 focus:border-white/30";

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="mt-3 w-full rounded-xl border border-dashed border-white/15 px-3 py-3 text-center font-mono text-xs text-(--mid) transition hover:border-white/30 hover:text-white"
      >
        + Add custom tool
      </button>
    );
  }

  return (
    <div className="mt-3 rounded-2xl border border-white/10 bg-black/20 p-3 sm:p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="font-mono text-[11px] uppercase tracking-[0.12em] text-(--mid)">
          Add custom tool
        </p>
        <button
          type="button"
          onClick={() => {
            reset();
            setOpen(false);
          }}
          className="font-mono text-xs text-(--mid) hover:text-white"
        >
          close
        </button>
      </div>

      {/* Mode tabs */}
      <div className="mt-3 flex gap-1 rounded-lg border border-white/10 bg-white/5 p-0.5">
        {(["paste", "form"] as const).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => {
              setMode(m);
              setError(null);
              if (m === "form") setManualKind("stdio");
            }}
            className={`flex-1 rounded-md px-3 py-1.5 font-mono text-[11px] uppercase tracking-wider transition ${
              mode === m ? "bg-white/10 text-white" : "text-(--mid) hover:text-white"
            }`}
          >
            {m === "paste" ? "Paste JSON" : "Manual"}
          </button>
        ))}
      </div>

      {mode === "paste" && (
        <div className="mt-3 grid gap-3">
          <div>
            <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
              Tool name (optional if JSON is wrapped)
            </label>
            <input
              type="text"
              value={pasteName}
              onChange={(e) => setPasteName(e.target.value)}
              placeholder="my-tool"
              className={inputClass}
            />
          </div>
          <div>
            <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
              JSON config
            </label>
            <textarea
              value={jsonText}
              onChange={(e) => setJsonText(e.target.value)}
              rows={7}
              placeholder={`{\n  "type": "stdio",\n  "command": "npx",\n  "args": ["-y", "@modelcontextprotocol/server-something"]\n}`}
              className={`${inputClass} resize-y`}
            />
          </div>
          <button
            type="button"
            disabled={busy || !jsonText.trim()}
            onClick={handlePasteSubmit}
            className="rounded-lg border border-white/20 bg-white/10 px-3 py-1.5 text-xs font-medium text-white hover:bg-white/15 disabled:opacity-40"
          >
            Add custom tool
          </button>
        </div>
      )}

      {mode === "form" && (
        <div className="mt-3 grid gap-3">
          <div className="flex gap-1 rounded-lg border border-white/10 bg-white/5 p-0.5">
            {(["stdio", "native"] as const).map((k) => (
              <button
                key={k}
                type="button"
                onClick={() => {
                  setManualKind(k);
                  setError(null);
                }}
                className={`flex-1 rounded-md px-3 py-1.5 font-mono text-[11px] uppercase tracking-wider transition ${
                  manualKind === k ? "bg-white/10 text-white" : "text-(--mid) hover:text-white"
                }`}
              >
                {k === "stdio" ? "Subprocess" : "Native"}
              </button>
            ))}
          </div>

          <div className="grid gap-3 md:grid-cols-2">
            <div>
              <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
                Tool name
              </label>
              <input
                type="text"
                value={formName}
                onChange={(e) => setFormName(e.target.value)}
                placeholder="my-tool"
                className={inputClass}
              />
            </div>

            {manualKind === "native" ? (
              <div>
                <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
                  Native id
                </label>
                <input
                  type="text"
                  value={nativeId}
                  onChange={(e) => setNativeId(e.target.value)}
                  placeholder="tool_manager"
                  list="pengine-known-native-ids"
                  className={inputClass}
                />
                <datalist id="pengine-known-native-ids">
                  <option value="dice" />
                  <option value="tool_manager" />
                </datalist>
              </div>
            ) : (
              <div>
                <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
                  Command
                </label>
                <input
                  type="text"
                  value={command}
                  onChange={(e) => setCommand(e.target.value)}
                  placeholder="npx"
                  className={inputClass}
                />
              </div>
            )}
          </div>

          {manualKind === "native" && (
            <p className="text-[11px] leading-snug text-(--mid)">
              Built-in tools run inside Pengine (no subprocess). Use{" "}
              <code className="text-white/70">tool_manager</code> for install/uninstall via chat, or{" "}
              <code className="text-white/70">dice</code> for the sample die roll.
            </p>
          )}

          {manualKind === "stdio" && (
            <div className="grid gap-3 md:grid-cols-2">
              <div className="md:col-span-2">
                <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
                  Args (one per line)
                </label>
                <textarea
                  value={argsText}
                  onChange={(e) => setArgsText(e.target.value)}
                  rows={3}
                  placeholder={"-y\n@modelcontextprotocol/server-something"}
                  className={`${inputClass} resize-y`}
                />
              </div>
              <div className="md:col-span-2">
                <label className="mb-1 block font-mono text-[10px] uppercase tracking-wider text-(--mid)">
                  Env (KEY=value per line)
                </label>
                <textarea
                  value={envText}
                  onChange={(e) => setEnvText(e.target.value)}
                  rows={2}
                  placeholder={"API_KEY=sk-..."}
                  className={`${inputClass} resize-y`}
                />
              </div>
              <label className="flex items-center gap-2 text-xs text-white/80 md:col-span-2">
                <input
                  type="checkbox"
                  checked={directReturn}
                  onChange={(e) => setDirectReturn(e.target.checked)}
                  className="accent-emerald-400"
                />
                Direct return (skip model summary)
              </label>
            </div>
          )}

          <button
            type="button"
            disabled={
              busy ||
              !formName.trim() ||
              (manualKind === "native" ? !nativeId.trim() : !command.trim())
            }
            onClick={handleFormSubmit}
            className="rounded-lg border border-white/20 bg-white/10 px-3 py-2 text-xs font-medium text-white hover:bg-white/15 disabled:opacity-40 md:w-fit md:px-4"
          >
            Add custom tool
          </button>
        </div>
      )}

      {error && <p className="mt-2 font-mono text-[11px] text-rose-300">{error}</p>}
    </div>
  );
}

/** Normalize a raw JSON object into a ServerEntry, filling in defaults. */
function normalizeEntry(obj: Record<string, unknown>): ServerEntry {
  if (obj.type === "native") {
    return { type: "native", id: String(obj.id ?? "") };
  }
  return {
    type: "stdio",
    command: String(obj.command ?? ""),
    args: Array.isArray(obj.args) ? obj.args.map(String) : [],
    env:
      typeof obj.env === "object" && obj.env !== null && !Array.isArray(obj.env)
        ? Object.fromEntries(
            Object.entries(obj.env as Record<string, unknown>).map(([k, v]) => [k, String(v)]),
          )
        : {},
    direct_return: Boolean(obj.direct_return ?? false),
  };
}
