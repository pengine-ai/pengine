import { useCallback, useEffect, useMemo, useState } from "react";
import { fetchOllamaModel } from "../../ollama/api";
import { fetchRuntimeStatus, type RuntimeStatus } from "../../toolengine";
import { getPengineHealth, PENGINE, postConnect } from "../api";
import { useAppSessionStore } from "../store/appSessionStore";
import { WizardLayout } from "../../../shared/ui/WizardLayout";
import {
  WizardStepConnect,
  WizardStepContainerRuntime,
  WizardStepCreateBot,
  WizardStepOllama,
  WizardStepPengineLocal,
} from "./SetupWizardSteps";

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
    title: "Install a container runtime",
    summary: "Install Podman (preferred) or Docker so Pengine can run tools in isolated sandboxes.",
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
  const [runtimeChecking, setRuntimeChecking] = useState(false);
  const [runtimeStatus, setRuntimeStatus] = useState<RuntimeStatus | null>(null);
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

  const canContinueStep = useMemo(() => {
    const gates: Record<number, boolean> = {
      0: status === "valid",
      1: !!ollamaModel,
      2: runtimeStatus?.available === true,
      3: pengineReachable === true,
      4: connectStatus === "connected",
    };
    return gates[step] ?? false;
  }, [step, status, ollamaModel, runtimeStatus, pengineReachable, connectStatus]);

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

  const checkRuntime = useCallback(async () => {
    setRuntimeChecking(true);
    setRuntimeStatus(null);
    try {
      const rt = await fetchRuntimeStatus(5000);
      setRuntimeStatus(rt ?? { available: false });
    } finally {
      setRuntimeChecking(false);
    }
  }, []);

  useEffect(() => {
    if (step === 2) {
      checkRuntime();
    }
  }, [step, checkRuntime]);

  const checkPengineHealth = useCallback(async () => {
    setPengineChecking(true);
    try {
      setPengineReachable((await getPengineHealth(3000)) !== null);
    } finally {
      setPengineChecking(false);
    }
  }, []);

  useEffect(() => {
    if (step === 3) {
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
        <WizardStepCreateBot
          botToken={botToken}
          onBotTokenChange={setBotToken}
          status={status}
          tokenStatusMessage={tokenStatusMessage}
        />
      )}
      {step === 1 && (
        <WizardStepOllama
          ollamaChecking={ollamaChecking}
          ollamaReachable={ollamaReachable}
          ollamaModel={ollamaModel}
          onRetry={checkOllama}
        />
      )}
      {step === 2 && (
        <WizardStepContainerRuntime
          runtimeChecking={runtimeChecking}
          runtimeStatus={runtimeStatus}
          onRetry={checkRuntime}
        />
      )}
      {step === 3 && (
        <WizardStepPengineLocal
          pengineChecking={pengineChecking}
          pengineReachable={pengineReachable}
          onRetry={checkPengineHealth}
        />
      )}
      {step === 4 && (
        <WizardStepConnect
          botId={botId}
          status={status}
          ollamaModel={ollamaModel}
          runtimeStatus={runtimeStatus}
          pengineReachable={pengineReachable}
          connectStatus={connectStatus}
          connectError={connectError}
          verifiedBot={verifiedBot}
          botUsername={botUsername}
          onBotUsernameChange={setBotUsername}
          onConnect={handleConnect}
          onCopyUri={handleCopyUri}
          copiedUri={copiedUri}
          onCompleteSetup={onCompleteSetup}
        />
      )}
    </WizardLayout>
  );
}
