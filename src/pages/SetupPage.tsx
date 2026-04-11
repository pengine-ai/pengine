import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { SETUP_STEPS, SetupWizard } from "../modules/bot/components/SetupWizard";
import { TerminalPreview } from "../modules/bot/components/TerminalPreview";
import { TopMenu } from "../shared/ui/TopMenu";

const requirements = [
  "Telegram account",
  "Bot token from BotFather",
  "Ollama installed on this machine",
  "Podman or Docker installed",
  "Pengine desktop app installed",
];

function getFlowCardClasses(index: number, activeStep: number) {
  if (index === activeStep) return "border-cyan-300/30 bg-cyan-300/10";
  if (index < activeStep) return "border-emerald-300/20 bg-emerald-300/10";
  return "border-white/10 bg-white/5";
}

function getFlowStepBadgeClasses(index: number, activeStep: number) {
  if (index === activeStep) return "bg-cyan-300 text-slate-950";
  if (index < activeStep) return "bg-emerald-300 text-slate-950";
  return "bg-white/10 text-slate-200";
}

export function SetupPage() {
  const [activeStep, setActiveStep] = useState(0);
  const currentStep = SETUP_STEPS[activeStep];
  const navigate = useNavigate();

  const handleCompleteSetup = () => {
    navigate("/dashboard", { replace: true });
  };

  return (
    <div className="relative overflow-x-clip pb-20">
      <TopMenu />

      <main className="page-main">
        <SetupWizard onStepChange={setActiveStep} onCompleteSetup={handleCompleteSetup} />

        <section className="mt-8 grid gap-5 sm:mt-10 lg:grid-cols-[0.9fr_1.1fr] lg:gap-6">
          <div className="grid gap-6">
            <div className="panel rounded-4xl p-6">
              <p className="mono-label">Current focus</p>
              <p className="mt-3 text-2xl font-extrabold text-white">{currentStep.title}</p>
              <p className="mt-3 subtle-copy">{currentStep.summary}</p>
              <div className="mt-5 flex items-center gap-3">
                <span className="rounded-full border border-white/10 bg-white/5 px-3 py-1 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
                  {currentStep.duration}
                </span>
                <span className="font-mono text-[11px] uppercase tracking-[0.14em] text-(--dim)">
                  guided flow
                </span>
              </div>
            </div>

            <div className="panel p-5">
              <p className="mono-label">What you need</p>
              <div className="mt-4 grid gap-3 sm:grid-cols-2">
                {requirements.map((item) => (
                  <div key={item} className="feature-chip bg-slate-950/60">
                    {item}
                  </div>
                ))}
              </div>
            </div>
          </div>

          <div className="grid gap-6">
            <div className="panel p-5">
              <p className="mono-label">Setup flow</p>
              <div className="mt-4 grid gap-3 sm:grid-cols-2">
                {SETUP_STEPS.map((step, index) => (
                  <div
                    key={step.title}
                    className={`rounded-2xl border p-4 transition ${getFlowCardClasses(index, activeStep)}`}
                  >
                    <div className="flex items-center gap-3">
                      <span
                        className={`inline-grid h-8 w-8 place-items-center rounded-full font-mono text-xs ${getFlowStepBadgeClasses(index, activeStep)}`}
                      >
                        {index < activeStep ? "✓" : index + 1}
                      </span>
                      <div>
                        <p className="text-sm font-semibold text-white">{step.title}</p>
                        <p className="font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
                          {step.duration}
                        </p>
                      </div>
                    </div>
                    <p className="mt-3 text-sm text-slate-300">{step.summary}</p>
                  </div>
                ))}
              </div>
            </div>

            <div className="grid gap-6 lg:grid-cols-[1fr_0.95fr]">
              <div className="panel p-5">
                <p className="mono-label">Runtime note</p>
                <p className="mt-3 subtle-copy">
                  The Pengine desktop app must be running for the bot to receive messages. You can
                  close this browser tab after setup.
                </p>
              </div>
              <TerminalPreview />
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}
