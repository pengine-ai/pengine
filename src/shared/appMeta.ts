const rawOrigin = import.meta.env.VITE_APP_ORIGIN;

if (import.meta.env.PROD && (rawOrigin === undefined || rawOrigin === "")) {
  const msg =
    "[pengine] VITE_APP_ORIGIN is missing — set it for production (e.g. .env.production or Docker build-arg).";
  throw new Error(msg);
}

/** Public origin for the deployed web app (production: https://pengine.net). */
export const APP_ORIGIN: string = rawOrigin ?? "https://pengine.net";
