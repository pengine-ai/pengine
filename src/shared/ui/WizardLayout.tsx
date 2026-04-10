import type { ReactNode } from "react";

type WizardLayoutProps = {
  stepTitles: string[];
  activeStep: number;
  children: ReactNode;
  onBack: () => void;
  onNext: () => void;
  onSelectStep: (index: number) => void;
  canGoBack: boolean;
  canGoNext: boolean;
};

function getStepCardClasses(index: number, activeStep: number) {
  if (index === activeStep) return "border-cyan-300/40 bg-cyan-300/10 text-white";
  if (index < activeStep) return "border-emerald-300/20 bg-emerald-300/10 text-slate-100";
  return "border-white/10 bg-white/5 text-(--mid)";
}

function getStepBadgeClasses(index: number, activeStep: number) {
  if (index === activeStep) return "bg-cyan-300 text-slate-950";
  if (index < activeStep) return "bg-emerald-300 text-slate-950";
  return "bg-white/10 text-slate-200";
}

export function WizardLayout({
  stepTitles,
  activeStep,
  children,
  onBack,
  onNext,
  onSelectStep,
  canGoBack,
  canGoNext,
}: WizardLayoutProps) {
  const progress = ((activeStep + 1) / stepTitles.length) * 100;

  return (
    <section className="space-y-4">
      <div className="panel p-4 sm:p-5">
        <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
          <div>
            <p className="mono-label">Setup progress</p>
            <h2 className="mt-2 text-2xl font-extrabold text-white">
              Step {activeStep + 1} of {stepTitles.length}: {stepTitles[activeStep]}
            </h2>
            <p className="mt-2 subtle-copy">
              Follow the flow from left to right, and jump back to any completed step if you want to
              verify something.
            </p>
          </div>
          <div className="min-w-40 sm:text-right">
            <p className="font-mono text-xs uppercase tracking-[0.14em] text-(--mid)">
              {Math.round(progress)}% complete
            </p>
          </div>
        </div>
        <div className="mt-4 h-2 overflow-hidden rounded-full bg-white/10">
          <div
            className="h-full rounded-full bg-linear-to-r from-cyan-300 via-emerald-300 to-yellow-300 transition-[width] duration-300"
            style={{ width: `${progress}%` }}
          />
        </div>
      </div>

      <ol className="grid gap-3 sm:grid-cols-2 xl:grid-cols-5" aria-label="Setup steps">
        {stepTitles.map((title, index) => (
          <li key={title}>
            <button
              type="button"
              disabled={index > activeStep}
              onClick={() => onSelectStep(index)}
              className={`flex w-full items-center gap-3 rounded-2xl border px-4 py-3 text-left transition ${getStepCardClasses(index, activeStep)}`}
            >
              <span
                className={`inline-grid h-8 w-8 place-items-center rounded-full font-mono text-xs ${getStepBadgeClasses(index, activeStep)}`}
              >
                {index < activeStep ? "✓" : index + 1}
              </span>
              <span className="font-mono text-xs uppercase tracking-[0.14em]">{title}</span>
            </button>
          </li>
        ))}
      </ol>

      <div className="panel overflow-hidden">
        <div className="border-b border-white/10 bg-white/5 px-4 py-3 font-mono text-xs uppercase tracking-[0.18em] text-(--dim) sm:px-5">
          Setup wizard
        </div>
        <div className="px-4 py-5 sm:px-6 sm:py-6">{children}</div>
      </div>

      <div className="flex items-center justify-between gap-3">
        <button
          type="button"
          className="secondary-button rounded-xl px-4 py-2.5 text-xs text-slate-200 disabled:cursor-not-allowed disabled:opacity-50"
          onClick={onBack}
          disabled={!canGoBack}
        >
          Back
        </button>
        <button
          type="button"
          className="primary-button rounded-xl px-4 py-2.5 text-xs disabled:cursor-not-allowed disabled:opacity-50"
          onClick={onNext}
          disabled={!canGoNext}
        >
          Continue
        </button>
      </div>
    </section>
  );
}
