const items = [
  ["Interface", "Telegram chat on your phone"],
  ["Runtime", "Rust + WASM"],
  ["Models", "Ollama local inference by default"],
  ["Tools", "Docker containers become agent abilities"],
  ["Loop", "Plan -> execute -> reflect"],
  ["Cloud", "Optional external APIs when explicitly enabled"],
];

export function SpecMockup() {
  return (
    <div className="panel overflow-hidden">
      <div className="border-b border-white/10 px-5 py-4">
        <p className="mono-label">Product Spec Mockup</p>
        <h3 className="mt-2 text-xl font-extrabold text-white">
          One runtime. Two modes. Zero silent bills.
        </h3>
      </div>
      <div className="grid gap-4 px-5 py-5 lg:grid-cols-[1.1fr_0.9fr]">
        <div className="rounded-2xl border border-white/10 bg-white/5 p-4">
          <p className="font-mono text-xs uppercase tracking-[0.18em] text-(--dim)">Flow</p>
          <div className="mt-4 flex flex-col gap-3 text-sm text-slate-100">
            <div className="rounded-xl border border-white/10 bg-slate-950/60 px-4 py-3">
              📱 Telegram
            </div>
            <div className="pl-3 font-mono text-xs text-(--teal)">HTTPS / Bot API</div>
            <div className="rounded-xl border border-yellow-300/20 bg-yellow-300/5 px-4 py-3">
              🐧 Pengine runtime
            </div>
            <div className="pl-3 font-mono text-xs text-(--blue)">localhost / events / sockets</div>
            <div className="rounded-xl border border-white/10 bg-slate-950/60 px-4 py-3">
              🦙 Ollama + 🐳 Tools
            </div>
          </div>
        </div>
        <div className="space-y-3">
          {items.map(([label, value]) => (
            <div
              key={label}
              className="rounded-2xl border border-white/10 bg-slate-950/60 px-4 py-3"
            >
              <p className="font-mono text-[11px] uppercase tracking-[0.18em] text-(--dim)">
                {label}
              </p>
              <p className="mt-1 text-sm text-slate-100">{value}</p>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
