# Pengine CLI Plan (v3 - Tauri-only, implementation-corrected)

> Last updated: 2026-04-23.
> This version supersedes the earlier v2 draft and aligns with the current repository state.

## 1. Goal

Ship a first-class terminal surface for Pengine with the same domain services as the desktop app, while keeping model access constrained to the agent path (`ask` and free text in REPL/Telegram bridge), not native operational commands.

## 2. Scope and non-goals

### In scope

- One binary (`pengine`) with CLI branching through `tauri-plugin-cli`.
- Native subcommands for status/config/model/bot/tools/skills/fs/logs (and `app`, etc.).
- Agent path via `ask` and REPL free text.
- Telegram `$` bridge reusing the same router + handlers.
- Same feature that claude code - https://github.com/anthropics/claude-code

### Out of scope for initial delivery

- A separate non-Tauri runtime or a new `pengine-engine` product.
- Full dashboard parity in the same PR as CLI bootstrap stabilization.
- Telegram inline rerun/rollback buttons.

## 3. Current baseline (code already present)

Implemented in-tree:

- `src-tauri/src/modules/cli/` exists with:
  - `bootstrap.rs`, `commands.rs`, `dispatch.rs`, `handlers.rs`, `output.rs`,
    `repl.rs`, `router.rs`, `telegram_bridge.rs`
- `modules/agent/` exists and is used by both bot + CLI.
- `tauri-plugin-cli` is registered in `app.rs`.
- CLI schema is present in `src-tauri/tauri.conf.json`.
- **How to test:** [doc/guides/cli.md](doc/guides/cli.md) — `bun run cli -- …`,
  global flags **before** subcommands (`--json status`), `bun run cli:test` for
  `tests/cli_oneshot.rs`.

## 4. Architecture and boundaries

### 4.1 Module layout (authoritative for this feature)

```text
src-tauri/src/modules/cli/
├── mod.rs
├── bootstrap.rs
├── commands.rs
├── dispatch.rs          # shared router dispatch (REPL + Telegram `$`)
├── handlers.rs
├── output.rs
├── repl.rs
├── router.rs
└── telegram_bridge.rs   # `$` strip, MCP warmup, Telegram chunk formatting
```

### 4.2 Dependency rules

- Keep DDD direction: `infrastructure -> modules -> shared`.
- CLI handlers are adapters only. Business logic stays in existing domain services.
- No transport formatting in handlers (ANSI/fences/chunking stays in sinks).

## 5. CLI bootstrap contract

### 5.1 Entry behavior

`app.rs` calls `cli_bootstrap::handle_cli_or_continue(app)` before full UI initialization.

Expected contract:

1. CLI invocation (`--help`, `--version`, or configured subcommand): run handler and exit process with a deterministic code.
2. Non-CLI invocation (bare app open): return and continue normal UI startup.

### 5.2 Bootstrap validation

- One-shot exit is covered by `src-tauri/tests/cli_oneshot.rs` (`version`, `help`,
  `pengine --json status`). Run: `bun run cli:test`.
- Optional next: `ask` integration with a stubbed or absent Ollama (flaky in CI).

### 5.3 Stateful CLI process hydration

Current one-shot state build is too minimal for some commands. Stateful bootstrap must hydrate:

- `connection.json` metadata (`bot_id`, `bot_username`, `connected_at`).
- Keychain token only when needed by command semantics.
- `AppState.connection` where commands rely on in-memory connection data.

Without this, one-shot `status` and `bot disconnect` can misreport or skip token cleanup.

## 6. Command model

### 6.1 Source of truth

- `commands.rs` is the metadata registry for command names and summaries.
- Dispatch is still explicit in `bootstrap.rs` (one match arm per command).
- Therefore, adding a command is currently multi-point, not "one file edit".

Future improvement:

- Generate dispatch/help/http metadata from one typed registry to remove drift.

### 6.2 Native command mapping (corrected to current services)

| Command                                 | Backing behavior                                                                          |
| --------------------------------------- | ----------------------------------------------------------------------------------------- |
| `status`                                | Read hydrated connection state + `ollama::active_model` + MCP registry count + settings   |
| `config [k=v ...]`                      | `shared::user_settings` load/save + clamp                                                 |
| `model [name\|--clear]`                 | `state.preferred_ollama_model` + `ollama::model_catalog` validation                       |
| `bot connect <token>`                   | `bot_service::verify_token` + `secure_store::save_token` + `bot_repo::persist`            |
| `bot disconnect`                        | `bot_lifecycle::stop_and_wait_for_bot` + `bot_repo::clear` + `secure_store::delete_token` |
| `tools [search]`                        | MCP registry `all_tools()` after `mcp_service::rebuild_registry_into_state`               |
| `skills [list\|enable\|disable <slug>]` | `skills::service` list/set enabled                                                        |
| `fs <list\|add\|remove> [path]`         | MCP config load/mutate/save via `mcp_service` helpers                                     |
| `logs [--tail N] [--follow]`            | `AppState.log_tx.subscribe()` (follow implemented, historical tail pending)               |
| `ask <text>`                            | `modules::agent::run_turn`                                                                |
| `repl`                                  | `modules::cli::repl`                                                                      |
| `help`, `version`                       | native handlers + plugin parse support                                                    |

### 6.3 Agent path invariants

- Model path is only `ask` or free text in REPL/bridge.
- Native command names are never injected into model prompt context.

## 7. Router safety contract

Router outcomes:

- `Native { name, rest }`
- `Agent(text)`
- `Unknown(name)`

Invariant:

- `Unknown` must never fall through to `Agent`.
- Keep unit tests for this invariant on every router change.

## 8. Output contract

### 8.1 Reply envelope

Current `ReplyKind`:

- `Text`
- `CodeBlock { lang }`
- `Diff`
- `Log`
- `Error`

### 8.2 Sink status

Implemented:

- `TerminalSink`
- `JsonSink`
- Telegram `$` bridge: fenced + `split_by_chars` chunking in
  `telegram_bridge::telegram_cli_reply_chunks` (transport-specific; not the
  same trait as terminal sinks).

Planned:

- `FanOut` multiplexer for `--no-terminal` / `--no-telegram` when wired

### 8.3 JSON schema (actual current behavior)

Machine output is versioned and currently emitted as:

```json
{ "v": 1, "reply": { "kind": "text", "body": "..." } }
```

If we want top-level `kind/body`, do it as an explicit schema migration (v2), not as a silent shape change.

### 8.4 Flag parity issue to resolve

`--no-terminal` and `--no-telegram` exist in CLI schema/help text, but runtime currently only honors `--json`.

Decision required:

- Either implement sink toggling now, or remove unsupported flags until Telegram sink lands.

## 9. REPL

Current design is acceptable:

- `rustyline` line editing/history.
- `/exit`, `/quit`, `exit`, `quit` terminate loop.
- History path: `$APP_DATA/cli_history`.
- Best-effort MCP warmup before interactive loop.

Needed refinement:

- Route log streaming through sink abstractions where feasible (`logs --follow` currently prints directly).

## 10. Telegram bridge (PR C — implemented)

Behavior:

- In `bot/service.rs::text_handler`, messages whose trimmed text starts with `$`
  are handled **before** `agent::run_turn`.
- Payload after `$` is passed to `cli::telegram_bridge::run_telegram_cli_line`,
  which MCP-warmups (best-effort), then `cli::dispatch::dispatch_line` with
  `DispatchContext::telegram()` (e.g. `logs --follow` is rejected on Telegram).
- Replies are formatted (Markdown-style fences for code/log/diff) and split
  with `shared::text::split_by_chars` under the same UTF-16-safe budget as
  normal Telegram replies.

Policy:

- `$` prefix is CLI intent (router + native handlers + `/ask`-style slash paths).
- Non-`$` text stays on the normal agent conversation path.

Dependency note: `verify_token` lives in `modules/bot/token_verify.rs` so
`cli::handlers` does not import `bot::service`, avoiding a `bot → cli → bot`
cycle.

## 11. Single-instance + shim strategy (phase 3)

Status:

- `tauri-plugin-single-instance` is not wired yet.
- No implemented argv forwarding + response channel exists today.

Plan:

1. Validate if plugin callback surface can support request/response semantics for CLI output.
2. If not, add a small explicit local IPC mechanism for forwarded CLI output.
3. Only then add OS shim install/uninstall UX.

Keep one-binary objective; do not block CLI stabilization on this phase.

## 12. HTTP/dashboard parity (phase 4)

Implemented:

- `GET /v1/cli/commands` — JSON from `modules::cli::commands::COMMANDS` (see `http_server.rs`).
- Dashboard: `CliCommandsPanel` on the main dashboard (fetches the endpoint).

Further work (optional): richer panel (copy snippets, deep links), `doc/guides/cli.md`.

## 13. Acceptance criteria status

| Criterion                                | Status on 2026-04-24 | Notes                                                |
| ---------------------------------------- | -------------------- | ---------------------------------------------------- |
| CLI command list exists                  | Implemented          | Registry + `GET /v1/cli/commands` + dashboard panel  |
| One-shot CLI execution exits reliably    | Partial              | `cli_oneshot` tests + `bun run cli`; packaged builds TBD |
| Agent path isolated from native commands | Implemented          | Router invariant; `$` bridge uses same dispatch    |
| Versioned JSON output                    | Implemented          | Shape is `{"v":1,"reply":...}`                       |
| Terminal REPL                            | Implemented          | `rustyline` + history                                |
| Telegram CLI bridge                      | Implemented          | `$` path + `telegram_bridge.rs` + `dispatch.rs`      |
| `--no-terminal` / `--no-telegram` flags  | Pending              | Declared, not honored                                |
| `/v1/cli/commands` API + dashboard list  | Implemented          | Axum route + `CliCommandsPanel`                      |
| Single-instance forwarding + shim UX     | Partial              | Dashboard `pengine-cli` launcher install/remove; single-instance TBD |
| SSH guide + setup script                 | Pending              | Docs/script not present                              |

## 14. Delivery sequence (updated)

1. **PR A - Stabilize bootstrap (blocking)**
   - Fix one-shot CLI termination behavior.
   - Hydrate state correctly for connection-aware commands.
   - Add integration tests for one-shot commands.
2. **PR B - Contract cleanup**
   - Resolve unsupported flag mismatch.
   - Align JSON contract docs with emitted envelope.
   - Route streaming output through sinks where practical.
3. **PR C - Telegram bridge** (done in tree)
   - `$` path in `bot/service.rs::text_handler` before `agent::run_turn`.
   - `cli/telegram_bridge.rs` + `cli/dispatch.rs`; chunk-safe fenced output.
   - Optional follow-up: unify streaming/`println` paths with sink traits.
4. **PR D - Parity and operability** (partially done)
   - `GET /v1/cli/commands` + dashboard `CliCommandsPanel` — merged.
   - Remaining: single-instance forwarding + shim UX; CLI guides (`doc/guides/cli.md`, `doc/guides/cli-ssh.md`); optional sink/`--no-terminal` polish from PR B.

## 15. Required documentation sync

Must update with CLI work:

- `doc/README.md` feature map currently references old agent path (`modules/bot/agent.rs`).
- `doc/architecture/README.md` backend tree still shows `bot/agent.rs`.
- Add CLI guides only when behavior is stable and tested.

## 16. Decision log (defaults)

- Keep `$` as Telegram CLI prefix.
- Keep JSON envelope version field from day one.
- Keep Tauri-only single-binary direction.
- Defer broader core/engine tree migration from this CLI track.
