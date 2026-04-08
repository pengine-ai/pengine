import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { TerminalPreview } from "../components/TerminalPreview";
import { TopMenu } from "../components/TopMenu";
import { useAppSessionStore } from "../stores/appSessionStore";

const PENGINE_API = "http://127.0.0.1:21516";

type ServiceInfo = {
  name: string;
  status: "running" | "stopped" | "checking";
  detail: string;
};

export function DashboardPage() {
  const navigate = useNavigate();
  const disconnectDevice = useAppSessionStore((state) => state.disconnectDevice);
  const botUsername = useAppSessionStore((state) => state.botUsername);
  const [services, setServices] = useState<ServiceInfo[]>([
    { name: "Telegram gateway", status: "checking", detail: "Checking…" },
    { name: "Pengine runtime", status: "checking", detail: "Checking…" },
    { name: "Ollama", status: "checking", detail: "Checking…" },
  ]);

  const refreshStatus = useCallback(async () => {
    let botConnected = false;
    let botUser = botUsername ?? "unknown";
    let pengineUp = false;

    try {
      const resp = await fetch(`${PENGINE_API}/v1/health`, {
        signal: AbortSignal.timeout(3000),
      });
      if (resp.ok) {
        pengineUp = true;
        const data = await resp.json();
        botConnected = data.bot_connected;
        if (data.bot_username) botUser = data.bot_username;
      }
    } catch {
      // not reachable
    }

    let ollamaUp = false;
    try {
      const resp = await fetch("http://localhost:11434/api/tags", {
        signal: AbortSignal.timeout(2000),
      });
      ollamaUp = resp.ok;
    } catch {
      // not reachable
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
        detail: pengineUp ? "localhost:21516 reachable" : "App not running",
      },
      {
        name: "Ollama",
        status: ollamaUp ? "running" : "stopped",
        detail: ollamaUp ? "localhost:11434 reachable" : "Not reachable",
      },
    ]);
  }, [botUsername]);

  useEffect(() => {
    refreshStatus();
    const timer = setInterval(refreshStatus, 10000);
    return () => clearInterval(timer);
  }, [refreshStatus]);

  const handleDisconnect = async () => {
    await disconnectDevice();
    navigate("/setup", { replace: true });
  };

  return (
    <div className="relative overflow-x-hidden pb-20">
      <TopMenu ctaLabel="Project overview" ctaTo="/" showNavigationLinks={false} />

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
              <p className="mt-3 text-lg font-semibold text-white">1 connected device</p>
              <p className="mt-2 subtle-copy">
                Telegram messaging is active and local runtime services are
                available.
              </p>
            </div>

            <div className="panel p-6">
              <p className="mono-label">Controls</p>
              <p className="mt-3 subtle-copy">
                Disconnect the current device session and return to setup.
              </p>
              <button
                type="button"
                className="secondary-button mt-5 w-full rounded-xl border-rose-300/30 bg-rose-300/10 text-rose-100 hover:bg-rose-300/15"
                onClick={handleDisconnect}
              >
                Disconnect device
              </button>
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}
