export function PhoneMockup() {
  return (
    <div className="mx-auto w-full max-w-[290px] rounded-[2.4rem] border border-white/15 bg-slate-950/80 p-3 shadow-[0_30px_80px_rgba(0,0,0,0.45)]">
      <div className="mx-auto mb-3 h-6 w-24 rounded-b-2xl border-x border-b border-white/10 bg-slate-950" />
      <div className="overflow-hidden rounded-4xl border border-white/10 bg-[#111927]">
        <div className="flex items-center gap-3 border-b border-white/10 bg-[#172030] px-4 py-3">
          <img
            src="/pengine-logo-64.png"
            alt="Pengine"
            width={32}
            height={32}
            className="h-8 w-8 rounded-full object-cover"
            decoding="async"
          />
          <div>
            <p className="text-sm font-semibold text-white">MyPengineBot</p>
            <p className="font-mono text-[11px] text-emerald-300">
              ● online · your laptop is thinking
            </p>
          </div>
        </div>
        <div className="space-y-3 px-3 py-4 font-mono text-[11px]">
          <div className="ml-auto max-w-[80%] rounded-2xl rounded-br-md bg-cyan-400/10 px-3 py-2 text-slate-100">
            /install file-browser
          </div>
          <div className="max-w-[92%] rounded-2xl bg-yellow-400/10 px-3 py-2 text-yellow-200">
            pulling pengine/file-browser... done
          </div>
          <div className="ml-auto max-w-[85%] rounded-2xl rounded-br-md bg-cyan-400/10 px-3 py-2 text-slate-100">
            list what&apos;s in my Downloads folder, newest first
          </div>
          <div className="max-w-[92%] rounded-2xl bg-sky-400/10 px-3 py-2 text-sky-200">
            [files] scanning ~/Downloads…
          </div>
          <div className="max-w-[88%] rounded-2xl rounded-bl-md bg-[#1e2e44] px-3 py-2 text-slate-100">
            Found 8 items. Newest: project-export.zip, meeting-notes.md, screenshot-chat.png… Want
            me to open one or peek in Documents instead?
          </div>
        </div>
        <div className="m-3 flex items-center gap-2 rounded-full bg-[#1e2e44] px-4 py-3 font-mono text-[11px] text-(--dim)">
          <span>Message Pengine...</span>
          <span className="ml-auto text-slate-300">➤</span>
        </div>
      </div>
    </div>
  );
}
