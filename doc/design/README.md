# Pengine — Architecture & DDD Design Reference

> **Agent rule:** Before adding, moving, or renaming any file in `src/` or `src-tauri/src/`, read this document first. It defines where code lives and why.

---

## What Is Pengine?

A Tauri v2 desktop app. The frontend (React + TypeScript, built with Vite) talks to a loopback HTTP server embedded in the Tauri Rust backend. The backend connects to Telegram (via teloxide) and Ollama (local inference) on behalf of the user.

---

## Top-Level Layout

```
pengine/
├── src/                  # Frontend — React + TypeScript
├── src-tauri/            # Backend — Rust (Tauri + Axum + teloxide)
├── e2e/                  # Playwright end-to-end tests
├── doc/design/           # Architecture docs (this file)
├── eslint.config.ts
├── .prettierrc
└── package.json
```

---

## Frontend — `src/`

### Folder Structure

```
src/
├── main.tsx              # React entry point
├── App.tsx               # Router + startup health-check redirect logic
├── index.css
├── styles/
│   └── utilities.css
├── assets/
├── pages/                # Route-level components (one file per route)
│   ├── LandingPage.tsx
│   ├── SetupPage.tsx
│   └── DashboardPage.tsx
├── modules/              # Feature modules (DDD bounded contexts)
│   ├── bot/
│   │   ├── api/index.ts        # Fetch wrappers for Pengine loopback API
│   │   ├── components/
│   │   │   ├── SetupWizard.tsx
│   │   │   └── TerminalPreview.tsx
│   │   ├── store/
│   │   │   └── appSessionStore.ts  # Zustand store (persisted to localStorage)
│   │   ├── types.ts            # PengineHealth and related types
│   │   └── index.ts            # Public barrel export
│   └── ollama/
│       ├── api/index.ts        # fetchOllamaModel — probes local Ollama daemon
│       ├── types.ts            # OllamaProbe
│       └── index.ts            # Public barrel export
└── shared/
    ├── api/
    │   └── config.ts           # PENGINE_API_BASE, OLLAMA_API_BASE constants
    └── ui/                     # Reusable presentational components
        ├── TopMenu.tsx
        ├── WizardLayout.tsx
        ├── PhoneMockup.tsx
        ├── SpecMockup.tsx
        └── StyledQrCode.tsx
```

### Frontend Layer Rules

| Layer | Path | Allowed imports |
|---|---|---|
| `pages/` | `src/pages/` | `modules/*`, `shared/*` |
| `modules/` | `src/modules/<name>/` | `shared/*`, own module internals |
| `shared/` | `src/shared/` | Nothing from `modules/` or `pages/` |

- **Pages** compose module components and wire routing. No business logic.
- **Modules** own their api calls, state, components, and types. A module imports only from `shared/` or its own subtree.
- **Shared** is utility/primitive only — no domain knowledge, no feature state.
- Cross-module imports are **not allowed**. If two modules need the same thing, extract it to `shared/`.

### Key Frontend Files

- `src/shared/api/config.ts` — single source of truth for base URLs (`http://127.0.0.1:21516` for Pengine, `http://127.0.0.1:11434` for Ollama). Change ports here only.
- `src/modules/bot/api/index.ts` — all fetch calls to the Rust loopback server (`/v1/connect`, `/v1/health`, `/v1/logs`).
- `src/modules/bot/store/appSessionStore.ts` — Zustand store for bot connection state, persisted to localStorage under key `pengine-device-session`.
- `src/App.tsx` — on startup, polls `getPengineHealth()` and redirects to `/dashboard` if a bot is already connected (avoids landing on setup after restart).

---

## Backend — `src-tauri/src/`

### Folder Structure

```
src-tauri/src/
├── main.rs               # Binary entry (calls lib::run)
├── lib.rs                # Declares top-level modules, calls app::run()
├── app.rs                # Tauri builder: registers commands, spawns HTTP server
├── shared/
│   ├── mod.rs
│   └── state.rs          # AppState, ConnectionData, LogEntry — shared across all layers
├── modules/
│   ├── mod.rs
│   ├── bot/
│   │   ├── mod.rs
│   │   ├── commands.rs   # Tauri IPC commands (get_connection_status, disconnect_bot)
│   │   ├── repository.rs # File-based persistence (persist / load / clear)
│   │   └── service.rs    # Bot lifecycle (verify_token, start_bot, message handlers)
│   └── ollama/
│       ├── mod.rs
│       ├── constants.rs  # OLLAMA_PS_URL, OLLAMA_TAGS_URL, OLLAMA_CHAT_URL
│       └── service.rs    # active_model(), chat() — HTTP calls to local Ollama
└── infrastructure/
    ├── mod.rs
    ├── http_server.rs    # Axum server: route definitions + HTTP handlers
    └── bot_lifecycle.rs  # stop_and_wait_for_bot() — graceful shutdown helper
```

### Backend Layer Rules

| Layer | Path | Responsibility |
|---|---|---|
| `shared/` | `src-tauri/src/shared/` | Types shared across all layers; no domain logic |
| `modules/` | `src-tauri/src/modules/` | Domain logic, isolated per bounded context |
| `infrastructure/` | `src-tauri/src/infrastructure/` | Transport (HTTP, Tauri IPC); imports from `shared/` and `modules/` |
| `app.rs` | root | Wiring only — instantiates state, registers Tauri commands, spawns tasks |

**Dependency direction:** `infrastructure` → `modules` → `shared`. Never the reverse.

### Why `ConnectionData` Lives in `shared/state.rs`

`ConnectionData` is defined next to `AppState` (not inside `modules/bot/`) because `AppState` holds a `Mutex<Option<ConnectionData>>` and `AppState` is imported by `modules/bot/service.rs`. Splitting them would create a circular dependency (`shared → bot → shared`). Rule: types owned by `AppState` belong in `shared/state.rs`.

### Key Backend Files

- `src-tauri/src/shared/state.rs` — `AppState` is the single shared handle cloned into every Axum handler and Tauri command. It holds the Tokio broadcast channel for SSE logs, the bot running flag, the connection data, and the store path.
- `src-tauri/src/infrastructure/http_server.rs` — Axum router. Port `21516`. Routes: `POST /v1/connect`, `DELETE /v1/connect`, `GET /v1/health`, `GET /v1/logs` (SSE). Bind uses `SO_REUSEADDR` + retry loop for fast restarts.
- `src-tauri/src/modules/bot/repository.rs` — Persists `ConnectionData` as JSON to a single file at `$APP_DATA/connection.json`. `clear()` uses direct `remove_file` (not existence check first) to avoid TOCTOU.
- `src-tauri/src/modules/bot/service.rs` — `verify_token` calls Telegram `getMe`. `start_bot` runs the teloxide dispatcher and sets `bot_running` flag on entry/exit.
- `src-tauri/src/infrastructure/bot_lifecycle.rs` — `stop_and_wait_for_bot` fires `shutdown_notify`, then polls `bot_running` every 50 ms up to 30 s before giving up.

---

## Communication Flow

```
User types bot token
        │
        ▼
SetupWizard (frontend)
  POST /v1/connect  ──────────────────────────────►  http_server::handle_connect
                                                           │
                                                    verify_token (Telegram getMe)
                                                           │
                                                    persist to disk
                                                           │
                                                    spawn start_bot (teloxide)
                                                           │
                                                    ◄── ConnectResponse { bot_id, bot_username }
        │
        ▼
appSessionStore.connectDevice()  →  localStorage
        │
        ▼
redirect to /dashboard
        │
DashboardPage polls GET /v1/health every 5 s
DashboardPage streams GET /v1/logs (SSE)
```

Incoming Telegram messages flow:

```
Telegram  ──►  teloxide dispatcher  ──►  text_handler
                                              │
                                       ollama::active_model()
                                              │
                                       ollama::chat(model, text)
                                              │
                                       bot.send_message(reply)
```

---

## Modules

- [MCP — Model Context Protocol](./mcp.md) — agent tool-use via external MCP servers (POC).

---

## Adding a New Module

### Frontend

1. Create `src/modules/<name>/` with `api/index.ts`, `types.ts`, `index.ts`.
2. Export public surface through `index.ts` only.
3. Import in pages via `../../modules/<name>`.
4. Do not import between sibling modules.

### Backend

1. Create `src-tauri/src/modules/<name>/` with `mod.rs`, `service.rs`, and whatever else is needed.
2. Register the module in `src-tauri/src/modules/mod.rs` (`pub mod <name>;`).
3. If the module exposes Tauri IPC commands, add a `commands.rs` and register them in `app.rs`.
4. Keep HTTP handlers in `infrastructure/http_server.rs`, not in the module itself.

---

## Tooling Quick Reference

| Task | Command |
|---|---|
| Dev server | `bun run dev` |
| Type check | `bun run typecheck` |
| Lint (TS) | `bun run lint` |
| Format (TS) | `bun run format` |
| Rust format check | `bun run rust:fmt` |
| Rust lint | `bun run rust:lint` |
| Rust format + lint | `bun run rust:check` |
| Auto-format Rust | `cargo fmt --all --manifest-path src-tauri/Cargo.toml` |
| E2E tests | `bun run test:e2e` |
| Tauri dev | `bun run tauri dev` |

Pre-commit hook runs: `lint-staged` → `typecheck` → `rust:check`.
