import process from "node:process";
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { createPengineViteLogger } from "./vite/pengine-logger";

const host = process.env.TAURI_DEV_HOST;

/** Split heavy `node_modules` so no single chunk exceeds the default 500 kB warning. */
function manualChunks(id: string): string | undefined {
  if (!id.includes("node_modules")) return;
  if (/\/(?:react\/|react-dom\/|scheduler\/)/.test(id)) return "react-vendor";
  if (id.includes("react-router")) return "router";
  if (id.includes("@radix-ui") || id.includes("@dnd-kit")) return "ui-vendor";
  if (id.includes("@tauri-apps")) return "tauri";
  if (id.includes("qrcode.react")) return "qrcode";
  return undefined;
}

export default defineConfig(async () => {
  const clearScreen = false;
  return {
    customLogger: createPengineViteLogger("info", { allowClearScreen: clearScreen }),
    plugins: [tailwindcss(), react()],
    clearScreen,
    build: {
      rollupOptions: {
        output: {
          manualChunks,
        },
      },
    },
    server: {
      port: 1420,
      strictPort: true,
      host: host || false,
      hmr: host
        ? {
            protocol: "ws",
            host,
            port: 1421,
          }
        : undefined,
      watch: {
        ignored: ["**/src-tauri/**"],
      },
    },
    // Production build preview — not Vite’s default 4173 (avoids clashes e.g. with other Vite apps / tooling); adjacent to dev :1420.
    preview: {
      port: 1422,
      strictPort: true,
      // Required when nginx (or any reverse proxy) sends Host: pengine.net — Vite 7 blocks unknown hosts by default.
      allowedHosts: [
        "pengine.net",
        "localhost",
        "127.0.0.1",
        ...(process.env.VITE_PREVIEW_ALLOWED_HOSTS?.split(",")
          .map((h) => h.trim())
          .filter(Boolean) ?? []),
      ],
    },
  };
});
