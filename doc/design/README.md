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
├── tools/                # MCP tool images, bundled skills, mcp-tools.json
├── e2e/                  # Playwright end-to-end tests
├── doc/
│   ├── README.md            # Doc index + feature map (start here)
│   ├── skills.md
│   ├── custom-mcp-tools.md
│   ├── design/              # Architecture (this file)
│   └── tool-engine/
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
│   │   ├── api/index.ts        # Fetch wrappers: /v1/connect, /v1/health
│   │   ├── components/         # Setup wizard, terminal preview
│   │   ├── store/appSessionStore.ts
│   │   ├── types.ts
│   │   └── index.ts
│   ├── ollama/
│   │   ├── api/index.ts        # /v1/ollama/models, model PUT
│   │   ├── types.ts
│   │   └── index.ts
│   ├── mcp/ # Dashboard MCP tools UI + API helpers
│   ├── toolengine/             # Tool Engine catalog panel
│   ├── skills/                 # Skills + ClawHub UI
│   └── settings/               # User settings API (e.g. skills byte cap)
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
- `src/modules/bot/api/index.ts` — core loopback calls (`/v1/connect`, `/v1/health`). Other modules own their routes (MCP, Tool Engine, skills, Ollama, settings).
- `src/modules/bot/store/appSessionStore.ts` — Zustand store for bot connection state, persisted to localStorage under key `pengine-device-session`.
- `src/App.tsx` — after Zustand hydration, redirects `/` → `/dashboard` when session says connected, or when health reports a connected bot (skips `/setup` so the wizard can load).

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
│   ├── state.rs          # AppState, ConnectionData, LogEntry, …
│   ├── user_settings.rs  # Defaults for skills hint size, etc.
│   ├── keywords.rs       # Keyword matching helpers
│   └── text.rs           # Truncation / output shaping for the model
├── modules/
│   ├── mod.rs
│   ├── bot/              # Telegram, agent loop, persistence
│   │   ├── agent.rs
│   │   ├── service.rs
│   │   ├── repository.rs
│   │   ├── commands.rs
│   │   └── search_followup.rs
│   ├── ollama/
│   ├── mcp/              # MCP client, registry, native tools, config load
│   ├── tool_engine/      # Catalog fetch, install/uninstall, custom Docker tools
│   ├── skills/           # Skill dirs, ClawHub, HTTP handlers delegate here
│   ├── memory/           # Memory MCP adapter + keyword-driven sessions
│   ├── secure_store/     # OS keychain / keyring for secrets
│   └── keywords/         # Phrase lists shared with agent behavior
└── infrastructure/
    ├── mod.rs
    ├── http_server.rs    # Axum: /v1 REST + SSE
    ├── bot_lifecycle.rs  # Graceful bot shutdown
    └── executable_resolve.rs
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
- `src-tauri/src/infrastructure/http_server.rs` — Axum router on **`127.0.0.1:21516`**. Core routes plus **`/v1/ollama/*`**, **`/v1/mcp/*`**, **`/v1/toolengine/*`**, **`/v1/skills/*`**, **`/v1/settings`**. Full list is the `Router::new()` chain in this file. Bind uses `SO_REUSEADDR` + retry loop for fast restarts.
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
DashboardPage refreshes health + Ollama status on an interval (10 s) and can open the log stream (SSE) from the MCP/tools UI
```

Incoming Telegram messages flow (simplified):

```
Telegram  -->  teloxide dispatcher  -->  text_handler
                                              |
                                       bot::agent::run_turn
                                              |
                              Ollama chat + tools  <--> mcp::registry (tools/call)
                                              |
                                       bot.send_message(reply)
```

`run_turn` also applies memory/skills context and enforces agent policies (step cap, duplicate fetch, Brave search limits); see `src-tauri/src/modules/bot/agent.rs`.

---

## Modules

- [MCP — Model Context Protocol](./mcp.md) — agent tool-use via MCP servers (stdio, native, Docker).

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
