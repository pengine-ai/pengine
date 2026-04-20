/**
 * Detect the Tauri desktop/webview shell. Static marketing builds (e.g. Docker) run in a normal
 * browser with no Tauri globals.
 */
export function isTauriApp(): boolean {
  if (typeof window === "undefined") return false;
  const w = window as Window & { __TAURI_INTERNALS__?: object; isTauri?: boolean };
  return Boolean(w.__TAURI_INTERNALS__ ?? w.isTauri);
}

/**
 * “Marketing” shell: any normal browser (dev or prod) without Tauri — only landing + about.
 * Set `VITE_ENABLE_APP_ROUTES_IN_BROWSER=true` to expose setup/dashboard/settings in a browser
 * (used by Playwright; optional for local UI testing without the desktop shell).
 */
export function isMarketingWebsite(): boolean {
  if (isTauriApp()) return false;
  return import.meta.env.VITE_ENABLE_APP_ROUTES_IN_BROWSER !== "true";
}
