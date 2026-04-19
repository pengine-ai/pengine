import * as Menubar from "@radix-ui/react-menubar";
import { useState } from "react";
import { Link, useLocation } from "react-router-dom";
import { useAppSessionStore } from "../../modules/bot/store/appSessionStore";

const navLinks = [
  { label: "Home", to: "/" },
  { label: "Dashboard", to: "/dashboard" },
  { label: "Settings", to: "/settings" },
];

export function TopMenu() {
  const location = useLocation();
  const isDeviceConnected = useAppSessionStore((s) => s.isDeviceConnected);
  const showOpenSetup = !isDeviceConnected && location.pathname !== "/setup";
  const [mobileOpen, setMobileOpen] = useState(false);
  const isOnSetup = location.pathname === "/setup";

  const closeMobileMenu = () => setMobileOpen(false);
  const isActiveLink = (to: string) =>
    to === "/" ? location.pathname === "/" : location.pathname.startsWith(to);

  return (
    <header className="section-shell sticky top-0 z-40 pt-2 sm:pt-3">
      <div className="rounded-2xl border border-white/10 bg-slate-950/70 px-3 py-2 backdrop-blur sm:px-4 sm:py-2.5">
        <div className="flex min-h-13 items-center justify-between gap-3">
          <Link to="/" className="flex min-w-0 items-center gap-3" onClick={closeMobileMenu}>
            <img
              src="/pengine-logo-64.png"
              alt="Pengine logo"
              width={32}
              height={32}
              className="h-8 w-8 rounded-lg object-cover"
              decoding="async"
            />
            <div className="min-w-0">
              <p className="font-mono text-[11px] uppercase tracking-[0.18em] text-(--mid)">
                Pengine
              </p>
              <p className="truncate text-sm font-semibold text-white">Local AI Agent Engine</p>
            </div>
          </Link>

          <div className="flex items-center gap-2">
            <button
              type="button"
              aria-expanded={mobileOpen}
              aria-controls="mobile-main-menu"
              aria-label="Toggle main menu"
              onClick={() => setMobileOpen((prev) => !prev)}
              className="inline-flex items-center rounded-lg border border-white/15 px-3 py-2 font-mono text-[11px] uppercase tracking-[0.12em] text-(--mid) transition hover:border-white/30 hover:text-white md:hidden"
            >
              Menu
            </button>
            <Link
              to="/setup"
              className={`primary-button hidden rounded-xl px-4 py-2 text-xs md:inline-flex ${showOpenSetup ? "" : "pointer-events-none invisible"}`}
              tabIndex={showOpenSetup ? undefined : -1}
              aria-hidden={!showOpenSetup}
            >
              Open setup
            </Link>
          </div>
        </div>

        <div
          id="mobile-main-menu"
          className={`overflow-hidden transition-[max-height,opacity,margin] duration-200 md:hidden ${
            mobileOpen ? "mt-3 max-h-64 opacity-100" : "max-h-0 opacity-0"
          }`}
        >
          <nav className="grid gap-2 border-t border-white/10 pt-3" aria-label="Mobile main menu">
            {navLinks.map((item) => (
              <Link
                key={item.label}
                to={item.to}
                onClick={closeMobileMenu}
                className="rounded-lg border border-white/10 bg-white/5 px-3 py-2.5 font-mono text-xs uppercase tracking-[0.14em] text-(--mid) transition hover:border-white/25 hover:text-slate-100"
              >
                {item.label}
              </Link>
            ))}
            {showOpenSetup && (
              <Link
                to="/setup"
                onClick={closeMobileMenu}
                className="primary-button mt-1 inline-flex w-full justify-center rounded-xl px-4 py-2 text-xs"
              >
                Open setup
              </Link>
            )}
          </nav>
        </div>

        <div className="mt-2 hidden items-center justify-between gap-3 border-t border-white/10 pt-2 md:flex">
          <Menubar.Root
            className="inline-flex items-center gap-1 rounded-xl border border-white/10 bg-white/3 p-1"
            aria-label="Main menu"
          >
            {navLinks.map((item) => {
              const active = isActiveLink(item.to);
              return (
                <Menubar.Menu key={item.label}>
                  <Menubar.Trigger asChild>
                    <Link
                      to={item.to}
                      className={`rounded-lg px-3 py-1.5 font-mono text-xs uppercase tracking-[0.14em] outline-none transition ${
                        active
                          ? "border border-cyan-300/25 bg-cyan-300/10 text-cyan-100"
                          : "border border-transparent text-(--mid) hover:border-white/10 hover:bg-white/5 hover:text-slate-100 data-highlighted:bg-white/5 data-highlighted:text-slate-100"
                      }`}
                    >
                      {item.label}
                    </Link>
                  </Menubar.Trigger>
                </Menubar.Menu>
              );
            })}
          </Menubar.Root>

          <p
            className={`font-mono text-[11px] uppercase tracking-[0.14em] ${
              isOnSetup ? "text-cyan-200/90" : "text-(--dim)"
            }`}
          >
            {isOnSetup ? "Setup in progress" : "Runtime dashboard"}
          </p>
        </div>
      </div>
    </header>
  );
}
