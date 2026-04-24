# Pengine terminal CLI (testing and daily use)

The desktop app binary (`pengine`) also handles **native CLI** commands via
[`tauri-plugin-cli`](https://v2.tauri.app/plugin/cli/). There is no separate CLI
executable. The main webview window is created only when the app stays in **GUI
mode** (after setup), so a terminal **`pengine`** session never opens a webview in that process.

## `pengine` (shell) vs `pengine app` (window)

- **`pengine`** with no subcommand in a **real terminal (TTY)** → interactive shell only (REPL). That process is terminal-only (no menu-bar / Dock “app open” state tied to a GUI window from this invocation).
- **`pengine app`** → starts the **desktop UI in a separate process**. You can leave a **`pengine`** shell running in one terminal and **`pengine app`** in another (or run the app from Finder); they can run in parallel.
- **`pengine --shell`** — with no subcommand, never opens the GUI **in-process**; exits with an error if there is no TTY (same idea as the **`pengine-cli`** launcher).
- **No TTY** (Finder / Dock / `.desktop` / Windows Start menu / `open -a pengine`) — the process opens the **GUI window** on every platform, unless `--shell` / `PENGINE_LAUNCH_MODE=cli` explicitly forces terminal-only mode.

macOS does **not** put the dev build on `PATH` automatically, so a bare `pengine`
in Terminal fails with `command not found` until you either:

1. **Add the repo `scripts/` folder to `PATH`** (recommended for development).  
   The repo includes `scripts/pengine`, a small launcher that runs
   `src-tauri/target/debug/pengine` from your clone.

   ```bash
   # Replace with the path where you cloned pengine
   export PATH="/Users/you/Projects/agents/pengine/scripts:$PATH"
   ```

   Add that line to `~/.zshrc` (or `~/.bashrc`), open a new terminal, then:

   ```bash
   pengine version
   ```

   The first time, build the real binary once:

   ```bash
   cd /path/to/pengine && cargo build --manifest-path src-tauri/Cargo.toml
   ```

2. **Or** call the binary by full path (no `PATH` change):

   ```bash
   /path/to/pengine/src-tauri/target/debug/pengine version
   ```

3. **Or** stay inside the repo and use **`bun run cli -- …`** (see below).

Packaged app installs may expose `pengine` differently; this guide focuses on
**local development**.

## Quick test (development tree)

From the **repository root**:

```bash
bun run cli --              # interactive REPL + ASCII welcome (same as bare `pengine` in a real terminal)
bun run cli -- version
bun run cli -- help
bun run cli -- status
```

**Bare `pengine`:** when stdin is a **terminal** (TTY) and you pass no
subcommand, the process stays in the REPL only (**no GUI** in that process).
macOS **Finder / Dock** still use a non-TTY launch with `-psn_…`, and that path
starts the GUI **in-process** as before. From a script without a TTY (and no
Finder arg), use **`pengine app`** for the window or **`pengine status`** (etc.)
for one-shots.

These run `cargo run --manifest-path src-tauri/Cargo.toml -- …`, so the first
build can take a while.

Direct **Cargo** (same effect):

```bash
cargo run --manifest-path src-tauri/Cargo.toml -- version
```

### Installed app (Tauri desktop): add `pengine-cli` to the terminal

A normal **.dmg / .app** install does **not** change your shell `PATH` (macOS
Gatekeeper and security best practice). After you open the installed app once:

1. Open **Dashboard** → **Terminal CLI** panel.
2. Turn **CLI on PATH** on (writes `~/.local/bin/pengine-cli` on macOS/Linux, or
   `%LOCALAPPDATA%\Pengine\bin\pengine-cli.cmd` on Windows). The launcher sets
   `PENGINE_LAUNCH_MODE=cli` and runs the same binary as the app, so terminal use
   matches `bun run cli` (REPL when you run it with no args in a TTY; one-shots
   with subcommands). It does **not** open the GUI when stdin is not a TTY.
3. If the panel says the launcher directory is not on `PATH`, add the suggested
   `export PATH=…` line to `~/.zshrc` (or Windows user PATH), then open a **new**
   terminal. Use **`pengine-cli`** for the shell, **`pengine-cli app`** for the window.

Re-toggle **CLI on PATH** after moving or updating the app if you want the launcher
to track the new binary path.

The app bundle still contains the **`pengine`** binary; **`pengine-cli`** on `PATH` is the terminal-first launcher (same idea as dev **`bun run cli`**).

Fully automatic PATH changes **at install time** would require a custom
installer (e.g. macOS `.pkg` postinstall script); the dashboard flow keeps that
explicit and reversible (**Remove launcher**).

## Global flags (order matters)

Flags declared at the **root** of the CLI schema (`--json`, `--no-terminal`,
`--no-telegram`) must appear **before** the subcommand, for example:

```bash
pengine --json status
```

Not:

```bash
pengine status --json   # rejected by the CLI parser
```

`pengine help` documents the native command names; machine-readable metadata is
also available from the local HTTP API: `GET /v1/cli/commands`.

## `tauri dev` and arguments

To pass CLI args through the Tauri dev runner, put them **after** `--` so they
reach the app binary, for example:

```bash
bun run tauri dev -- -- version
```

Without subcommands after `--`, the app starts in **GUI** mode as usual.

## What to expect

- **One-shot commands** (`version`, `help`, `status`, `ask`, …) should print and
  exit with code **0** (or non-zero on error), without leaving a window open.
- **Bare `pengine`** (TTY, no subcommand) starts an interactive session (line editor + history); exit with
  `/exit`, `exit`, `quit`, or Ctrl+D.
- **`logs --follow`** streams until interrupted (Ctrl+C); avoid it in
  automation unless you plan to kill the process.

## Interactive feedback (REPL + `ask`)

The REPL renders Claude-Code-style:

```
❯ what changed in the fetch tool?
  ⎿  · called fetch (step 0)
  ⎿  · fetch: 4012 bytes
  ⎿  Baked for 4.8s
  ⎿  The fetch tool now deduplicates URLs per user message …
```

- **Prompt**: bold-cyan `❯ ` on a TTY, plain `> ` when stdout is piped.
- **Reply prefix**: `  ⎿  ` on the first line, five-space continuation for the rest. Replies are coloured by [`ReplyKind`](../../src-tauri/src/modules/cli/output.rs): diff blocks get green `+` / red `-`, code blocks print raw.
- **Inline tool-event blocks**: while `ask` / a free-text REPL line is running, each `"tool"` log event (call start, `name: N bytes` result, errors, host auto-fetch) is printed as its own persistent `  ⎿  · …` line above the spinner (`handlers::inline_tool_block`). This is Claude-Code-like per-step visibility without touching the agent loop — events come from the existing `AppState.log_tx` broadcast.
- **Thinking spinner** (free-text or `/ask …`): between tool events a braille spinner on **stderr** tags the latest `run` / `tool_ctx` / `mcp` / `ollama` event, e.g. `⠋ Thinking · tool_ctx: ranked 4/22 · 2.3s`. The spinner is suppressed when stderr is not a TTY, so `--json`, CI, and piped output stay clean.
- **Elapsed summary**: after the turn finishes the spinner line is cleared and replaced with `  ⎿  Baked for 4.8s`, matching the reply prefix.
- **Diff blocks from the agent**: if the agent's reply contains ` ```diff … ``` ` fences, each fence is pulled out and rendered as its own coloured diff block; surrounding prose stays as text (see `output::split_text_into_blocks`).

## Audit log (`"kind":"cli"`)

Every CLI action lands in `{store_dir}/logs/audit-<YYYY-MM-DD>.log` (JSON-lines, same shape as the in-memory log broadcast) alongside the bot / MCP / agent events. Two kinds of audit lines:

- **One-shot subcommand**: `bootstrap::cli_subcommand_audit_summary` emits `pengine <name> …` with secrets redacted (e.g. `pengine bot connect <redacted>`) and long args truncated (~400 chars for `config`, 800 for `ask`, etc.).
- **REPL line**: `dispatch::format_repl_line_for_audit` emits `repl <line>` with the same redaction rules (case-insensitive `/bot connect` and `bot connect` are both caught).

Tail the last N audit entries from the CLI itself:

```
pengine logs --tail 100
```

That reads the newest files under `{store_dir}/logs/` backwards until N lines are collected (or no older files remain). For an on-disk grep:

```bash
store=$(pengine status | awk '/^store:/ {print $2}' | xargs dirname)
rg '"kind":"cli"' "$store"/logs/audit-$(date +%Y-%m-%d).log
```

(`pengine status` prints the `connection.json` path; its parent directory holds the `logs/` folder. `secure_store` keys never touch the audit JSON.)

## Known gaps vs Claude Code

These are deliberate omissions for the current feature set — tracked for later but not implemented today:

- **Streaming tool-call result bodies inside a reply**: each tool call now shows as its own `  ⎿  · …` line, but the **full** result body is still collapsed. Claude Code shows expandable tool outputs; Pengine only shows the one-line summary (`name: N bytes`, `name error: …`). Surfacing full bodies would need `agent::run_turn` to forward content, not just `emit_log` notices.
- **Inline Telegram buttons** (rerun / rollback): explicitly deferred (`cli_plan.md` §11).

## Automated checks

```bash
bun run cli:test
```

Runs `tests/cli_oneshot.rs` (spawns `target/debug/pengine`). Requires a
successful `cargo build` for the binary to exist.

## Linux / headless note

Tauri still initializes the GUI stack on Linux. If `cargo run` / tests fail
with display errors, run under a virtual framebuffer (example):

```bash
xvfb-run -a cargo run --manifest-path src-tauri/Cargo.toml -- version
```

## Telegram

Messages starting with **`$`** are treated as the same router surface as the
REPL (native `/…` commands or free text to the agent). Normal messages (no
`$`) go straight to the agent.
