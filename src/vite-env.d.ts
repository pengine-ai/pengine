/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** Production site URL (see `.env.production`). */
  readonly VITE_APP_ORIGIN?: string;
  /** When `"true"`, allow /setup, /dashboard, /settings in a normal browser (e.g. E2E). */
  readonly VITE_ENABLE_APP_ROUTES_IN_BROWSER?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
