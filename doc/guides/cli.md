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

Flags declared at the **root** of the CLI schema must appear **before** the
subcommand, for example:

```bash
pengine --json status
```

Not:

```bash
pengine status --json   # rejected by the CLI parser
```

| Flag | Purpose |
| --- | --- |
| `--json` | Emit a versioned JSON envelope per reply (`{"v":1,"reply":{…}}`). |
| `-p`, `--print "<prompt>"` | Non-interactive: run one agent turn on `<prompt>` and exit. |
| `--output-format <fmt>` | With `-p`: `text` (default), `json`, or `stream-json`. |
| `--continue` | Resume the most recent saved REPL session (works with bare `pengine`, `pengine -p`, and `pengine ask`). |
| `--shell` | With no subcommand, require a TTY for the REPL; never open the GUI in-process. |
| `-V`, `--version` | Print version and exit. |
| `-h`, `--help [topic]` | Print help; with `[topic]`, print detailed help for that command. |
| `--no-terminal`, `--no-telegram` | Reserved for future sink routing. |

`pengine help [command]` documents the native command names; per-command details
include usage examples. Machine-readable metadata is also available from the
local HTTP API: `GET /v1/cli/commands`.

### Non-interactive mode (`-p`)

Stream a single prompt through the agent and exit — useful for scripts:

```bash
pengine -p "summarize the last hour of MCP tool errors"
pengine --json -p "what's my preferred model?"
pengine --output-format stream-json -p "..."
```

Combine with `--continue` to keep using the most recent REPL session (the
session's prior summary + last few turns are prepended automatically):

```bash
pengine --continue -p "and what about the failures yesterday?"
```

## `tauri dev` and arguments

To pass CLI args through the Tauri dev runner, put them **after** `--` so they
reach the app binary, for example:

```bash
bun run tauri dev -- -- version
```

Without subcommands after `--`, the app starts in **GUI** mode as usual.

## What to expect

- **One-shot commands** (`version`, `help`, `status`, `ask`, `doctor`, …)
  print and exit with code **0** (or non-zero on error), without leaving a
  window open.
- **Bare `pengine`** (TTY, no subcommand) starts an interactive session (line
  editor + history). Exit with:
  - `/exit`, `/quit`, `exit`, `quit`, Ctrl+D, **or**
  - **double Ctrl+C** within 2 seconds — the first Ctrl+C clears the line,
    the second exits.
- **Multi-line input**: end a line with `\` to continue on the next line. The
  joined message is sent on the first line that does **not** end with `\`.
- **Clear screen**: `/clear` (or bare `clear`) clears the REPL, the same as
  Ctrl+L.
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

## Sessions: `/compact`, `/resume`, `/cost`, `--continue`

`pengine` keeps an in-memory session of the REPL turns + token totals.
The session is persisted on every successful agent turn to:

- `{store_dir}/cli_sessions/<id>.json` — full turn record
- `{store_dir}/cli_session_last.json` — pointer to the most recent session

Each new user message is decorated with a context prefix built from the
session's optional summary plus the last **6 turns** / **12 KB** of history,
so prompt size stays bounded across long sessions.

| Command | Effect |
| --- | --- |
| `/cost` | Token totals for the active session + heuristic cloud cost estimate ($1/$3 per M in/out). Local models report $0. |
| `/compact` | Calls the model to summarize the transcript, replaces the turn history with that summary, and saves. Use when the prefix budget gets tight or you want to start fresh without losing context. |
| `/resume` | Loads the most recent saved session into the current REPL. |
| `pengine --continue` | One-shot equivalent of `/resume`; works with bare `pengine` (REPL), `pengine -p "..."`, and `pengine ask "..."`. |

## First-run folder trust prompt

When you start the **REPL** inside a directory that is not yet covered by an
MCP filesystem root, Pengine asks once — same idea as Claude Code's "trust
this folder" prompt:

```
  ⎿  /Users/you/Projects/myapp
     Add this folder to Pengine's MCP filesystem roots? [y/n]
```

- **`y` / `yes`** — adds the folder to `mcp.json` (same effect as
  `pengine fs add <path>`) and records the choice so you're never asked
  again for that path. The decision also covers any subdirectory.
- **`n` / `no`** — saves the path on the deny list so you're not asked
  again.
- **Any other answer (or `Ctrl+C`)** — skipped; you'll be asked again next
  launch.

Decisions live in `{store_dir}/folder_trust.json`:

```json
{
  "trusted": ["/Users/you/Projects/myapp"],
  "denied": ["/private/tmp"]
}
```

The prompt is skipped entirely when:

- stdin is not a TTY (one-shot commands, `pengine -p`, scripts, CI),
- the cwd is already under an existing MCP fs root,
- the cwd has already been decided (in `trusted` or `denied`),
- the cwd is the filesystem root.

To revoke trust later, edit `folder_trust.json` directly and remove the
folder from `mcp.json` via `pengine fs remove <path>`.

## `@file` mentions

In the REPL or `pengine ask`, tokens like `@README.md` or `@/abs/path/to/file`
are detected and the file content is appended to the prompt under a
`## Mentioned files` block. Trailing punctuation (`,` `:` `.` `)` `]`) is
stripped from the path.

- **Cap per file**: 64 KB. Larger files are truncated with a `(truncated)` marker.
- **Cap per message**: 8 files.
- **Sandbox**: when MCP filesystem roots are configured (`pengine fs add …`),
  mentions are restricted to those roots; otherwise `cwd`-relative paths are
  unrestricted.
- Errors (missing file, outside roots) are reported inline at the bottom of the
  reply and never abort the turn.

## Plan mode

Toggle with `/plan` (or `pengine plan on|off`). When on, the agent is given a
planning system prompt and write-style tools are stripped from the catalog
(name substring match: `write`, `edit`, `append`, `create`, `delete`, `patch`,
`update`, `save`, `set_`/`_set`, `rename`, `move`, `upsert`, `insert`,
`put`, `post`).

```
❯ /plan on
  ⎿  plan mode: ON
       · agent will produce a markdown plan
       · write tools (memory writes, fs writes, edits) are stripped from the catalog
❯ migrate the user table to add a `created_at` column
  ⎿  ## Plan
     1. Add `created_at TIMESTAMPTZ DEFAULT now()` to `users` …
     2. Backfill existing rows in batches …
```

Plan mode is process-local state on `AppState.plan_mode`; it resets when the
process exits.

## Adding MCP servers

Pengine speaks the same MCP wire protocol as Claude Code, so any server you
can run in Claude can run in Pengine. Three install paths:

### 1. Docker image (recommended)

Wraps the server in a container the same way pengine's built-in Tool Engine
catalog does. Requires podman or docker on the host.

```bash
pengine mcp add github \
  --image ghcr.io/example/github-mcp:latest \
  --mount-workspace \
  --append-roots
```

Flags:

- `--mount-workspace` — bind-mount every MCP filesystem root into the container
- `--mount-rw` — make those mounts read-write (default is read-only)
- `--append-roots` — append the container-side mount paths as argv after the image
- `--cmd <arg>` — extra argv after the image (repeatable; for images whose ENTRYPOINT is not the MCP server)
- `--direct-return` — send tool output straight to the user, no model summarisation

### 2. HTTP (Claude Code's `"type": "http"`)

For remote MCP servers (Anthropic-hosted, GitHub Copilot's, etc.):

```bash
pengine mcp add gh \
  --url https://api.example.com/mcp/ \
  --header "Authorization: Bearer $GITHUB_TOKEN"
```

Headers can be repeated. `Key: value` and `Key=value` are both accepted.
Pengine accepts `application/json` and `text/event-stream` responses.

### 3. Plain stdio

For Node `npx` servers when you don't want the Docker wrap (faster iteration,
no container runtime needed):

```bash
pengine mcp add fs \
  --command npx \
  --arg -y --arg @modelcontextprotocol/server-filesystem --arg "$PWD" \
  --env DEBUG=1
```

### List / remove

```bash
pengine mcp                      # alias for `pengine mcp list`
pengine mcp list
pengine mcp remove fs            # remove from mcp.json (and custom_tools, if applicable)
```

## Importing a Claude Code config

If you already have a `~/.claude.json` (or any `mcpServers`-shaped file),
import its servers into pengine's global `mcp.json`:

```bash
pengine mcp import ~/.claude.json
```

Servers with the same name are overwritten; new ones are added.

## Project-local `.mcp.json`

Drop a Claude Code-style `.mcp.json` at the root of any project:

```json
{
  "mcpServers": {
    "fs":  { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "."] },
    "gh":  { "type": "http", "url": "https://api.example.com/mcp/", "headers": { "Authorization": "Bearer …" } }
  }
}
```

When pengine starts in that directory, the file is loaded **on top of** the
global `mcp.json` for the lifetime of the process — it is never written to
the global config. Project entries with the same key as a global entry win
for that session. A `loaded project .mcp.json (N server(s)) from …` line
appears in the audit log on every rebuild.

## Diagnostics (`pengine doctor`)

Probe each subsystem and print a checklist (`[ok]` / `[warn]` / `[fail]`):

```
pengine doctor
```

Checks: store writability, Ollama daemon reachability, model catalog, MCP
registry rebuild, keychain (when a bot is connected), and outbound HTTPS.
Exit code is non-zero if any check fails.

## Known gaps vs Claude Code

These are deliberate omissions for the current feature set — tracked for later but not implemented today:

- **Streaming tool-call result bodies inside a reply**: each tool call now shows as its own `  ⎿  · …` line, but the **full** result body is still collapsed. Claude Code shows expandable tool outputs; Pengine only shows the one-line summary (`name: N bytes`, `name error: …`). Surfacing full bodies would need `agent::run_turn` to forward content, not just `emit_log` notices.
- **Inline Telegram buttons** (rerun / rollback): explicitly deferred (`cli_plan.md` §11).
- **In-flight turn cancellation**: double Ctrl+C exits the REPL but does not currently abort an in-progress agent turn — Pengine's tool loop is not cooperatively cancellable yet. Press Ctrl+C twice to exit; the in-flight turn's tool calls run to completion in the background.

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
