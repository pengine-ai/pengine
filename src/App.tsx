import { useEffect, useRef, useState } from "react";
import { Navigate, Route, Routes, useNavigate } from "react-router-dom";
import { PENGINE_API_BASE } from "./config";
import { DashboardPage } from "./pages/DashboardPage";
import { LandingPage } from "./pages/LandingPage";
import { SetupPage } from "./pages/SetupPage";
import { useAppSessionStore } from "./stores/appSessionStore";

/**
 * Once after load: if there is an active connection (persisted or reported by
 * the local app), go to the dashboard. Does not run again on navigation.
 */
function StartupDashboardRedirect() {
  const navigate = useNavigate();
  const connectDevice = useAppSessionStore((state) => state.connectDevice);
  /** Ensures redirect logic runs only once on first paint — never on later navigations to Home. */
  const startupDone = useRef(false);

  useEffect(() => {
    if (startupDone.current) return;
    startupDone.current = true;

    const path = window.location.pathname;
    if (path === "/dashboard") return;

    if (useAppSessionStore.getState().isDeviceConnected) {
      navigate("/dashboard", { replace: true });
      return;
    }

    // Recover session from a running local app only when opening the home page
    // (skip on /setup so the wizard can load; health mocks in e2e use /setup)
    if (path !== "/") return;

    let cancelled = false;
    (async () => {
      try {
        const resp = await fetch(`${PENGINE_API_BASE}/v1/health`, {
          signal: AbortSignal.timeout(2000),
        });
        if (!resp.ok || cancelled) return;
        const data = await resp.json();
        if (data.bot_connected && data.bot_username && !cancelled) {
          connectDevice({
            bot_username: data.bot_username,
            bot_id: data.bot_id ?? null,
          });
          navigate("/dashboard", { replace: true });
        }
      } catch {
        // local app not running
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [navigate, connectDevice]);

  return null;
}

function App() {
  const [sessionReady, setSessionReady] = useState(false);

  useEffect(() => {
    if (useAppSessionStore.persist.hasHydrated()) {
      setSessionReady(true);
      return;
    }
    return useAppSessionStore.persist.onFinishHydration(() => {
      setSessionReady(true);
    });
  }, []);

  if (!sessionReady) {
    return (
      <div
        className="flex min-h-screen items-center justify-center bg-slate-950 text-slate-400"
        data-testid="session-hydrating"
      >
        <p className="font-mono text-xs uppercase tracking-[0.2em]">Loading…</p>
      </div>
    );
  }

  return (
    <div data-testid="app-ready">
      <StartupDashboardRedirect />
      <Routes>
        <Route path="/" element={<LandingPage />} />
        <Route path="/setup" element={<SetupPage />} />
        <Route path="/dashboard" element={<DashboardPage />} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </div>
  );
}

export default App;
