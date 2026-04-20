import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { getPengineHealth } from "../modules/bot/api";
import { useAppSessionStore } from "../modules/bot/store/appSessionStore";
import { AboutLegalContent } from "../shared/about/AboutLegalContent";
import { TopMenu } from "../shared/ui/TopMenu";

type SettingsTab = "preferences" | "about";

export function SettingsPage() {
  const isDeviceConnected = useAppSessionStore((s) => s.isDeviceConnected);
  const botUsername = useAppSessionStore((s) => s.botUsername);
  const [tab, setTab] = useState<SettingsTab>("preferences");
  const [healthBot, setHealthBot] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string | null>(null);

  const refreshMeta = useCallback(async () => {
    const health = await getPengineHealth(3000);
    if (health?.bot_username) setHealthBot(health.bot_username);
    else setHealthBot(null);
    setAppVersion(health?.app_version ?? null);
  }, []);

  useEffect(() => {
    void refreshMeta();
  }, [refreshMeta]);

  const displayBot = healthBot ?? botUsername;

  const tabBarClass =
    "flex gap-1 rounded-xl border border-white/10 bg-white/5 p-0.5 sm:inline-flex sm:max-w-md";

  const tabBtn = (id: SettingsTab, label: string) => (
    <button
      key={id}
      id={`settings-tab-${id}`}
      type="button"
      role="tab"
      aria-selected={tab === id}
      onClick={() => setTab(id)}
      className={`flex-1 rounded-lg px-3 py-2 font-mono text-[11px] uppercase tracking-[0.12em] transition sm:flex-none sm:px-4 ${
        tab === id
          ? "border border-cyan-300/25 bg-cyan-300/10 text-cyan-100"
          : "border border-transparent text-(--mid) hover:border-white/10 hover:bg-white/5 hover:text-slate-100"
      }`}
    >
      {label}
    </button>
  );

  return (
    <div className="relative overflow-x-clip pb-20">
      <TopMenu />

      <main className="section-shell pt-6 sm:pt-10">
        <div className="flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <p className="mono-label">User &amp; profile</p>
            <h1 className="mt-2 text-2xl font-extrabold text-white sm:text-3xl">Settings</h1>
            <p className="mt-2 max-w-xl subtle-copy">
              Preferences for this install and compliance information for the app.
            </p>
          </div>
          <Link
            to="/dashboard"
            className="secondary-button self-start rounded-xl px-4 py-2 text-xs sm:self-auto"
          >
            Back to dashboard
          </Link>
        </div>

        <div className={`mt-8 ${tabBarClass}`} role="tablist" aria-label="Settings sections">
          {tabBtn("preferences", "Preferences")}
          {tabBtn("about", "About")}
        </div>

        <div className="mt-6 panel p-6 sm:p-8">
          {tab === "preferences" && (
            <div className="grid gap-6" role="tabpanel" aria-labelledby="settings-tab-preferences">
              <div>
                <p className="mono-label">Session</p>
                <p className="mt-3 subtle-copy">
                  {isDeviceConnected && displayBot ? (
                    <>
                      Connected Telegram bot:{" "}
                      <span className="text-cyan-200/90">@{displayBot}</span>
                    </>
                  ) : (
                    "No Telegram bot is connected in this UI session. Use setup to connect."
                  )}
                </p>
                <div className="mt-4 flex flex-wrap gap-2">
                  <Link
                    to="/setup"
                    className="rounded-lg border border-white/15 bg-white/5 px-3 py-1.5 font-mono text-[11px] text-white/80 transition hover:bg-white/10 hover:text-white"
                  >
                    Open setup
                  </Link>
                  <Link
                    to="/dashboard"
                    className="rounded-lg border border-white/15 bg-white/5 px-3 py-1.5 font-mono text-[11px] text-white/80 transition hover:bg-white/10 hover:text-white"
                  >
                    Dashboard
                  </Link>
                </div>
              </div>

              <div>
                <p className="mono-label">Runtime preferences</p>
                <ul className="mt-3 list-inside list-disc space-y-2 subtle-copy marker:text-(--dim)">
                  <li>
                    <strong className="font-semibold text-slate-200">Ollama model</strong> — choose
                    the preferred model from the selector in the dashboard header.
                  </li>
                  <li>
                    <strong className="font-semibold text-slate-200">Skills &amp; context</strong> —
                    manage skill templates and the skills context size limit in the Skills panel on
                    the dashboard.
                  </li>
                </ul>
              </div>
            </div>
          )}

          {tab === "about" && (
            <div className="grid gap-10" role="tabpanel" aria-labelledby="settings-tab-about">
              <AboutLegalContent appVersion={appVersion} />
            </div>
          )}
        </div>
      </main>
    </div>
  );
}
