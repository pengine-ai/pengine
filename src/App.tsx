import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { Navigate, Route, Routes, useNavigate } from "react-router-dom";
import { getPengineHealth } from "./modules/bot/api";
import { useAppSessionStore } from "./modules/bot/store/appSessionStore";

const LandingPage = lazy(() =>
  import("./pages/LandingPage").then((m) => ({ default: m.LandingPage })),
);
const SetupPage = lazy(() => import("./pages/SetupPage").then((m) => ({ default: m.SetupPage })));
const DashboardPage = lazy(() =>
  import("./pages/DashboardPage").then((m) => ({ default: m.DashboardPage })),
);
const SettingsPage = lazy(() =>
  import("./pages/SettingsPage").then((m) => ({ default: m.SettingsPage })),
);

function RoutePageFallback() {
  return (
    <div
      className="flex min-h-screen items-center justify-center bg-slate-950 text-slate-400"
      data-testid="route-chunk-loading"
    >
      <p className="font-mono text-xs uppercase tracking-[0.2em]">Loading…</p>
    </div>
  );
}

/** One-shot: sync dashboard route with persisted session or running local app. */
function StartupDashboardRedirect() {
  const navigate = useNavigate();
  const connectDevice = useAppSessionStore((state) => state.connectDevice);
  const startupDone = useRef(false);

  useEffect(() => {
    if (startupDone.current) return;
    startupDone.current = true;

    const path = window.location.pathname;
    if (path === "/dashboard" || path === "/settings") return;

    if (useAppSessionStore.getState().isDeviceConnected) {
      navigate("/dashboard", { replace: true });
      return;
    }

    // Recover session from a running local app only when opening the home page
    // (skip on /setup so the wizard can load; health mocks in e2e use /setup)
    if (path !== "/") return;

    let cancelled = false;
    (async () => {
      const health = await getPengineHealth(2000);
      if (!health || cancelled) return;
      if (health.bot_connected && health.bot_username) {
        connectDevice({
          bot_username: health.bot_username,
          bot_id: health.bot_id ?? null,
        });
        navigate("/dashboard", { replace: true });
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
      <Suspense fallback={<RoutePageFallback />}>
        <Routes>
          <Route path="/" element={<LandingPage />} />
          <Route path="/setup" element={<SetupPage />} />
          <Route path="/dashboard" element={<DashboardPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </Suspense>
    </div>
  );
}

export default App;
