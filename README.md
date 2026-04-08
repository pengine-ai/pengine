# Pengine

<img src="public/pengine-logo-128.png" alt="Pengine logo" width="40" height="40" />

**Your local AI agent (that can phone home if you let it).**

Pengine is a local-first agent runtime: you talk to it from **Telegram** while inference and tools stay on your machine by default.

- No cloud dependency by default  
- No silent API calls  
- No surprise bills  

## Stack

| Layer | Choice |
| --- | --- |
| UI | React 19, Vite 7, Tailwind CSS 4 |
| Desktop (optional) | Tauri 2 |
| State | Zustand (session / device gate) |
| E2E | Playwright |

## Project layout

```
src/                 Web app (landing, setup wizard, dashboard)
src/assets/          Source logo: pengine-logo.png (master for all derivatives)
public/              Favicon + small PNGs for the web UI (generated, committed)
src-tauri/icons/     App bundle icons (generated from the same source, committed)
e2e/                 Playwright specs
```

**Logo source of truth:** `src/assets/pengine-logo.png`. Regenerate everything from it after you change the artwork:

```bash
bun run generate:logos
```

This writes `public/favicon-32.png`, `public/pengine-logo-64.png`, `public/pengine-logo-128.png`, and runs `tauri icon`, which fills `src-tauri/icons/` (desktop bundle assets plus `icons/ios/` and `icons/android/`). Web resizing uses macOS `sips` or ImageMagick `magick` if `sips` is unavailable.

## Routes

| Path | Purpose |
| --- | --- |
| `/` | Landing: vision, scope, roadmap |
| `/setup` | Guided onboarding (see below) |
| `/dashboard` | Shown after “device connected” (session gate) |

## Setup wizard (`/setup`)

Onboarding is a **four-step** flow:

1. **Create bot** — BotFather, paste the Telegram bot token (bot ID is derived from the token for pairing).
2. **Install Ollama** — Local model runtime; demo button stands in until real health checks exist.
3. **Pengine local** — Run the agent on this machine (web or Tauri); demo button for now.
4. **Connect** — Optional `@username` for the QR / deep link; simulate linking the bot to Pengine (mock), then open the dashboard.

End-to-end tests cover this path under `e2e/`.

## How messages flow

`Phone → Telegram → local runtime (browser/Tauri) → Ollama → optional Docker tools → back to you`

## Development

### Prerequisites

- **Node.js** ≥ 20 (see `.nvmrc`)
- **Rust** (stable) if you use Tauri
- **Ollama** and **Docker** are expected for a full local stack (optional for UI-only work)

### Install and run

```bash
bun install
bun run dev
```

### Tauri (optional)

```bash
bun run tauri dev
```

### Build

```bash
bun run build
```

### End-to-end tests

Install browsers once (if you have not):

```bash
npx playwright install
```

Then:

```bash
bun run test:e2e
```

Playwright starts the Vite dev server automatically. In CI (`CI=true`), it always spawns a fresh server; locally it may reuse an existing dev server on port 1420 if one is already running.

Artifacts: `playwright-report/` and `test-results/` are gitignored.

### Pull a model (when using Ollama)

```bash
ollama pull llama3.2
```

## Product direction

- **Phone-as-UI** — Telegram as the control surface  
- **Local by default** — your hardware, models, and data  
- **Tools via containers** — install capabilities as needed  
- **Minimal runtime** — explicit opt-in for remote providers later  

## Roadmap (near term)

- Stabilize the agentic loop (reason → tools → reflection)
- Improve container tool lifecycle and `/install` UX
- Ship a tray-style always-on Tauri runtime
- Opt-in remote providers behind explicit controls

## Principles

- Local first  
- User-controlled cost  
- Composable tools  
- Privacy by default  
