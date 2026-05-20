# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

### Development
```bash
bun install          # install JS dependencies
bun run dev          # Vite dev server (web UI only, no Rust)
bun run tauri dev    # full Tauri desktop app (Rust + web UI)
bun run cli --       # interactive REPL (same as bare `pengine` on TTY)
bun run cli -- <cmd> # one-shot CLI command (version, status, ask "…", …)
```

### Testing
```bash
# Rust unit + integration tests
cargo test --manifest-path src-tauri/Cargo.toml

# Single Rust test by name
cargo test --manifest-path src-tauri/Cargo.toml <test_name>

# One-shot CLI integration tests (requires built binary)
bun run cli:test

# End-to-end (Playwright)
bun run test:e2e
bun run test:e2e:ui   # interactive UI mode
```

### Linting and formatting
```bash
# TypeScript
bun run typecheck     # tsc --noEmit
bun run lint          # eslint
bun run format        # prettier

# Rust
bun run rust:fmt      # cargo fmt
bun run rust:lint     # cargo clippy -D warnings
bun run rust:check    # fmt + clippy together (what pre-commit runs)

# Auto-format Rust
cargo fmt --all --manifest-path src-tauri/Cargo.toml
```

### Build
```bash
bun run build         # web UI only (tsc + vite)
cargo build --manifest-path src-tauri/Cargo.toml  # Rust binary
```

Pre-commit hook runs: `lint-staged` → `typecheck` → `rust:check`.

## Architecture overview

Pengine is a **Tauri v2 desktop app** — a Rust backend embedding Axum on `127.0.0.1:21516`, plus a React/TypeScript frontend served by Vite. The same binary also acts as a terminal CLI and Telegram bot.

### Message flow

```
Telegram message → teloxide dispatcher → agent::run_turn
                                              ↓
                              Ollama chat + MCP tool loop
                                              ↓
                                      bot.send_message
```
The terminal REPL takes the same path: user input → `handlers::ask_in_session` → `agent::run_turn`.

### Backend: `src-tauri/src/`

Strict dependency direction: `infrastructure/` → `modules/` → `shared/`. Never reverse.

- **`shared/state.rs`** — `AppState` is the single handle cloned into every handler and command. Holds the Tokio broadcast log channel (`log_tx`), bot running flag, connection data, preferred model, and store path.
- **`infrastructure/http_server.rs`** — Axum router. All REST routes live here. The route table is the `Router::new()` chain in this file.
- **`modules/agent/mod.rs`** — The core loop. Public API: `run_turn` (user-facing, wires text streaming) and `run_system_turn` (internal, no streaming, used by compaction and cron). Key constants: `MAX_STEPS = 6`, `MAX_STEPS_APPLY_FIX = 10`, `POST_TOOL_NUM_PREDICT = 1024`.
- **`modules/ollama/service.rs`** — Ollama HTTP client. `ChatOptions::default()` uses `num_ctx: 16384` and `keep_alive: "30m"`. `text_chunk_tx: Option<UnboundedSender<String>>` enables live streaming to the REPL.
- **`modules/mcp/`** — MCP host (stdio + HTTP transports), tool registry, native built-in tools (`mcp/native.rs`). Three native server IDs: `task_spawner` (recursive subagent, `TASK_SPAWN_MAX_DEPTH = 1`), `tool_manager`, `cron_manager`.
- **`modules/cli/`** — Terminal CLI and REPL. Key files: `router.rs` (classify slash-commands vs. agent text), `dispatch.rs` (route to handlers), `handlers.rs` (business logic + streaming consumer), `output.rs` (REPL chrome, spinner, `ProgressHandle`), `session.rs` (turn history, `.pengine` file, compaction prompt).
- **`modules/bot/service.rs`** — teloxide Telegram dispatcher.
- **`modules/skills/`** — `SKILL.md` recipes injected into the system prompt.
- **`modules/memory/`** — Session/diary keyword-driven memory MCP adapter.
- **`modules/cron/`** — Scheduled agent turns.
- **`shared/text.rs`** — `PENGINE_OUTPUT_CONTRACT_LEAD` forces all model replies into `<pengine_reply>…</pengine_reply>`. `normalize_assistant_message_content` extracts the tag content (or falls back through several heuristics).

### Frontend: `src/`

Layer rules (cross-module imports are forbidden):
- `pages/` → composes `modules/*` and `shared/*`
- `modules/<name>/` → only `shared/*` and own subtree
- `shared/` → nothing from `modules/` or `pages/`

Key files:
- `src/shared/api/config.ts` — single source of truth for base URLs (`http://127.0.0.1:21516` Pengine API, `http://127.0.0.1:11434` Ollama).
- `src/App.tsx` — redirects `/` → `/dashboard` when bot is already connected.
- `src/modules/bot/store/appSessionStore.ts` — Zustand connection state, persisted to `localStorage` under `pengine-device-session`.

## Key invariants

**Output contract:** All model responses must wrap user-visible text in `<pengine_reply>…</pengine_reply>`. Private chain-of-thought goes in `<pengine_plan>…</pengine_plan>`. `normalize_assistant_message_content` enforces this on the way out.

**CLI command isolation:** Native slash commands (`/status`, `/model`, `/session`, etc.) are never forwarded to the model. `RouterOutcome::Unknown` is an error to the user, not an agent message.

**MCP config mutex:** Any code that reads then writes `mcp.json` must hold `AppState.mcp_config_mutex` for the duration to prevent TOCTOU races.

**`task_spawn` depth:** Subagents spawned by `task_spawn` cannot themselves spawn subagents (`TASK_SPAWN_MAX_DEPTH = 1`). `run_task_spawn_inline` calls `run_system_turn` (which passes `None, None` for tokens/streaming) — not `run_turn`.

**Streaming consumer and spinner:** `ProgressHandle::finish()` must be called after the streaming consumer task completes (`stream_task.await`). The consumer calls `ProgressStatus::stop_spinner()` and waits 95 ms before printing to stdout so the spinner can clear its stderr line cleanly.

**`.pengine` context files** are sanitized through `sanitize_dot_pengine` before being included in the agent prompt — lines matching injection patterns are redacted.

## Data and config paths

- Bot token: OS keychain (never on disk).
- `$APP_DATA/connection.json` — bot metadata (id, username).
- `$APP_DATA/mcp.json` — MCP server config.
- `$APP_DATA/skills/` — custom skills.
- `$APP_DATA/cli_sessions/` — REPL session history.
- `$APP_DATA/logs/audit-<YYYY-MM-DD>.log` — NDJSON audit log.

## Documentation index

Detailed references in `doc/`:
- `doc/architecture/README.md` — DDD boundaries, frontend/backend layer rules, adding modules
- `doc/agent/runtime.md` — `run_turn` internals, tool routing, system prompt construction, step policies
- `doc/platform/data-and-startup.md` — `AppState`, boot order, disk paths
- `doc/reference/http-api.md` — all `/v1/*` routes
- `doc/guides/cli.md` — CLI flags, REPL commands, session management, `@file` mentions
- `doc/guides/skills.md` — skill format, enabling/disabling, prompt injection
- `doc/guides/custom-mcp-tools.md` — `mcp.json`, Docker/stdio servers
