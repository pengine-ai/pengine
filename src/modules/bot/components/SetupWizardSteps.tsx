import { useMemo, useState } from "react";
import { OLLAMA_API_BASE } from "../../../shared/api/config";
import { PENGINE } from "../api";
import { StyledQrCode } from "../../../shared/ui/StyledQrCode";
import type { RuntimeStatus } from "../../toolengine";

type TokenStatus = "idle" | "valid" | "typing";

export function WizardStepCreateBot(props: {
  botToken: string;
  onBotTokenChange: (value: string) => void;
  status: TokenStatus;
  tokenStatusMessage: (status: TokenStatus) => string;
}) {
  const { botToken, onBotTokenChange, status, tokenStatusMessage } = props;
  const [showToken, setShowToken] = useState(false);
  return (
    <div className="grid gap-8 lg:grid-cols-[1.05fr_0.95fr]">
      <div className="space-y-5">
        <div>
          <p className="mono-label">Step 1</p>
          <h2 className="mt-2 text-3xl font-extrabold text-white">Create your Telegram bot</h2>
          <p className="mt-3 subtle-copy">
            Open BotFather, create a new bot, then paste the token here.
          </p>
        </div>
        <a
          className="primary-button w-fit rounded-xl px-5 py-3 text-xs"
          href="https://t.me/botfather"
          target="_blank"
          rel="noreferrer"
        >
          Open BotFather
        </a>
        <div className="rounded-2xl border border-white/10 bg-white/5 p-4">
          <label
            htmlFor="token"
            className="font-mono text-xs uppercase tracking-[0.14em] text-(--mid)"
          >
            Bot token
          </label>
          <input
            id="token"
            type={showToken ? "text" : "password"}
            autoComplete="off"
            spellCheck={false}
            className="mt-3 w-full rounded-xl border border-white/10 bg-slate-950/70 px-4 py-3 text-slate-100 outline-none placeholder:text-(--dim) focus:border-cyan-300/40"
            value={botToken}
            onChange={(event) => onBotTokenChange(event.target.value)}
            placeholder="1234567890:ABCdefGHIjklMNOpqrSTUvwxYZ-abc123..."
          />
          <button
            type="button"
            onClick={() => setShowToken((v) => !v)}
            className="mt-2 font-mono text-[10px] uppercase tracking-wider text-(--mid) underline decoration-white/20 underline-offset-4 hover:text-slate-300"
          >
            {showToken ? "Hide token" : "Show token"}
          </button>
          <div className="mt-3 flex items-center gap-3 text-sm">
            <span
              className={`h-2.5 w-2.5 rounded-full ${
                status === "valid"
                  ? "status-pulse bg-emerald-300"
                  : status === "typing"
                    ? "bg-yellow-300"
                    : "bg-slate-500"
              }`}
            />
            <p className="text-(--mid)">{tokenStatusMessage(status)}</p>
          </div>
        </div>
      </div>
      <div className="panel rounded-4xl p-5">
        <p className="mono-label">Why</p>
        <p className="mt-4 text-sm text-slate-200">
          The token encodes your <strong className="text-slate-100">bot ID</strong>. Pengine uses
          that ID to pair with your bot automatically.
        </p>
      </div>
    </div>
  );
}

export function WizardStepOllama(props: {
  ollamaChecking: boolean;
  ollamaReachable: boolean | null;
  ollamaModel: string | null;
  onRetry: () => void;
}) {
  const { ollamaChecking, ollamaReachable, ollamaModel, onRetry } = props;
  return (
    <div className="grid gap-8 lg:grid-cols-[1fr_1fr]">
      <div>
        <p className="mono-label">Step 2</p>
        <h2 className="mt-2 text-3xl font-extrabold text-white">Install Ollama</h2>
        <p className="mt-3 subtle-copy">
          Ollama runs AI models on your machine. Install it and pull a model before continuing.
        </p>
        <pre className="mt-5 overflow-x-auto rounded-2xl border border-white/10 bg-slate-950/70 p-4 font-mono text-sm text-emerald-200">
          <code>{`curl -fsSL https://ollama.com/install.sh | sh
ollama pull qwen3:8b`}</code>
        </pre>
        <p className="mt-2 text-xs text-white/40">
          Recommended: <span className="text-white/60">qwen3:8b</span> — good balance of speed and
          tool-calling support.
        </p>

        {ollamaChecking && (
          <p className="mt-4 font-mono text-xs text-yellow-300">Detecting Ollama…</p>
        )}

        {ollamaReachable === true && ollamaModel && (
          <div className="mt-4 rounded-2xl border border-emerald-300/20 bg-emerald-300/10 px-4 py-3">
            <p className="font-mono text-xs text-emerald-300">Ollama detected — active model:</p>
            <p className="mt-1 text-lg font-semibold text-white">{ollamaModel}</p>
          </div>
        )}

        {ollamaReachable === true && !ollamaModel && (
          <p className="mt-4 font-mono text-xs text-yellow-300">
            Ollama is running but no model is pulled yet. Run{" "}
            <code className="text-slate-200">ollama pull qwen3:8b</code> first.
          </p>
        )}

        {ollamaReachable === false && (
          <div className="mt-4 space-y-2">
            <p className="font-mono text-xs text-rose-300">
              Could not reach Ollama at {OLLAMA_API_BASE}. Make sure it&apos;s installed and
              running.
            </p>
            <button
              type="button"
              className="secondary-button w-full max-w-md rounded-xl text-xs"
              onClick={onRetry}
            >
              Retry detection
            </button>
          </div>
        )}

        {ollamaModel && ollamaReachable === true && (
          <p className="mt-3 font-mono text-xs text-emerald-300">Ready to continue.</p>
        )}
      </div>
      <div className="rounded-2xl border border-emerald-300/20 bg-emerald-300/10 p-5">
        <p className="font-mono text-xs uppercase tracking-[0.14em] text-emerald-200">
          Ollama status
        </p>
        <ul className="mt-3 list-inside list-disc space-y-2 text-sm text-slate-100">
          <li>
            Connection:{" "}
            <span
              className={
                ollamaReachable
                  ? "text-emerald-300"
                  : ollamaReachable === false
                    ? "text-rose-300"
                    : "text-slate-400"
              }
            >
              {ollamaReachable
                ? "reachable"
                : ollamaReachable === false
                  ? "not reachable"
                  : "checking…"}
            </span>
          </li>
          <li>
            Active model:{" "}
            <span className={ollamaModel ? "text-emerald-300" : "text-slate-400"}>
              {ollamaModel ?? "none detected"}
            </span>
          </li>
        </ul>
      </div>
    </div>
  );
}

export function WizardStepContainerRuntime(props: {
  runtimeChecking: boolean;
  runtimeStatus: RuntimeStatus | null;
  onRetry: () => void;
}) {
  const { runtimeChecking, runtimeStatus, onRetry } = props;
  return (
    <div className="grid gap-8 lg:grid-cols-[1fr_1fr]">
      <div>
        <p className="mono-label">Step 3</p>
        <h2 className="mt-2 text-3xl font-extrabold text-white">Install a container runtime</h2>
        <p className="mt-3 subtle-copy">
          Pengine uses Podman (preferred) or Docker to run tools inside isolated, rootless
          containers. Install one of them before continuing.
        </p>

        <div className="mt-5 space-y-3">
          <div>
            <p className="font-mono text-xs uppercase tracking-[0.14em] text-(--mid)">
              Option A — Podman (recommended)
            </p>
            <pre className="mt-2 overflow-x-auto rounded-2xl border border-white/10 bg-slate-950/70 p-4 font-mono text-sm text-emerald-200">
              <code>{`# macOS
brew install podman
podman machine init
podman machine start

# Linux (Debian/Ubuntu)
sudo apt install podman`}</code>
            </pre>
          </div>

          <div>
            <p className="font-mono text-xs uppercase tracking-[0.14em] text-(--mid)">
              Option B — Docker
            </p>
            <pre className="mt-2 overflow-x-auto rounded-2xl border border-white/10 bg-slate-950/70 p-4 font-mono text-sm text-emerald-200">
              <code>{`# macOS / Linux
# Install Docker Desktop from https://docker.com/get-started
# or use the convenience script:
curl -fsSL https://get.docker.com | sh`}</code>
            </pre>
          </div>
        </div>

        <p className="mt-3 text-xs text-white/40">
          Podman is preferred because it runs{" "}
          <span className="text-white/60">rootless by default</span> — no daemon, no elevated
          privileges.
        </p>

        {runtimeChecking && (
          <p className="mt-4 font-mono text-xs text-yellow-300">Detecting container runtime…</p>
        )}

        {runtimeStatus?.available && (
          <div className="mt-4 rounded-2xl border border-emerald-300/20 bg-emerald-300/10 px-4 py-3">
            <p className="font-mono text-xs text-emerald-300">Container runtime detected:</p>
            <p className="mt-1 text-lg font-semibold text-white">
              {runtimeStatus.kind ?? "unknown"} {runtimeStatus.version ?? ""}
              {runtimeStatus.rootless ? " (rootless)" : ""}
            </p>
          </div>
        )}

        {runtimeStatus && !runtimeStatus.available && (
          <div className="mt-4 space-y-2">
            <p className="font-mono text-xs text-rose-300">
              No container runtime found. Install Podman or Docker and make sure it&apos;s running.
            </p>
            <button
              type="button"
              className="secondary-button w-full max-w-md rounded-xl text-xs"
              onClick={onRetry}
            >
              Retry detection
            </button>
          </div>
        )}

        {runtimeStatus?.available && (
          <p className="mt-3 font-mono text-xs text-emerald-300">Ready to continue.</p>
        )}
      </div>

      <div className="rounded-2xl border border-emerald-300/20 bg-emerald-300/10 p-5">
        <p className="font-mono text-xs uppercase tracking-[0.14em] text-emerald-200">
          Runtime status
        </p>
        <ul className="mt-3 list-inside list-disc space-y-2 text-sm text-slate-100">
          <li>
            Engine:{" "}
            <span className={runtimeStatus?.available ? "text-emerald-300" : "text-slate-400"}>
              {runtimeStatus?.available
                ? (runtimeStatus.kind ?? "unknown")
                : runtimeChecking
                  ? "checking…"
                  : "not detected"}
            </span>
          </li>
          <li>
            Version:{" "}
            <span className={runtimeStatus?.version ? "text-emerald-300" : "text-slate-400"}>
              {runtimeStatus?.version?.trim() || "—"}
            </span>
          </li>
          <li>
            Rootless:{" "}
            <span className={runtimeStatus?.rootless ? "text-emerald-300" : "text-slate-400"}>
              {runtimeStatus?.available ? (runtimeStatus.rootless ? "yes" : "no") : "—"}
            </span>
          </li>
        </ul>

        <div className="mt-5 border-t border-emerald-300/10 pt-4">
          <p className="font-mono text-xs uppercase tracking-[0.14em] text-emerald-200">
            Why containers?
          </p>
          <p className="mt-2 text-sm text-slate-100">
            The Tool Engine runs each tool inside an isolated container with no network access,
            read-only filesystem, and strict resource limits. This keeps your system safe even when
            the AI agent executes external tools.
          </p>
        </div>
      </div>
    </div>
  );
}

export function WizardStepPengineLocal(props: {
  pengineChecking: boolean;
  pengineReachable: boolean | null;
  onRetry: () => void;
}) {
  const { pengineChecking, pengineReachable, onRetry } = props;
  return (
    <div className="grid gap-8 lg:grid-cols-[1fr_1fr]">
      <div>
        <p className="mono-label">Step 4</p>
        <h2 className="mt-2 text-3xl font-extrabold text-white">Start Pengine locally</h2>
        <p className="mt-3 subtle-copy">
          The Pengine desktop app must be running on this machine. It hosts the bot service on
          localhost so messages keep flowing even after you close this browser tab.
        </p>
        <div className="mt-5 rounded-2xl border border-white/10 bg-white/5 p-4 font-mono text-xs text-(--mid)">
          <p>
            Checking <code className="text-slate-300">{PENGINE.health}</code>…
          </p>
        </div>
        {pengineChecking && <p className="mt-3 font-mono text-xs text-yellow-300">Checking…</p>}
        {pengineReachable === true && (
          <p className="mt-3 font-mono text-xs text-emerald-300">
            Pengine is running on localhost.
          </p>
        )}
        {pengineReachable === false && (
          <div className="mt-3 space-y-2">
            <p className="font-mono text-xs text-rose-300">
              Could not reach Pengine. Start the desktop app and retry.
            </p>
            <button
              type="button"
              className="secondary-button w-full max-w-md rounded-xl text-xs"
              onClick={onRetry}
            >
              Retry health check
            </button>
          </div>
        )}
      </div>
      <div className="rounded-2xl border border-cyan-300/20 bg-cyan-300/10 p-5">
        <p className="font-mono text-xs uppercase tracking-[0.14em] text-cyan-200">
          What happens next
        </p>
        <p className="mt-3 text-sm text-slate-100">
          The next step hands off your bot token to the local Pengine process. The bot will start
          polling Telegram automatically.
        </p>
      </div>
    </div>
  );
}

export function WizardStepConnect(props: {
  botId: string | null;
  status: TokenStatus;
  ollamaModel: string | null;
  runtimeStatus: RuntimeStatus | null;
  pengineReachable: boolean | null;
  connectStatus: "idle" | "connecting" | "connected" | "error";
  connectError: string;
  verifiedBot: { bot_id: string; bot_username: string } | null;
  botUsername: string;
  onBotUsernameChange: (value: string) => void;
  onConnect: () => void;
  onCopyUri: () => void;
  copiedUri: boolean;
  onCompleteSetup?: () => void;
}) {
  const {
    botId,
    status,
    ollamaModel,
    runtimeStatus,
    pengineReachable,
    connectStatus,
    connectError,
    verifiedBot,
    botUsername,
    onBotUsernameChange,
    onConnect,
    onCopyUri,
    copiedUri,
    onCompleteSetup,
  } = props;

  const telegramBotUrl = useMemo(() => {
    const fromInput = botUsername.replace(/^@+/, "").trim();
    const fromVerified = verifiedBot?.bot_username.replace(/^@+/, "").trim() ?? "";
    const name = fromInput || fromVerified;
    return name ? `https://t.me/${name}` : "https://t.me/botfather";
  }, [botUsername, verifiedBot]);

  return (
    <div className="grid gap-8 lg:grid-cols-[1fr_1fr]">
      <div>
        <p className="mono-label">Step 5</p>
        <h2 className="mt-2 text-3xl font-extrabold text-white">Connect bot to Pengine</h2>
        <p className="mt-3 subtle-copy">
          Send your bot token to the local Pengine service. It will verify the token with Telegram
          and start listening for messages.
        </p>
        <div className="mt-5 rounded-2xl border border-white/10 bg-slate-950/60 p-4 font-mono text-sm text-slate-100">
          <p>
            Bot ID: <span className="text-(--yellow)">{botId ?? "— paste token in step 1"}</span>
          </p>
        </div>

        {connectStatus === "idle" && (
          <button
            type="button"
            data-testid="connect-to-pengine"
            className="primary-button mt-6 w-full max-w-md rounded-xl text-xs"
            onClick={onConnect}
          >
            Connect to Pengine
          </button>
        )}
        {connectStatus === "connecting" && (
          <p className="mt-4 font-mono text-xs text-yellow-300">Verifying token with Telegram…</p>
        )}
        {connectStatus === "error" && (
          <div className="mt-4 space-y-2">
            <p className="font-mono text-xs text-rose-300">{connectError}</p>
            <button
              type="button"
              className="secondary-button w-full max-w-md rounded-xl text-xs"
              onClick={onConnect}
            >
              Retry connection
            </button>
          </div>
        )}
        {connectStatus === "connected" && verifiedBot && (
          <div className="mt-4 space-y-3">
            <p className="font-mono text-xs text-emerald-300">
              Connected as @{verifiedBot.bot_username} (ID: {verifiedBot.bot_id})
            </p>
            <div className="mt-4 rounded-2xl border border-white/10 bg-white/5 p-4">
              <label
                htmlFor="bot-username-connect"
                className="font-mono text-xs uppercase tracking-[0.14em] text-(--mid)"
              >
                Bot username (for QR link)
              </label>
              <input
                id="bot-username-connect"
                className="mt-3 w-full rounded-xl border border-white/10 bg-slate-950/70 px-4 py-3 text-slate-100 outline-none placeholder:text-(--dim) focus:border-cyan-300/40"
                value={botUsername || verifiedBot.bot_username}
                onChange={(event) => onBotUsernameChange(event.target.value)}
                placeholder="@YourPengineBot"
              />
            </div>
            <div className="mt-4 flex justify-center rounded-3xl border border-white/10 bg-white p-5">
              <StyledQrCode value={telegramBotUrl} size={208} />
            </div>
            <p className="text-center font-mono text-[11px] text-(--dim)">
              Scan to open your bot in Telegram
            </p>
          </div>
        )}

        <div className="mt-6">
          <button
            type="button"
            className="font-mono text-[11px] text-(--dim) underline decoration-white/20 underline-offset-4 hover:text-slate-300"
            onClick={onCopyUri}
          >
            {copiedUri ? "Copied!" : "Copy connection command (curl)"}
          </button>
        </div>
      </div>
      <div className="space-y-4">
        <div className="rounded-2xl border border-white/10 bg-white/5 p-5">
          <p className="font-mono text-xs uppercase tracking-[0.14em] text-(--mid)">Direct link</p>
          <a
            href={telegramBotUrl}
            target="_blank"
            rel="noreferrer"
            className="mt-3 inline-flex break-all font-mono text-xs text-cyan-200"
          >
            {telegramBotUrl}
          </a>
        </div>
        <div className="rounded-3xl border border-emerald-300/20 bg-emerald-300/10 p-5">
          <div className="space-y-3 font-mono text-sm text-slate-100">
            <p>{status === "valid" ? "✓" : "○"} Bot token saved</p>
            <p>{ollamaModel ? "✓" : "○"} Ollama ready</p>
            <p>{runtimeStatus?.available ? "✓" : "○"} Container runtime</p>
            <p>{pengineReachable ? "✓" : "○"} Pengine running</p>
            <p>{connectStatus === "connected" ? "✓" : "○"} Bot connected</p>
          </div>
          {connectStatus === "connected" && onCompleteSetup && (
            <button
              type="button"
              className="primary-button mt-5 w-full rounded-xl text-xs"
              onClick={() => onCompleteSetup()}
            >
              Open dashboard
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
