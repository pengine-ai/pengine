import * as Menubar from "@radix-ui/react-menubar";
import { Link, useLocation } from "react-router-dom";
import { useAppSessionStore } from "../stores/appSessionStore";

const navLinks = [
  { label: "Home", to: "/" },
  { label: "Dashboard", to: "/dashboard" },
];

export function TopMenu() {
  const location = useLocation();
  const isDeviceConnected = useAppSessionStore((s) => s.isDeviceConnected);
  const showOpenSetup = !isDeviceConnected && location.pathname !== "/setup";

  return (
    <header className="section-shell sticky top-0 z-40 pt-2 sm:pt-3">
      <div className="flex min-h-[3.25rem] items-center justify-between rounded-2xl border border-white/10 bg-slate-950/70 px-3 py-2 sm:px-4 sm:py-2.5 backdrop-blur">
        <Link to="/" className="flex items-center gap-3">
          <img
            src="/pengine-logo-64.png"
            alt="Pengine logo"
            width={32}
            height={32}
            className="h-8 w-8 rounded-lg object-cover"
            decoding="async"
          />
          <div>
            <p className="font-mono text-[11px] uppercase tracking-[0.18em] text-(--mid)">
              Pengine
            </p>
            <p className="text-sm font-semibold text-white">Local AI Agent Engine</p>
          </div>
        </Link>

        <Menubar.Root className="hidden items-center gap-2 md:flex" aria-label="Main menu">
          {navLinks.map((item) => (
            <Menubar.Menu key={item.label}>
              <Menubar.Trigger asChild>
                <Link
                  to={item.to}
                  className="rounded-lg px-3 py-2 font-mono text-xs uppercase tracking-[0.14em] text-(--mid) outline-none transition hover:text-slate-100 data-highlighted:bg-white/5 data-highlighted:text-slate-100"
                >
                  {item.label}
                </Link>
              </Menubar.Trigger>
            </Menubar.Menu>
          ))}
        </Menubar.Root>

        <Link
          to="/setup"
          className={`primary-button rounded-xl px-4 py-2 text-xs ${showOpenSetup ? "" : "pointer-events-none invisible"}`}
          tabIndex={showOpenSetup ? undefined : -1}
          aria-hidden={!showOpenSetup}
        >
          Open setup
        </Link>
      </div>
    </header>
  );
}
