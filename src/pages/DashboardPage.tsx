import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { TerminalPreview } from "../components/TerminalPreview";
import { TopMenu } from "../components/TopMenu";
import { OLLAMA_API_BASE, PENGINE_API_BASE } from "../config";
import { useAppSessionStore } from "../stores/appSessionStore";

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
  const [services, setServices] = useState<ServiceInfo[]>([
    { name: "Telegram gateway", status: "checking", detail: "Checking…" },
    { name: "Pengine runtime", status: "checking", detail: "Checking…" },
    { name: "Ollama", status: "checking", detail: "Checking…" },
  ]);
  const [disconnectError, setDisconnectError] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    let botConnected = false;
    let botUser = botUsername ?? "unknown";
    let pengineUp = false;

    try {
      const resp = await fetch(`${PENGINE_API_BASE}/v1/health`, {
        signal: AbortSignal.timeout(3000),
      });
      if (resp.ok) {
        pengineUp = true;
        const data = await resp.json();
        botConnected = data.bot_connected;
        if (data.bot_username) botUser = data.bot_username;
      }
    } catch {
      // Pengine API not reachable (app stopped or wrong port)
    }

    let ollamaUp = false;
    let ollamaModel: string | null = null;
    try {
      // Check for a model currently loaded in memory
      const psResp = await fetch(`${OLLAMA_API_BASE}/api/ps`, {
        signal: AbortSignal.timeout(2000),
      });
      if (psResp.ok) {
        ollamaUp = true;
        const psData = await psResp.json();
        ollamaModel = psData.models?.[0]?.name ?? null;
      }
      // Fallback to first pulled model if nothing is loaded yet
      if (!ollamaModel) {
        const tagsResp = await fetch(`${OLLAMA_API_BASE}/api/tags`, {
          signal: AbortSignal.timeout(2000),
        });
        if (tagsResp.ok) {
          ollamaUp = true;
          const tagsData = await tagsResp.json();
          ollamaModel = tagsData.models?.[0]?.name ?? null;
        }
      }
    } catch {
      // Ollama not reachable
    }

    setServices([
      {
        name: "Telegram gateway",
        status: botConnected ? "running" : "stopped",
        detail: botConnected ? `@${botUser} long poll active` : "Not connected",
      },
      {
        name: "Pengine runtime",
        status: pengineUp ? "running" : "stopped",
        detail: pengineUp
          ? `${PENGINE_API_BASE.replace(/^https?:\/\//, "")} reachable`
          : "App not running",
      },
      {
        name: "Ollama",
        status: ollamaUp ? "running" : "stopped",
        detail: ollamaUp
          ? ollamaModel
            ? `model: ${ollamaModel}`
            : "Running, no model loaded"
          : "Not reachable",
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

  return (
    <div className="relative overflow-x-clip pb-20">
      <TopMenu />

      <main className="section-shell pt-10">
        <section className="max-w-4xl">
          <p className="mono-label">Dashboard</p>
          <h1 className="mt-3 text-5xl font-extrabold leading-tight tracking-tight text-white">
            Connected device and running services
          </h1>
          <p className="mt-5 max-w-3xl subtle-copy">
            The Pengine desktop app is running the bot service. Messages from
            Telegram are handled locally even when this page is closed.
          </p>
        </section>

        <section className="mt-10 grid gap-6 lg:grid-cols-[1.15fr_0.85fr]">
          <div className="space-y-6">
            <div className="panel p-5">
              <p className="mono-label">Services</p>
              <div className="mt-4 grid gap-3">
                {services.map((service) => (
                  <div
                    key={service.name}
                    className="flex items-center justify-between gap-4 rounded-2xl border border-white/10 bg-white/5 px-4 py-3"
                  >
                    <div>
                      <p className="text-sm font-semibold text-white">{service.name}</p>
                      <p className="mt-1 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
                        {service.detail}
                      </p>
                    </div>
                    <span
                      className={`rounded-full border px-3 py-1 font-mono text-[11px] uppercase tracking-[0.14em] ${
                        service.status === "running"
                          ? "border-emerald-300/30 bg-emerald-300/10 text-emerald-200"
                          : service.status === "stopped"
                            ? "border-rose-300/30 bg-rose-300/10 text-rose-200"
                            : "border-yellow-300/30 bg-yellow-300/10 text-yellow-200"
                      }`}
                    >
                      {service.status}
                    </span>
                  </div>
                ))}
              </div>
            </div>

            <TerminalPreview />
          </div>

          <div className="grid gap-6">
            <div className="panel rounded-4xl p-6">
              <p className="mono-label">Device session</p>
              {isDeviceConnected ? (
                <>
                  <p className="mt-3 text-lg font-semibold text-white">1 connected device</p>
                  <p className="mt-2 subtle-copy">
                    Telegram messaging is active and local runtime services are
                    available.
                  </p>
                </>
              ) : (
                <>
                  <p className="mt-3 text-lg font-semibold text-white">No device connected</p>
                  <p className="mt-2 subtle-copy">
                    Run through the setup wizard to connect your Telegram bot.
                  </p>
                  <Link
                    to="/setup"
                    className="primary-button mt-5 inline-block rounded-xl px-5 py-2 text-xs"
                  >
                    Go to setup
                  </Link>
                </>
              )}
            </div>

            {isDeviceConnected && (
              <div className="panel p-6">
                <p className="mono-label">Controls</p>
                <p className="mt-3 subtle-copy">
                  Disconnect the current device session and return to setup.
                </p>
                {disconnectError && (
                  <p className="mt-3 font-mono text-xs text-rose-300">{disconnectError}</p>
                )}
                <button
                  type="button"
                  className="secondary-button mt-5 w-full rounded-xl border-rose-300/30 bg-rose-300/10 text-rose-100 hover:bg-rose-300/15"
                  onClick={handleDisconnect}
                >
                  Disconnect device
                </button>
              </div>
            )}
          </div>
        </section>
      </main>
    </div>
  );
}
