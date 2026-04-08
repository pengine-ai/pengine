import { useCallback, useEffect, useRef, useState } from "react";

type LogLine = { timestamp: string; kind: string; message: string };

const PENGINE_API = "http://127.0.0.1:21516";

const fallbackLines: LogLine[] = [
  { timestamp: "00:00:00", kind: "ok", message: "Waiting for Pengine service…" },
];

function kindClass(kind: string) {
  if (kind === "ok") return "bg-emerald-400/10 text-emerald-300";
  if (kind === "run") return "bg-sky-400/10 text-sky-300";
  if (kind === "tool") return "bg-yellow-400/10 text-yellow-200";
  if (kind === "reply") return "bg-violet-400/10 text-violet-300";
  if (kind === "msg") return "bg-cyan-400/10 text-cyan-300";
  return "bg-slate-400/10 text-slate-300";
}

export function TerminalPreview() {
  const [lines, setLines] = useState<LogLine[]>(fallbackLines);
  const scrollRef = useRef<HTMLDivElement>(null);

  const addLine = useCallback((line: LogLine) => {
    setLines((prev) => [...prev.slice(-49), line]);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let cleanup: (() => void) | null = null;

    async function connect() {
      // Try Tauri event listener first (works inside the desktop app)
      try {
        const { listen } = await import("@tauri-apps/api/event");
        const unlisten = await listen<LogLine>("pengine-log", (event) => {
          if (!cancelled) addLine(event.payload);
        });
        cleanup = unlisten;
        return;
      } catch {
        // Not running inside Tauri — fall through to SSE
      }

      // Browser fallback: SSE stream from the loopback API
      try {
        const es = new EventSource(`${PENGINE_API}/v1/logs`);
        es.onmessage = (event) => {
          if (cancelled) return;
          try {
            const entry: LogLine = JSON.parse(event.data);
            addLine(entry);
          } catch {
            // ignore malformed events
          }
        };
        cleanup = () => es.close();
      } catch {
        // SSE not available
      }
    }

    connect();

    return () => {
      cancelled = true;
      cleanup?.();
    };
  }, [addLine]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight, behavior: "smooth" });
  }, [lines]);

  return (
    <section className="panel overflow-hidden" aria-label="Runtime preview">
      <div className="flex items-center gap-2 border-b border-white/10 bg-white/5 px-4 py-3 font-mono text-xs text-(--dim)">
        <span className="h-3 w-3 rounded-full bg-[#ff5f57]" />
        <span className="h-3 w-3 rounded-full bg-[#febc2e]" />
        <span className="h-3 w-3 rounded-full bg-[#28c840]" />
        <p className="ml-2">pengine runtime</p>
      </div>
      <div ref={scrollRef} className="max-h-64 space-y-3 overflow-y-auto px-4 py-5 font-mono text-sm">
        {lines.map((line, i) => (
          <div key={`${line.timestamp}-${i}`} className="flex flex-wrap items-center gap-2">
            <span className="text-(--dim)">[{line.timestamp}]</span>
            <span
              className={`rounded-full px-2 py-0.5 text-[11px] uppercase tracking-[0.18em] ${kindClass(line.kind)}`}
            >
              {line.kind}
            </span>
            <span className="text-slate-100">{line.message}</span>
          </div>
        ))}
      </div>
    </section>
  );
}
