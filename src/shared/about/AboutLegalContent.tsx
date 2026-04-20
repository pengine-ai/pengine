const CONTACT = {
  x: { label: "X (Twitter)", href: "https://x.com/PengineAI" },
  github: { label: "GitHub", href: "https://github.com/pengine-ai/pengine" },
} as const;

type Props = {
  /** Shown on the Settings page when the local app reports a version. */
  appVersion?: string | null;
};

export function AboutLegalContent({ appVersion }: Props) {
  return (
    <div className="grid gap-10">
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
              Pengine is a <strong className="font-semibold text-slate-200">desktop app</strong>{" "}
              that runs an agent loop on your machine. There is no Pengine-hosted account or cloud
              database for your chats: ordinary use keeps configuration and skills on your device,
              while the bot and tools you enable determine what leaves it.
            </p>
          </div>
          <div>
            <h3 className="font-mono text-xs font-semibold uppercase tracking-[0.14em] text-slate-300">
              Analytics and telemetry
            </h3>
            <p className="mt-2">
              The app does not include first-party analytics or behavioral tracking, and it does not
              phone home to a Pengine telemetry service. The dashboard may{" "}
              <strong className="font-semibold text-slate-200">check GitHub’s public API</strong>{" "}
              for new releases (version metadata only), which is subject to GitHub’s own policies
              and your network path.
            </p>
          </div>
          <div>
            <h3 className="font-mono text-xs font-semibold uppercase tracking-[0.14em] text-slate-300">
              What stays on your device
            </h3>
            <p className="mt-2">
              Pengine stores app data under your OS app-data location: connection metadata, skills,
              MCP configuration, and UI-related settings files. Sensitive values such as your{" "}
              <strong className="font-semibold text-slate-200">Telegram bot token</strong> and MCP
              secrets you configure are kept in the{" "}
              <strong className="font-semibold text-slate-200">platform secure store</strong> (for
              example Keychain on macOS), not in a Pengine cloud.
            </p>
          </div>
          <div>
            <h3 className="font-mono text-xs font-semibold uppercase tracking-[0.14em] text-slate-300">
              Network, Telegram, and inference
            </h3>
            <p className="mt-2">
              When your bot is connected, message traffic uses{" "}
              <strong className="font-semibold text-slate-200">Telegram’s services</strong> per
              Telegram’s terms. The app sends prompts and tool activity to{" "}
              <strong className="font-semibold text-slate-200">
                Ollama (or another endpoint you configure)
              </strong>
              — typically on your LAN or localhost, but cloud or remote models are possible if you
              point the stack there. Any{" "}
              <strong className="font-semibold text-slate-200">
                MCP servers, containers, or custom tools
              </strong>{" "}
              you add can read or forward data according to their own configuration; treat them like
              any other software you install.
            </p>
          </div>
          <div>
            <h3 className="font-mono text-xs font-semibold uppercase tracking-[0.14em] text-slate-300">
              Responsibility
            </h3>
            <p className="mt-2">
              You choose the bot, models, skills, and tools. This policy describes how Pengine is
              designed to run locally; it does not replace Telegram’s, your host OS’s, or your
              inference provider’s policies. For exact storage and startup behavior, see the project
              documentation in the repository.
            </p>
          </div>
        </div>
      </section>

      <section data-testid="settings-about-app">
        <p className="mono-label">About the app</p>
        <p className="mt-3 subtle-copy">
          Pengine is a local AI agent runtime: it connects your Telegram bot to Ollama (or
          compatible inference) so conversations and tools run on your hardware. The project is open
          source; see the GitHub repository above for license and source code.
        </p>
        {appVersion ? (
          <p className="mt-4 font-mono text-[11px] text-white/45">
            Reported app version: v{appVersion}
          </p>
        ) : null}
      </section>
    </div>
  );
}
