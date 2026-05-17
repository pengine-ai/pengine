import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { AuditLogPanel, getPengineHealth, TerminalPreview } from "../modules/bot";
import { useAppSessionStore } from "../modules/bot/store/appSessionStore";
import { CronPanel } from "../modules/cron";
import { McpToolsPanel } from "../modules/mcp/components/McpToolsPanel";
import { fetchOllamaModels, setPreferredOllamaModel } from "../modules/ollama/api";
import type { OllamaModelInfo } from "../modules/ollama/types";
import { CliCommandsPanel } from "../modules/cli";
import { SkillsPanel } from "../modules/skills";
import { ToolEnginePanel } from "../modules/toolengine/components/ToolEnginePanel";
import { UpdateIndicator } from "../modules/updater";
import { TopMenu } from "../shared/ui/TopMenu";

type ServiceInfo = {
  name: string;
  status: "running" | "stopped" | "checking";
  detail: string;
};

export function DashboardPage() {
  const navigate = useNavigate();
  const isDeviceConnected = useAppSessionStore((state) => state.isDeviceConnected);
  const disconnectDevice = useAppSessionStore((state) => state.disconnectDevice);
  const botUsername = useAppSessionStore((state) => state.botUsername);
  const [availableModels, setAvailableModels] = useState<OllamaModelInfo[]>([]);
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [activeModel, setActiveModel] = useState<string | null>(null);
  const [savingModel, setSavingModel] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [services, setServices] = useState<ServiceInfo[]>([
    { name: "Pengine", status: "checking", detail: "Checking…" },
    { name: "Telegram", status: "checking", detail: "Checking…" },
    { name: "Ollama", status: "checking", detail: "Checking…" },
  ]);
  const [disconnectError, setDisconnectError] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [gitCommit, setGitCommit] = useState<string | null>(null);
  const refreshStatus = useCallback(async () => {
    let botUser = botUsername ?? "unknown";
    const health = await getPengineHealth(3000);
    const pengineUp = !!health;
    const botConnected = health?.bot_connected ?? false;
    if (health?.bot_username) botUser = health.bot_username;
    if (health) {
      setAppVersion(health.app_version ?? null);
      setGitCommit(health.git_commit ?? null);
    } else {
      setAppVersion(null);
      setGitCommit(null);
    }

    const ollama = await fetchOllamaModels(2500);
    const ollamaUp = ollama.reachable;
    const effectiveModel = ollama.selected_model ?? ollama.active_model;
    setAvailableModels(ollama.models);
    setSelectedModel(ollama.selected_model);
    setActiveModel(ollama.active_model);

    setServices([
      {
        name: "Pengine",
        status: pengineUp ? "running" : "stopped",
        detail: pengineUp ? "API reachable" : "Not running",
      },
      {
        name: "Telegram",
        status: botConnected ? "running" : "stopped",
        detail: botConnected ? `@${botUser}` : "Not connected",
      },
      {
        name: "Ollama",
        status: ollamaUp ? "running" : "stopped",
        detail: ollamaUp ? (effectiveModel ? effectiveModel : "No model loaded") : "Not reachable",
      },
    ]);
  }, [botUsername]);

  useEffect(() => {
    refreshStatus();
    const timer = setInterval(refreshStatus, 10000);
    return () => clearInterval(timer);
  }, [refreshStatus]);

  const handleDisconnect = async () => {
    setDisconnectError(null);
    try {
      await disconnectDevice();
      navigate("/setup", { replace: true });
    } catch (e) {
      setDisconnectError(e instanceof Error ? e.message : "Could not disconnect");
    }
  };

  const handleModelChange = async (value: string) => {
    const next = value === "__active__" ? null : value;
    setModelError(null);
    setSavingModel(true);
    const result = await setPreferredOllamaModel(next);
    setSavingModel(false);
    if (result.ok) {
      await refreshStatus();
      return;
    }
    setModelError(result.error ?? "Could not update model");
  };

  const anyChecking = services.some((s) => s.status === "checking");
  const allRunning = services.every((s) => s.status === "running");

  return (
    <div className="relative overflow-x-clip pb-20">
      <TopMenu />

      <main className="section-shell pt-6 sm:pt-10">
        {/* ── Status bar: services + connection ──────────────────── */}
        <div className="flex flex-wrap items-center gap-2 sm:flex-nowrap sm:gap-3">
          <div className="flex min-w-0 flex-wrap items-center gap-2 sm:gap-3">
            {/* Overall status */}
            <div className="flex items-center gap-2">
              <span
                className={`h-2 w-2 shrink-0 rounded-full sm:h-2.5 sm:w-2.5 ${
                  anyChecking
                    ? "bg-yellow-300"
                    : allRunning
                      ? "bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.5)]"
                      : "bg-rose-400"
                }`}
              />
              <p className="font-mono text-xs font-semibold text-white sm:text-sm">
                {anyChecking
                  ? "Checking services..."
                  : allRunning
                    ? "All systems running"
                    : "Some services offline"}
              </p>
            </div>

            <div className="mx-0.5 hidden h-4 w-px bg-white/10 sm:block" />

            {/* Service pills — hide detail text on small screens */}
            {services.map((service) => (
              <div
                key={service.name}
                className="flex items-center gap-1.5 rounded-full border border-white/10 bg-white/5 px-2.5 py-1 sm:px-3"
              >
                <span
                  className={`h-1.5 w-1.5 shrink-0 rounded-full ${
                    service.status === "running"
                      ? "bg-emerald-400"
                      : service.status === "stopped"
                        ? "bg-rose-400"
                        : "bg-yellow-400"
                  }`}
                />
                <span className="font-mono text-[10px] text-white/70 sm:text-[11px]">
                  {service.name}
                </span>
                <span className="hidden font-mono text-[11px] text-white/40 sm:inline">
                  {service.detail}
                </span>
              </div>
            ))}
          </div>

          {/* Connection controls */}
          <div className="flex shrink-0 items-center gap-2 whitespace-nowrap sm:ml-auto">
            <div className="flex items-center gap-1 rounded-lg border border-cyan-300/20 bg-cyan-300/10 px-2 py-1">
              <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-cyan-300" />
              <select
                value={selectedModel ?? "__active__"}
                onChange={(event) => void handleModelChange(event.target.value)}
                disabled={savingModel}
                className="max-w-28 bg-transparent font-mono text-[10px] text-cyan-100 outline-none sm:max-w-36"
                title={
                  selectedModel
                    ? `Using selected model: ${selectedModel}`
                    : activeModel
                      ? `Using active model: ${activeModel}`
                      : "No Ollama model detected"
                }
              >
                <option value="__active__">
                  {activeModel ? `Active (${activeModel})` : "Active model"}
                </option>
                {availableModels.map((model) => (
                  <option key={model.name} value={model.name}>
                    {model.kind === "cloud" ? `${model.name} · cloud` : model.name}
                  </option>
                ))}
              </select>
            </div>
            {!isDeviceConnected && (
              <Link
                to="/setup"
                className="rounded-lg border border-white/15 bg-white/5 px-3 py-1 font-mono text-[11px] text-white/70 transition hover:bg-white/10 hover:text-white"
              >
                Setup
              </Link>
            )}
            <Link
              to="/settings"
              className="rounded-lg border border-white/15 bg-white/5 px-3 py-1 font-mono text-[11px] text-white/70 transition hover:bg-white/10 hover:text-white"
            >
              Settings
            </Link>
            {isDeviceConnected && (
              <button
                type="button"
                onClick={handleDisconnect}
                className="rounded-lg border border-rose-300/20 bg-transparent px-3 py-1 font-mono text-[11px] text-rose-300/60 transition hover:bg-rose-300/10 hover:text-rose-200"
              >
                Disconnect
              </button>
            )}
          </div>
        </div>

        {disconnectError && (
          <p className="mt-2 font-mono text-xs text-rose-300">{disconnectError}</p>
        )}
        {modelError && <p className="mt-2 font-mono text-xs text-rose-300">{modelError}</p>}

        {appVersion && gitCommit && (
          <p
            className="mt-2 font-mono text-[10px] tracking-wide text-white/35 sm:text-[11px]"
            title={`${gitCommit}`}
            data-testid="app-build-info"
          >
            Pengine v{appVersion}
            {gitCommit !== "unknown" ? ` · ${gitCommit.slice(0, 7)}` : ""}
          </p>
        )}

        <UpdateIndicator currentVersion={appVersion} />

        {/* ── Terminal (full width) — live runtime log ───────── */}
        <section className="mt-4 sm:mt-6">
          <TerminalPreview />
        </section>

        {/* ── Terminal CLI (PATH launcher toggle) ───────────────── */}
        <section className="mt-4 sm:mt-6">
          <CliCommandsPanel />
        </section>

        {/* ── Saved audit files (disk) — separate from live stream ─ */}
        <section className="mt-4 sm:mt-6">
          <AuditLogPanel />
        </section>

        {/* ── MCP tools & commands ────────────────────────────────── */}
        <section className="mt-4 sm:mt-6">
          <McpToolsPanel />
        </section>

        {/* ── Tool Engine (container tools) ───────────────────────── */}
        <section className="mt-4 sm:mt-6">
          <ToolEnginePanel />
        </section>

        {/* ── Cron jobs (scheduled prompts) ────────────────────────── */}
        <section className="mt-4 sm:mt-6">
          <CronPanel />
        </section>

        {/* ── Skills (README-only context templates) ──────────────── */}
        <section className="mt-4 sm:mt-6">
          <SkillsPanel />
        </section>
      </main>
    </div>
  );
}
