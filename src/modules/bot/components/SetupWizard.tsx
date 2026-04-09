import { useCallback, useEffect, useMemo, useState } from "react";
import { OLLAMA_API_BASE } from "../../../shared/api/config";
import { fetchOllamaModel } from "../../ollama/api";
import { getPengineHealth, PENGINE, postConnect } from "../api";
import { useAppSessionStore } from "../store/appSessionStore";
import { StyledQrCode } from "../../../shared/ui/StyledQrCode";
import { WizardLayout } from "../../../shared/ui/WizardLayout";

export const SETUP_STEPS = [
  {
    title: "Create bot",
    summary: "Create a Telegram bot with BotFather and save your bot token.",
    duration: "~1 min",
  },
  {
    title: "Install Ollama",
    summary: "Install Ollama on this machine so Pengine can run models locally.",
    duration: "~2 min",
  },
  {
    title: "Pengine local",
    summary: "Install and start the Pengine runtime on this computer.",
    duration: "~1 min",
  },
  {
    title: "Connect",
    summary: "Hand off your bot token to the local Pengine app.",
    duration: "~30 sec",
  },
] as const;

function parseBotIdFromToken(token: string): string | null {
  const trimmed = token.trim();
  const match = /^(\d{8,12}):/.exec(trimmed);
  return match ? match[1] : null;
}

function tokenStatus(token: string) {
  if (!token.trim()) return "idle";
  if (/^\d{8,12}:[A-Za-z0-9_-]{35,}$/.test(token.trim())) return "valid";
  return "typing";
}

function tokenStatusMessage(status: ReturnType<typeof tokenStatus>) {
  if (status === "valid") return "Token format looks valid. Continue when ready.";
  if (status === "typing") return "Token looks incomplete, keep going.";
  return "Waiting for your token.";
}

type SetupWizardProps = {
  onStepChange?: (step: number) => void;
  onCompleteSetup?: () => void;
};

export function SetupWizard({ onStepChange, onCompleteSetup }: SetupWizardProps) {
  const [step, setStep] = useState(0);
  const [botToken, setBotToken] = useState("");
  const [ollamaChecking, setOllamaChecking] = useState(false);
  const [ollamaModel, setOllamaModel] = useState<string | null>(null);
  const [ollamaReachable, setOllamaReachable] = useState<boolean | null>(null);
  const [pengineReachable, setPengineReachable] = useState<boolean | null>(null);
  const [pengineChecking, setPengineChecking] = useState(false);
  const [connectStatus, setConnectStatus] = useState<"idle" | "connecting" | "connected" | "error">(
    "idle",
  );
  const [connectError, setConnectError] = useState("");
  const [verifiedBot, setVerifiedBot] = useState<{
    bot_id: string;
    bot_username: string;
  } | null>(null);
  const [botUsername, setBotUsername] = useState("");
  const [copiedUri, setCopiedUri] = useState(false);

  const connectDevice = useAppSessionStore((s) => s.connectDevice);

  const status = useMemo(() => tokenStatus(botToken), [botToken]);
  const stepTitles = SETUP_STEPS.map((item) => item.title);
  const botId = useMemo(() => parseBotIdFromToken(botToken), [botToken]);

  const telegramBotUrl = useMemo(() => {
    const name = verifiedBot?.bot_username || botUsername.replace(/^@+/, "").trim();
    if (name) return `https://t.me/${name}`;
    return "https://t.me/botfather";
  }, [botUsername, verifiedBot]);

  const canContinueStep = useMemo(() => {
    if (step === 0) return status === "valid";
    if (step === 1) return !!ollamaModel;
    if (step === 2) return pengineReachable === true;
    if (step === 3) return connectStatus === "connected";
    return false;
  }, [step, status, ollamaModel, pengineReachable, connectStatus]);

  const canGoNext = step < stepTitles.length - 1 && canContinueStep;

  useEffect(() => {
    onStepChange?.(step);
  }, [onStepChange, step]);

  const checkOllama = useCallback(async () => {
    setOllamaChecking(true);
    setOllamaReachable(null);
    setOllamaModel(null);
    try {
      const { reachable, model } = await fetchOllamaModel(3000);
      setOllamaReachable(reachable);
      setOllamaModel(model);
    } finally {
      setOllamaChecking(false);
    }
  }, []);

  useEffect(() => {
    if (step === 1) {
      checkOllama();
    }
  }, [step, checkOllama]);

  const checkPengineHealth = useCallback(async () => {
    setPengineChecking(true);
    try {
      setPengineReachable((await getPengineHealth(3000)) !== null);
    } finally {
      setPengineChecking(false);
    }
  }, []);

  useEffect(() => {
    if (step === 2) {
      checkPengineHealth();
    }
  }, [step, checkPengineHealth]);

  const handleConnect = useCallback(async () => {
    setConnectStatus("connecting");
    setConnectError("");
    try {
      const { ok, data } = await postConnect(botToken);
      if (ok && data.bot_id && data.bot_username) {
        setConnectStatus("connected");
        setVerifiedBot({ bot_id: data.bot_id, bot_username: data.bot_username });
        connectDevice({ bot_username: data.bot_username, bot_id: data.bot_id });
      } else {
        setConnectStatus("error");
        setConnectError(data.error || "Connection failed");
      }
    } catch (err) {
      setConnectStatus("error");
      setConnectError(
        err instanceof Error ? err.message : "Could not reach Pengine. Is the app running?",
      );
    }
  }, [botToken, connectDevice]);

  const connectionUri = PENGINE.connect;
  const connectionPayload = JSON.stringify({ bot_token: botToken.trim() }, null, 2);

  const handleCopyUri = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(
        `curl -X POST ${connectionUri} -H "Content-Type: application/json" -d '${connectionPayload}'`,
      );
      setCopiedUri(true);
      setTimeout(() => setCopiedUri(false), 2000);
    } catch {
      /* clipboard not available */
    }
  }, [connectionUri, connectionPayload]);

  return (
    <WizardLayout
      stepTitles={stepTitles}
      activeStep={step}
      onBack={() => setStep((prev) => Math.max(0, prev - 1))}
      onNext={() => setStep((prev) => Math.min(stepTitles.length - 1, prev + 1))}
      onSelectStep={(index) => setStep(index)}
      canGoBack={step > 0}
      canGoNext={canGoNext}
    >
      {step === 0 && (
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
                className="mt-3 w-full rounded-xl border border-white/10 bg-slate-950/70 px-4 py-3 text-slate-100 outline-none placeholder:text-(--dim) focus:border-cyan-300/40"
                value={botToken}
                onChange={(event) => setBotToken(event.target.value)}
                placeholder="1234567890:ABCdefGHIjklMNOpqrSTUvwxYZ-abc123..."
              />
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
              The token encodes your <strong className="text-slate-100">bot ID</strong>. Pengine
              uses that ID to pair with your bot automatically.
            </p>
          </div>
        </div>
      )}

      {step === 1 && (
        <div className="grid gap-8 lg:grid-cols-[1fr_1fr]">
          <div>
            <p className="mono-label">Step 2</p>
            <h2 className="mt-2 text-3xl font-extrabold text-white">Install Ollama</h2>
            <p className="mt-3 subtle-copy">
              Ollama runs AI models on your machine. Install it and pull a model before continuing.
            </p>
            <pre className="mt-5 overflow-x-auto rounded-2xl border border-white/10 bg-slate-950/70 p-4 font-mono text-sm text-emerald-200">
              <code>{`curl -fsSL https://ollama.com/install.sh | sh
ollama pull llama3.2`}</code>
            </pre>

            {ollamaChecking && (
              <p className="mt-4 font-mono text-xs text-yellow-300">Detecting Ollama…</p>
            )}

            {ollamaReachable === true && ollamaModel && (
              <div className="mt-4 rounded-2xl border border-emerald-300/20 bg-emerald-300/10 px-4 py-3">
                <p className="font-mono text-xs text-emerald-300">
                  Ollama detected — active model:
                </p>
                <p className="mt-1 text-lg font-semibold text-white">{ollamaModel}</p>
              </div>
            )}

            {ollamaReachable === true && !ollamaModel && (
              <p className="mt-4 font-mono text-xs text-yellow-300">
                Ollama is running but no model is pulled yet. Run{" "}
                <code className="text-slate-200">ollama pull llama3.2</code> first.
              </p>
            )}

            {ollamaReachable === false && (
              <div className="mt-4 space-y-2">
                <p className="font-mono text-xs text-rose-300">
                  Could not reach Ollama at {OLLAMA_API_BASE}. Make sure it's installed and running.
                </p>
                <button
                  type="button"
                  className="secondary-button w-full max-w-md rounded-xl text-xs"
                  onClick={checkOllama}
                >
                  Retry detection
                </button>
              </div>
            )}

            {ollamaModel && (
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
      )}

      {step === 2 && (
        <div className="grid gap-8 lg:grid-cols-[1fr_1fr]">
          <div>
            <p className="mono-label">Step 3</p>
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
                  onClick={checkPengineHealth}
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
              The next step hands off your bot token to the local Pengine process. The bot will
              start polling Telegram automatically.
            </p>
          </div>
        </div>
      )}

      {step === 3 && (
        <div className="grid gap-8 lg:grid-cols-[1fr_1fr]">
          <div>
            <p className="mono-label">Step 4</p>
            <h2 className="mt-2 text-3xl font-extrabold text-white">Connect bot to Pengine</h2>
            <p className="mt-3 subtle-copy">
              Send your bot token to the local Pengine service. It will verify the token with
              Telegram and start listening for messages.
            </p>
            <div className="mt-5 rounded-2xl border border-white/10 bg-slate-950/60 p-4 font-mono text-sm text-slate-100">
              <p>
                Bot ID:{" "}
                <span className="text-(--yellow)">{botId ?? "— paste token in step 1"}</span>
              </p>
            </div>

            {connectStatus === "idle" && (
              <button
                type="button"
                data-testid="connect-to-pengine"
                className="primary-button mt-6 w-full max-w-md rounded-xl text-xs"
                onClick={handleConnect}
              >
                Connect to Pengine
              </button>
            )}
            {connectStatus === "connecting" && (
              <p className="mt-4 font-mono text-xs text-yellow-300">
                Verifying token with Telegram…
              </p>
            )}
            {connectStatus === "error" && (
              <div className="mt-4 space-y-2">
                <p className="font-mono text-xs text-rose-300">{connectError}</p>
                <button
                  type="button"
                  className="secondary-button w-full max-w-md rounded-xl text-xs"
                  onClick={handleConnect}
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
                    onChange={(event) => setBotUsername(event.target.value)}
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
                onClick={handleCopyUri}
              >
                {copiedUri ? "Copied!" : "Copy connection command (curl)"}
              </button>
            </div>
          </div>
          <div className="space-y-4">
            <div className="rounded-2xl border border-white/10 bg-white/5 p-5">
              <p className="font-mono text-xs uppercase tracking-[0.14em] text-(--mid)">
                Direct link
              </p>
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
                <p>{pengineReachable ? "✓" : "○"} Pengine running</p>
                <p>{connectStatus === "connected" ? "✓" : "○"} Bot connected</p>
              </div>
              {connectStatus === "connected" && (
                <button
                  type="button"
                  className="primary-button mt-5 w-full rounded-xl text-xs"
                  onClick={() => onCompleteSetup?.()}
                >
                  Open dashboard
                </button>
              )}
            </div>
          </div>
        </div>
      )}
    </WizardLayout>
  );
}
