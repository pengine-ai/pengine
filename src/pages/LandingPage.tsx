import { Link } from "react-router-dom";
import { TerminalPreview } from "../modules/bot/components/TerminalPreview";
import { PhoneMockup } from "../shared/ui/PhoneMockup";
import { SpecMockup } from "../shared/ui/SpecMockup";
import { TopMenu } from "../shared/ui/TopMenu";

const steps = [
  {
    title: "Talk to BotFather",
    description:
      "Create a Telegram bot, paste the token, and let Pengine turn that into your remote control surface.",
    time: "~30 sec",
  },
  {
    title: "Scan the QR",
    description:
      "Your phone becomes the UI. Open Telegram, hit Start, and you are chatting with your own runtime.",
    time: "5 sec",
  },
  {
    title: "Run local inference",
    description:
      "Pengine talks to Ollama on your machine first, so the reasoning happens on your hardware by default.",
    time: "instant",
  },
  {
    title: "Install tools",
    description:
      "Every Docker container can become a new agent capability: search, code sandboxing, scraping, or workflow glue.",
    time: "ongoing",
  },
];

const phases = [
  {
    eyebrow: "Phase 01 · POC",
    title: "It lives",
    quip: "Prove the runtime, not the pitch deck.",
    items: [
      "Telegram bot connection",
      "Local Ollama chat loop",
      "Browser runtime proof-of-concept",
      "Tauri shell groundwork",
    ],
  },
  {
    eyebrow: "Phase 02 · Beta",
    title: "Tools get real",
    quip: "New superpower every /install.",
    items: [
      "ReAct-style loop",
      "Container tool registry",
      "Docker lifecycle UX",
      "First-party starter tools",
    ],
  },
  {
    eyebrow: "Phase 03 · v1",
    title: "Ship the penguin",
    quip: "When it stops feeling like a weekend experiment.",
    items: [
      "Always-on Tauri app",
      "Tool marketplace",
      "Permissions and isolation",
      "Optional remote providers",
    ],
  },
];

const feasibility = [
  ["Telegram long-poll loop", "Rust + teloxide", "Straightforward"],
  ["Tauri desktop shell", "Tauri v2", "Well-supported"],
  ["Local model chat", "Ollama HTTP API", "Easy to iterate"],
  ["Tools as containers", "Docker + manifests", "Very achievable"],
  ["Agent loop", "Rust state machine", "Beta scope"],
  ["Optional cloud routing", "User-supplied API tokens", "Future opt-in"],
];

const scopeItems = [
  "Think with local models",
  "Act through installable tools",
  "Loop with plan -> execute -> reflect",
  "Phone as the default UI",
];

const specCards = [
  {
    title: "Phone = UI",
    body: "Telegram becomes the control panel you already carry.",
  },
  {
    title: "Local by default",
    body: "Ollama handles inference on your hardware first.",
  },
  {
    title: "Tool = container",
    body: "Every Docker image can become a new ability.",
  },
  {
    title: "Optional remote power",
    body: "Future API tokens unlock stronger hosted models only when you permit it.",
  },
];

export function LandingPage() {
  return (
    <div className="relative overflow-x-clip pb-20">
      <TopMenu />

      <main className="page-main">
        <section className="grid items-start gap-6 lg:grid-cols-[1.05fr_0.95fr] lg:gap-8 lg:items-center">
          <div>
            <div className="inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/5 px-3 py-1.5 font-mono text-[11px] uppercase tracking-[0.18em] text-(--mid)">
              <span className="status-pulse h-2 w-2 rounded-full bg-(--teal)" />
              Open source · Local first · Optional cloud
            </div>
            <h1 className="mt-4 max-w-4xl text-4xl font-extrabold leading-[1.05] tracking-tight text-white sm:text-5xl lg:text-6xl">
              Your AI that lives on{" "}
              <span className="font-serif text-(--yellow) italic">your machine,</span> not someone
              else&apos;s server bill.
            </h1>
            <p className="mt-4 max-w-2xl subtle-copy">
              Pengine is a local-first AI agent runtime built around an agentic loop and a messaging
              interface. Telegram becomes the frontend, Ollama becomes the default brain, and Docker
              tools become new abilities on demand.
            </p>
            <div className="mt-5 flex flex-wrap gap-3">
              <Link to="/setup" className="primary-button px-6">
                Scan and connect
              </Link>
              <a href="#spec" className="secondary-button px-6">
                Read the spec
              </a>
            </div>
            <p className="mt-3 font-mono text-xs uppercase tracking-[0.16em] text-(--dim)">
              Rust + WASM now. Tauri shell coming next. No silent API calls.
            </p>
          </div>
          <div className="space-y-4">
            <TerminalPreview />
            <div className="panel p-5">
              <p className="mono-label">Project scope</p>
              <div className="mt-4 grid gap-3 sm:grid-cols-2">
                {scopeItems.map((item) => (
                  <div key={item} className="feature-chip bg-slate-950/60">
                    {item}
                  </div>
                ))}
              </div>
            </div>
          </div>
        </section>

        <section id="how" className="pt-24">
          <p className="mono-label">How it works</p>
          <div className="mt-3 flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
            <h2 className="max-w-3xl text-4xl font-extrabold tracking-tight text-white sm:text-5xl">
              Setup in under a minute, then let the penguin do the weirdly powerful part.
            </h2>
            <p className="max-w-xl subtle-copy">
              The core user experience is simple even if the internals get ambitious: create bot,
              scan QR, talk to your runtime, and install more abilities over time.
            </p>
          </div>
          <div className="mt-10 grid gap-8 lg:grid-cols-[1.05fr_0.95fr]">
            <div className="space-y-4">
              {steps.map((step, index) => (
                <div
                  key={step.title}
                  className="panel flex gap-4 p-5 transition hover:-translate-y-1"
                >
                  <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded-2xl border border-yellow-300/30 bg-yellow-300/10 font-mono text-lg font-bold text-(--yellow)">
                    {String(index + 1).padStart(2, "0")}
                  </div>
                  <div>
                    <div className="flex flex-wrap items-center gap-3">
                      <h3 className="text-lg font-bold text-white">{step.title}</h3>
                      <span className="rounded-full border border-white/10 bg-white/5 px-2 py-1 font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
                        {step.time}
                      </span>
                    </div>
                    <p className="mt-2 subtle-copy">{step.description}</p>
                  </div>
                </div>
              ))}
            </div>
            <PhoneMockup />
          </div>
        </section>

        <section id="spec" className="pt-24">
          <p className="mono-label">Spec overview</p>
          <div className="mt-3 grid gap-8 lg:grid-cols-[1fr_1fr]">
            <div>
              <h2 className="text-4xl font-extrabold tracking-tight text-white sm:text-5xl">
                A runtime, not just a chatbot wrapper.
              </h2>
              <p className="mt-4 subtle-copy">
                Pengine flips the standard AI app model. It runs local-first, keeps cost under user
                control, and treats tools as composable containerized capabilities.
              </p>
              <div className="mt-6 grid gap-3">
                {specCards.map((card) => (
                  <div key={card.title} className="feature-chip p-5">
                    <h3 className="text-lg font-bold text-white">{card.title}</h3>
                    <p className="mt-2 subtle-copy">{card.body}</p>
                  </div>
                ))}
              </div>
            </div>
            <SpecMockup />
          </div>
        </section>

        <section id="roadmap" className="pt-24">
          <p className="mono-label">Roadmap</p>
          <div className="mt-3 flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
            <h2 className="text-4xl font-extrabold tracking-tight text-white sm:text-5xl">
              Ship in phases, not in a 12-month fog.
            </h2>
            <p className="max-w-xl subtle-copy">
              Each phase should be useful on its own. The goal is to build a real local agent
              product, not a concept site that never reaches runtime.
            </p>
          </div>
          <div className="mt-10 grid gap-5 lg:grid-cols-3">
            {phases.map((phase, index) => (
              <div key={phase.title} className="panel overflow-hidden p-0">
                <div
                  className={`h-1 ${
                    index === 0
                      ? "bg-linear-to-r from-sky-400 to-emerald-400"
                      : index === 1
                        ? "bg-linear-to-r from-emerald-400 to-yellow-300"
                        : "bg-linear-to-r from-yellow-300 to-rose-400"
                  }`}
                />
                <div className="p-6">
                  <p className="font-mono text-[11px] uppercase tracking-[0.18em] text-(--dim)">
                    {phase.eyebrow}
                  </p>
                  <h3 className="mt-3 text-2xl font-extrabold text-white">{phase.title}</h3>
                  <p className="mt-2 font-serif text-sm italic text-(--teal)">{phase.quip}</p>
                  <div className="mt-5 space-y-3">
                    {phase.items.map((item) => (
                      <div key={item} className="flex gap-3 font-mono text-sm text-(--mid)">
                        <span className="text-(--yellow)">→</span>
                        <span>{item}</span>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            ))}
          </div>
        </section>

        <section className="pt-24">
          <p className="mono-label">Feasibility</p>
          <div className="mt-8 panel overflow-hidden">
            <div className="overflow-x-auto">
              <table className="min-w-full border-collapse font-mono text-sm">
                <thead>
                  <tr className="border-b border-white/10 text-left text-[11px] uppercase tracking-[0.18em] text-(--dim)">
                    <th className="px-5 py-4">What</th>
                    <th className="px-5 py-4">How</th>
                    <th className="px-5 py-4">Reality check</th>
                  </tr>
                </thead>
                <tbody>
                  {feasibility.map((row) => (
                    <tr key={row[0]} className="border-b border-white/5 last:border-b-0">
                      <td className="px-5 py-4 text-slate-100">{row[0]}</td>
                      <td className="px-5 py-4 text-(--mid)">{row[1]}</td>
                      <td className="px-5 py-4 text-(--mid)">{row[2]}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          </div>
        </section>

        <section className="pt-24">
          <div className="panel bg-linear-to-br from-yellow-300/10 via-transparent to-cyan-300/10 px-6 py-10 text-center">
            <p className="mono-label">Get started</p>
            <h2 className="mt-3 text-4xl font-extrabold tracking-tight text-white sm:text-5xl">
              Your AI. Your machine. Your rules.
            </h2>
            <p className="mx-auto mt-4 max-w-2xl subtle-copy">
              Use the landing page as the spec and vision. Use the wizard page as the actual
              onboarding flow. One explains the product, the other gets it running.
            </p>
            <div className="mt-8 flex flex-wrap justify-center gap-4">
              <Link to="/setup" className="primary-button px-6">
                Open setup wizard
              </Link>
              <a href="#spec" className="secondary-button px-6">
                Review project spec
              </a>
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}
