import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { getPengineHealth } from "../modules/bot/api";
import { useAppSessionStore } from "../modules/bot/store/appSessionStore";
import { TopMenu } from "../shared/ui/TopMenu";

type SettingsTab = "preferences" | "about";

const CONTACT = {
  x: { label: "X (Twitter)", href: "https://x.com/PengineAI" },
  github: { label: "GitHub", href: "https://github.com/pengine-ai/pengine" },
} as const;

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
              <section data-testid="settings-about-contact">
                <p className="mono-label">Contact info</p>
                <p className="mt-3 subtle-copy">
                  Project links and maintainer contact on social platforms.
                </p>
                <ul className="mt-4 grid gap-2 font-mono text-sm">
                  <li>
                    <a
                      href={CONTACT.x.href}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-cyan-200/90 underline decoration-cyan-200/30 underline-offset-2 transition hover:decoration-cyan-200/80"
                    >
                      {CONTACT.x.label}
                    </a>
                  </li>
                  <li>
                    <a
                      href={CONTACT.github.href}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-cyan-200/90 underline decoration-cyan-200/30 underline-offset-2 transition hover:decoration-cyan-200/80"
                    >
                      {CONTACT.github.label}
                    </a>
                  </li>
                </ul>
              </section>

              <section data-testid="settings-about-privacy">
                <p className="mono-label">Privacy info</p>
                <div className="mt-4 space-y-6 subtle-copy">
                  <div>
                    <h2 className="font-mono text-sm font-semibold uppercase tracking-[0.12em] text-slate-200">
                      Privacy Policy
                    </h2>
                    <p className="mt-2">
                      Pengine is a{" "}
                      <strong className="font-semibold text-slate-200">desktop app</strong> that
                      runs an agent loop on your machine. There is no Pengine-hosted account or
                      cloud database for your chats: ordinary use keeps configuration and skills on
                      your device, while the bot and tools you enable determine what leaves it.
                    </p>
                  </div>
                  <div>
                    <h3 className="font-mono text-xs font-semibold uppercase tracking-[0.14em] text-slate-300">
                      Analytics and telemetry
                    </h3>
                    <p className="mt-2">
                      The app does not include first-party analytics or behavioral tracking, and it
                      does not phone home to a Pengine telemetry service. The dashboard may{" "}
                      <strong className="font-semibold text-slate-200">
                        check GitHub’s public API
                      </strong>{" "}
                      for new releases (version metadata only), which is subject to GitHub’s own
                      policies and your network path.
                    </p>
                  </div>
                  <div>
                    <h3 className="font-mono text-xs font-semibold uppercase tracking-[0.14em] text-slate-300">
                      What stays on your device
                    </h3>
                    <p className="mt-2">
                      Pengine stores app data under your OS app-data location: connection metadata,
                      skills, MCP configuration, and UI-related settings files. Sensitive values
                      such as your{" "}
                      <strong className="font-semibold text-slate-200">Telegram bot token</strong>{" "}
                      and MCP secrets you configure are kept in the{" "}
                      <strong className="font-semibold text-slate-200">
                        platform secure store
                      </strong>{" "}
                      (for example Keychain on macOS), not in a Pengine cloud.
                    </p>
                  </div>
                  <div>
                    <h3 className="font-mono text-xs font-semibold uppercase tracking-[0.14em] text-slate-300">
                      Network, Telegram, and inference
                    </h3>
                    <p className="mt-2">
                      When your bot is connected, message traffic uses{" "}
                      <strong className="font-semibold text-slate-200">Telegram’s services</strong>{" "}
                      per Telegram’s terms. The app sends prompts and tool activity to{" "}
                      <strong className="font-semibold text-slate-200">
                        Ollama (or another endpoint you configure)
                      </strong>
                      — typically on your LAN or localhost, but cloud or remote models are possible
                      if you point the stack there. Any{" "}
                      <strong className="font-semibold text-slate-200">
                        MCP servers, containers, or custom tools
                      </strong>{" "}
                      you add can read or forward data according to their own configuration; treat
                      them like any other software you install.
                    </p>
                  </div>
                  <div>
                    <h3 className="font-mono text-xs font-semibold uppercase tracking-[0.14em] text-slate-300">
                      Responsibility
                    </h3>
                    <p className="mt-2">
                      You choose the bot, models, skills, and tools. This policy describes how
                      Pengine is designed to run locally; it does not replace Telegram’s, your host
                      OS’s, or your inference provider’s policies. For exact storage and startup
                      behavior, see the project documentation in the repository.
                    </p>
                  </div>
                </div>
              </section>

              <section data-testid="settings-about-app">
                <p className="mono-label">About the app</p>
                <p className="mt-3 subtle-copy">
                  Pengine is a local AI agent runtime: it connects your Telegram bot to Ollama (or
                  compatible inference) so conversations and tools run on your hardware. The project
                  is open source; see the GitHub repository above for license and source code.
                </p>
                {appVersion && (
                  <p className="mt-4 font-mono text-[11px] text-white/45">
                    Reported app version: v{appVersion}
                  </p>
                )}
              </section>
            </div>
          )}
        </div>
      </main>
    </div>
  );
}
