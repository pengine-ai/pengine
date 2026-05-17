import { useCallback, useEffect, useState } from "react";
import { isTauriApp } from "../../../shared/runtimeTarget";
import { cliShimInstall, cliShimRemove, cliShimStatus } from "../api";
import type { CliShimStatus } from "../types";

export function CliCommandsPanel() {
  const [shim, setShim] = useState<CliShimStatus | null>(null);
  const [shimError, setShimError] = useState<string | null>(null);
  const [shimBusy, setShimBusy] = useState(false);
  const [shimMsg, setShimMsg] = useState<string | null>(null);

  const refreshShim = useCallback(async () => {
    if (!isTauriApp()) {
      setShim(null);
      setShimError(null);
      return;
    }
    setShimError(null);
    const s = await cliShimStatus();
    if (s === null) {
      setShim(null);
      setShimError("Could not read CLI launcher status (invoke failed).");
      return;
    }
    setShim(s);
  }, []);

  useEffect(() => {
    void refreshShim();
  }, [refreshShim]);

  const setLauncherEnabled = async (enabled: boolean) => {
    if (!shim || shimBusy) return;
    setShimBusy(true);
    setShimMsg(null);
    if (enabled) {
      const r = await cliShimInstall();
      setShimBusy(false);
      if (r.ok) {
        setShim(r.status);
        setShimMsg(r.status.localBinOnPath ? "On PATH." : "Installed.");
      } else {
        setShimMsg(r.error);
      }
    } else {
      const r = await cliShimRemove();
      setShimBusy(false);
      if (r.ok) {
        await refreshShim();
        setShimMsg("Removed.");
      } else {
        setShimMsg(r.error);
      }
    }
  };

  return (
    <div className="rounded-xl border border-white/10 bg-white/3 p-4 sm:p-5">
      <h2 className="font-mono text-sm font-semibold tracking-wide text-white/90">Terminal CLI</h2>

      {isTauriApp() && (
        <div className="mt-3 rounded-lg border border-cyan-300/15 bg-cyan-300/5 p-3 sm:p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <p className="min-w-0 font-mono text-[11px] font-semibold text-cyan-100/90">
              CLI on PATH
            </p>
            <button
              type="button"
              role="switch"
              aria-checked={Boolean(shim?.installed)}
              aria-label="CLI launcher on PATH"
              disabled={shimBusy || !shim}
              title={
                shim?.installed ? "Remove CLI launcher from PATH" : "Install CLI launcher on PATH"
              }
              onClick={() => void setLauncherEnabled(!shim?.installed)}
              className={`relative h-5 w-9 shrink-0 rounded-full border transition disabled:opacity-40 ${
                shim?.installed ? "border-cyan-300/40 bg-cyan-300/25" : "border-white/15 bg-white/5"
              }`}
            >
              <span
                className={`absolute top-1/2 block h-3.5 w-3.5 -translate-y-1/2 rounded-full transition ${
                  shim?.installed
                    ? "left-[18px] bg-cyan-200 shadow-[0_0_6px_rgba(165,243,252,0.35)]"
                    : "left-[2px] bg-white/40"
                }`}
              />
            </button>
          </div>

          {shimError && <p className="mt-2 font-mono text-[11px] text-rose-300">{shimError}</p>}
          {!shim && !shimError && (
            <p className="mt-2 font-mono text-[11px] text-white/40">Loading…</p>
          )}
          {shim && (
            <div className="mt-2 space-y-1 break-all font-mono text-[10px] text-white/45">
              <div className="text-white/50">{shim.shimPath}</div>
              {shim.installed && shim.resolvesTo && (
                <div className="text-white/35" title={shim.resolvesTo}>
                  → {shim.resolvesTo}
                </div>
              )}
              {!shim.localBinOnPath && (
                <p className="pt-1 text-amber-200/75">{shim.pathExportHint}</p>
              )}
            </div>
          )}
          {shimMsg && <p className="mt-2 font-mono text-[11px] text-white/70">{shimMsg}</p>}
        </div>
      )}

      {!isTauriApp() && (
        <p className="mt-3 font-mono text-[10px] text-white/40">
          Use the desktop app for this toggle.
        </p>
      )}
    </div>
  );
}
