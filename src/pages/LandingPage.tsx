import { Link } from "react-router-dom";
import { TerminalPreview } from "../modules/bot/components/TerminalPreview";
import { DownloadLatestButton } from "../modules/updater";
import { AboutLegalContent } from "../shared/about/AboutLegalContent";
import { isMarketingWebsite } from "../shared/runtimeTarget";
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

const capabilities = [
  {
    title: "Tools in containers",
    body: "Every MCP tool runs in its own Docker container. Process isolation, named volumes, no surprise access to your home directory.",
  },
  {
    title: "You pick the mounts",
    body: "Filesystem access is a checkbox, not a config file. Grant a folder, revoke it from the dashboard, and the agent stops seeing it immediately.",
  },
  {
    title: "Audit log, always on",
    body: "Every tool call, every argument, every exit code lands in a scrollable log. You can see exactly what the agent ran, when, and why.",
  },
  {
    title: "Policy lives in the app",
    body: "Allow and deny rules are set in settings, not in prompts. The model cannot talk its way past a toggle it cannot see.",
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

const heroCapabilities = [
  { tag: "Isolation", label: "Tools in containers" },
  { tag: "Control", label: "You pick the mounts" },
  { tag: "Visibility", label: "Audit log, always on" },
  { tag: "Policy", label: "Rules in the app, not the prompt" },
];

const differentiators = [
  {
    theirs: "The agent runs shell commands straight on your laptop.",
    ours: "Every tool runs in its own container, with only the folders you hand it.",
  },
  {
    theirs: "The brain lives on someone else's server and bills per token.",
    ours: "Local models are the default. Cloud is an opt-in, not a surprise.",
  },
  {
    theirs: "You trust a system prompt to keep the agent in line.",
    ours: "Rules are toggles in settings. The model cannot argue with a checkbox.",
  },
  {
    theirs: "Adding a new ability means learning a plugin framework.",
    ours: "Point Pengine at a container image. That is the whole onboarding.",
  },
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
  const marketingSite = isMarketingWebsite();

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
              {marketingSite ? (
                <DownloadLatestButton className="primary-button px-6" />
              ) : (
                <Link to="/setup" className="primary-button px-6">
                  Scan and connect
                </Link>
              )}
              {!marketingSite && <DownloadLatestButton className="secondary-button px-6" />}
              <a href="#spec" className="secondary-button px-6">
                Read the spec
              </a>
            </div>
            <p className="mt-3 font-mono text-xs uppercase tracking-[0.16em] text-(--dim)">
              Containerized tools. Audit logs. Policy you set in the app.
            </p>
          </div>
          <div className="space-y-4">
            <TerminalPreview />
            <div className="panel p-5">
              <p className="mono-label">What you get</p>
              <div className="mt-4 grid gap-2 sm:grid-cols-2">
                {heroCapabilities.map((item) => (
                  <div
                    key={item.label}
                    className="rounded-xl border border-white/10 bg-slate-950/60 p-3"
                  >
                    <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-(--teal)">
                      {item.tag}
                    </p>
                    <p className="mt-1 text-sm text-slate-100">{item.label}</p>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </section>

        <section id="why" className="pt-24">
          <p className="mono-label">Why Pengine</p>
          <div className="mt-3 flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
            <h2 className="max-w-3xl text-4xl font-extrabold tracking-tight text-white sm:text-5xl">
              Other agents ask for trust.{" "}
              <span className="font-serif text-(--yellow) italic">Pengine gives you a leash.</span>
            </h2>
            <p className="max-w-xl subtle-copy">
              A quick look at the places where most agent tools cut corners&mdash;and what Pengine
              does instead.
            </p>
          </div>
          <div className="mt-10 grid gap-4">
            {differentiators.map((row) => (
              <div
                key={row.ours}
                className="panel grid gap-0 overflow-hidden p-0 md:grid-cols-[1fr_1fr]"
              >
                <div className="border-b border-white/10 p-5 md:border-b-0 md:border-r">
                  <p className="font-mono text-[11px] uppercase tracking-[0.18em] text-(--dim)">
                    Most agents
                  </p>
                  <p className="mt-2 text-sm text-(--mid)">{row.theirs}</p>
                </div>
                <div className="bg-slate-950/40 p-5">
                  <p className="font-mono text-[11px] uppercase tracking-[0.18em] text-(--yellow)">
                    Pengine
                  </p>
                  <p className="mt-2 text-sm text-slate-100">{row.ours}</p>
                </div>
              </div>
            ))}
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

        <section id="custom-tools" className="pt-24">
          <div className="panel bg-linear-to-br from-cyan-300/10 via-transparent to-yellow-300/10 p-8 sm:p-10">
            <div className="grid gap-8 lg:grid-cols-[1.1fr_0.9fr] lg:items-center">
              <div>
                <p className="mono-label">Bring your own tool</p>
                <h2 className="mt-3 text-3xl font-extrabold tracking-tight text-white sm:text-4xl">
                  If it runs in a container,{" "}
                  <span className="font-serif text-(--yellow) italic">it can be a tool.</span>
                </h2>
                <p className="mt-4 subtle-copy">
                  Point Pengine at a Docker image, give it a friendly name, and it shows up in the
                  dashboard next to everything else. Your agent picks up a new capability; you keep
                  the checkbox that turns it off.
                </p>
                <p className="mt-3 subtle-copy">
                  No framework to learn, no plugin API to wrestle. If you can write a Dockerfile (or
                  grab one someone else wrote), you can ship a Pengine tool.
                </p>
                <div className="mt-6 flex flex-wrap gap-3">
                  <a
                    href="https://github.com/pengine-ai/pengine/blob/main/doc/guides/custom-mcp-tools.md"
                    target="_blank"
                    rel="noreferrer"
                    className="primary-button px-6"
                  >
                    Read the tools guide
                  </a>
                  <a
                    href="https://github.com/pengine-ai/pengine/tree/main/tools"
                    target="_blank"
                    rel="noreferrer"
                    className="secondary-button px-6"
                  >
                    Browse example tools
                  </a>
                </div>
              </div>
              <ol className="space-y-3 font-mono text-sm text-(--mid)">
                <li className="flex gap-3">
                  <span className="text-(--yellow)">01</span>
                  <span>Pick a container image you already trust.</span>
                </li>
                <li className="flex gap-3">
                  <span className="text-(--yellow)">02</span>
                  <span>Add it from the dashboard. Pengine takes care of the wiring.</span>
                </li>
                <li className="flex gap-3">
                  <span className="text-(--yellow)">03</span>
                  <span>
                    Choose which folders it can touch. That is the whole permission model.
                  </span>
                </li>
                <li className="flex gap-3">
                  <span className="text-(--yellow)">04</span>
                  <span>Your agent can use it on the next turn. No restart, no rebuild.</span>
                </li>
              </ol>
            </div>
          </div>
        </section>

        <section id="skills" className="pt-24">
          <div className="panel bg-linear-to-br from-emerald-300/10 via-transparent to-fuchsia-300/10 p-8 sm:p-10">
            <div className="grid gap-8 lg:grid-cols-[1.1fr_0.9fr] lg:items-center">
              <div>
                <p className="mono-label">Skills layer</p>
                <h2 className="mt-3 text-3xl font-extrabold tracking-tight text-white sm:text-4xl">
                  Teach the model with markdown,{" "}
                  <span className="font-serif text-(--teal) italic">not microservices.</span>
                </h2>
                <p className="mt-4 subtle-copy">
                  Skills are small <strong className="text-slate-200">SKILL.md</strong> bundles:
                  YAML frontmatter plus a body with request examples, response shape, and when to
                  use them. They ship as extra <strong className="text-slate-200">system</strong>{" "}
                  context—cheap for tokens, easy to fork, and separate from Docker MCP tools.
                </p>
                <p className="mt-3 subtle-copy">
                  Use the dashboard to enable or disable each skill, tune how much context they may
                  consume, add your own markdown, or pull examples from ClawHub. When you need real
                  execution on disk or in a container, that is what the tools guide is for.
                </p>
                <div className="mt-6 flex flex-wrap gap-3">
                  <a
                    href="https://github.com/pengine-ai/pengine/blob/main/doc/guides/skills.md"
                    target="_blank"
                    rel="noreferrer"
                    className="primary-button px-6"
                  >
                    Read the skills guide
                  </a>
                  <a
                    href="https://github.com/pengine-ai/pengine/tree/main/tools/skills"
                    target="_blank"
                    rel="noreferrer"
                    className="secondary-button px-6"
                  >
                    Example bundled skills
                  </a>
                </div>
              </div>
              <ol className="space-y-3 font-mono text-sm text-(--mid)">
                <li className="flex gap-3">
                  <span className="text-(--teal)">01</span>
                  <span>
                    Frontmatter names the skill; the body is the recipe the model reads first.
                  </span>
                </li>
                <li className="flex gap-3">
                  <span className="text-(--teal)">02</span>
                  <span>Optional mandatory.md adds rules without cluttering the main doc.</span>
                </li>
                <li className="flex gap-3">
                  <span className="text-(--teal)">03</span>
                  <span>Bundled samples are read-only; copy to your custom dir to iterate.</span>
                </li>
                <li className="flex gap-3">
                  <span className="text-(--teal)">04</span>
                  <span>
                    Total injected text is capped—balance depth vs the context slider in the app.
                  </span>
                </li>
              </ol>
            </div>
          </div>
        </section>

        <section id="capabilities" className="pt-24">
          <p className="mono-label">Under the hood</p>
          <div className="mt-3 flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
            <h2 className="max-w-3xl text-4xl font-extrabold tracking-tight text-white sm:text-5xl">
              Local brain, boxed hands,{" "}
              <span className="font-serif text-(--yellow) italic">human veto.</span>
            </h2>
            <p className="max-w-xl subtle-copy">
              Four building blocks do the heavy lifting. Together they mean the agent can be useful
              without being loose.
            </p>
          </div>
          <div className="mt-10 grid gap-5 sm:grid-cols-2">
            {capabilities.map((card) => (
              <div key={card.title} className="panel p-6">
                <h3 className="text-xl font-bold text-white">{card.title}</h3>
                <p className="mt-3 subtle-copy">{card.body}</p>
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
            <div className="mt-8 flex flex-wrap justify-center gap-4">
              {marketingSite ? (
                <DownloadLatestButton className="primary-button px-6" />
              ) : (
                <Link to="/setup" className="primary-button px-6">
                  Open setup wizard
                </Link>
              )}
              {!marketingSite && <DownloadLatestButton className="secondary-button px-6" />}
              <a
                href="https://github.com/pengine-ai/pengine"
                className="secondary-button gap-2 px-6"
                target="_blank"
                rel="noopener noreferrer"
                aria-label="Pengine repository on GitHub"
              >
                <svg
                  className="h-4 w-4 shrink-0 text-slate-200"
                  viewBox="0 0 16 16"
                  fill="currentColor"
                  aria-hidden
                >
                  <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
                </svg>
                On GitHub
              </a>
            </div>
          </div>
        </section>

        {marketingSite && (
          <section id="about" className="scroll-mt-28 pt-24">
            <p className="mono-label">About</p>
            <div className="mt-3 flex flex-col gap-3 lg:flex-row lg:items-end lg:justify-between">
              <h2 className="max-w-3xl text-4xl font-extrabold tracking-tight text-white sm:text-5xl">
                Contact, privacy, and project
              </h2>
              <p className="max-w-xl subtle-copy">
                Links and compliance information for the open source app.
              </p>
            </div>
            <div className="mt-10 panel p-6 sm:p-8">
              <AboutLegalContent />
            </div>
          </section>
        )}
      </main>
    </div>
  );
}
