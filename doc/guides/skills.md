# Skills

This guide covers **Pengine skills**: folders under the app data directory (plus bundled examples) whose main file is **`SKILL.md`**. They are merged into the **system** prompt so the model sees recipes, curl patterns, and response shapes **before** it chooses tools.

They are **not** a plugin runtime. For executable capabilities (filesystem, databases, proprietary CLIs), use **MCP tools** instead — see [Adding custom MCP tools](./custom-mcp-tools.md).

---

## Mental model

| | Skill | MCP tool |
|---|--------|----------|
| **What the model gets** | Markdown + YAML frontmatter in context | JSON-RPC tool definitions + live server |
| **Who executes work** | Model / runtime follows the text (e.g. fetch URL) | MCP server process or container |
| **Latency / cost** | Only token cost of the text you include | Container startup, image pulls, host resources |
| **Typical use** | Public HTTP APIs, “how to call X”, response field cheat sheets | Stateful tools, sandboxes, anything that is not “one markdown page” |

Skills are lightweight context templates the agent can read before making a request. They are **secondary** to MCP tools — MCP servers run in containers and offer real capabilities, while skills are just markdown that tells the agent *how to call a public endpoint and what the response will look like*.

Think **OpenAPI-lite in a SKILL.md**.

---

## In the Dashboard (Skills panel)

From **Dashboard → Skills** you can:

- **Toggle** each skill on or off (disabled skills are omitted from the merged hint; see `.disabled.json` behavior in your install if documented in code comments).
- **Adjust “Context”** — a slider for **`skills_hint_max_bytes`**: the maximum UTF-8 size of the combined skills block injected into the system prompt. Lower = less recipe text, higher = more before truncation. Limits and default come from **`GET /v1/settings`** (with local fallbacks if the API is offline).
- **Add custom skill** — paste a full `SKILL.md` (including frontmatter); the app writes under `$APP_DATA/skills/<slug>/`.
- **Browse ClawHub** — install community **skills** from the registry (plain markdown). The **Plugins** tab lists ClawHub plugins for discovery; installing those still happens on ClawHub’s side, not inside Pengine.

The panel shows **Custom dir** — the resolved path to `$APP_DATA/skills/` on your machine (handy when editing files in an external editor).

---

## Why skills exist

Small local models can't interpret complex tools quickly. Skills solve this by giving the model a short, pre-shaped template instead of a full tool definition:

- A concrete `curl` / `fetch` example it can copy.
- An expected response schema so it knows which fields to extract.
- A one-line "when to use" hint.

They stay cheap on tokens because the content is hand-written for the task, not auto-generated.

---

## Folder layout

```text
tools/skills/              # bundled with the app (read-only examples)
  weather/
    SKILL.md
    mandatory.md           # optional; extra rules appended to the agent hint (not shown in UI)

$APP_DATA/skills/          # user-editable; ClawHub installs land here too
  <slug>/
    SKILL.md
    mandatory.md           # optional
```

One folder per skill. One `SKILL.md` per folder. That's it.

- **Bundled** skills ship with the app and can't be edited in-place. Copy them to your custom dir to tweak.
- **Custom** skills are yours. Edit freely.

On macOS the custom dir typically resolves under **`~/Library/Application Support/pengine/`** (see [platform/data-and-startup.md](../platform/data-and-startup.md) for the general layout). The exact **`skills/`** path is shown in the Dashboard panel.

### Optional `mandatory.md`

Beside `SKILL.md`, a folder may contain **`mandatory.md`**. When present, its text is appended to the agent hint for that skill (extra rules or constraints). It is **not** shown as a separate row in the Skills UI — think of it as “always-on fine print” for that skill’s turn.

---

## Skill file format

Each `SKILL.md` starts with a YAML frontmatter block:

```markdown
---
name: weather
description: Get current weather and forecasts — no API key required.
version: 1.0.0
author: Your Name
source: https://clawhub.ai/you/weather
license: MIT-0
tags: [weather, forecast]
requires: [curl]
---

# Weather

<free-form markdown body…>
```

### Required fields

| field | purpose |
|---|---|
| `name` | Short slug the agent uses to refer to the skill. |
| `description` | One-line summary shown in the UI and prepended to the model prompt. |

### Optional fields

`version`, `author`, `source` (ClawHub may use `homepage` — treated like `source`), `license`, `tags` (string[]), `requires` (string[] — host binaries or tool names such as `curl`; also used to gate **`brave_web_search`** for this turn), `brave_allow_substrings` (string[] — extra user-message phrases that may enable web search for this skill).

### Body convention

Write the body for a reader who will execute the request by hand. The agent treats it the same way — it reads the body as context, extracts the request pattern, and runs it.

A good skill body has three sections:

1. **Request** — the exact `curl` (or `fetch`) line, with query params explained in a table.
2. **Response schema** — a trimmed JSON example + a field-level cheatsheet.
3. **When to use** — 1–3 bullets about which question this skill answers.

See `tools/skills/weather/SKILL.md` for a worked example.

---

## Adding a skill

### From the Dashboard

1. Open the **Skills** panel on the Dashboard.
2. Click **Add custom skill** (or **Edit** on a custom skill).
3. Provide a slug and the full skill markdown for **SKILL.md**. Optionally fill **mandatory.md** in the second editor; leave it empty to omit that file. Saving with an empty **mandatory.md** field removes an existing `mandatory.md`. The app writes under `$APP_DATA/skills/<slug>/`.

### By hand

Drop a folder into `$APP_DATA/skills/` with a `SKILL.md` inside. The Dashboard picks it up on reload. (Legacy folders with only `README.md` are ignored — rename to `SKILL.md`.)

### From ClawHub

1. Click **Browse ClawHub** in the Skills panel.
2. Pick a skill and hit **Install**. The app fetches `SKILL.md` from ClawHub and writes it to your custom dir.

> ClawHub is the community registry. Skills there are plain markdown with the same frontmatter shape as local ones — no special magic.

---

## Editing a skill

Open the file at `$APP_DATA/skills/<slug>/SKILL.md` in any editor, or use **Edit** in the Dashboard to change both **SKILL.md** and optional **mandatory.md** in one save. Changes from disk are picked up on the next dashboard refresh. There is no compile step.

Deleting a custom skill from the Dashboard removes the entire skill folder, including **mandatory.md** if present.

To tweak a **bundled** skill, click **Fork to custom** in the panel (or copy the folder manually). Edits to `tools/skills/` inside the app bundle will not persist across reinstalls.

---

## Sharing a skill

1. Put the skill folder in a public repo (or submit it to ClawHub).
2. Set `source:` in the frontmatter to the canonical URL.
3. Others can install via Dashboard → Browse ClawHub, or clone the repo into their `$APP_DATA/skills/`.

---

## Skills vs MCP tools — when to use which

| | Skill | MCP tool |
|---|---|---|
| Runtime | Agent executes a `fetch`/`curl` inline | Separate MCP server (often containerised) |
| Cost | Free — just text context | Container memory + startup time |
| Best for | Read-only public APIs, templated fetches | Stateful tools, filesystem, long-running workers |
| Editability | Edit a markdown file | Rebuild container image |

Reach for a skill first if the task is "call this URL, return this JSON". Reach for an MCP tool if the agent needs to write to disk, keep state, or call something that doesn't fit in one HTTP request.

---

## Wiring into the agent

**Enabled** skills are merged into the **system** prompt on every turn: for each skill, the runtime adds `description`, a truncated `SKILL.md` body, and optional `mandatory.md` text (see `skills_prompt_hint` in `src-tauri/src/modules/skills/service.rs`). Disabled skills (Dashboard toggle → `.disabled.json`) are omitted.

The **user message** does *not* pick which skill bodies load; it is used for other gates (for example whether **`brave_web_search`** is exposed this turn — keywords module + skill `requires` / `brave_allow_substrings` / long `tags`).

The total skills fragment is capped by **`skills_hint_max_bytes`** (`GET/PUT /v1/settings`; dashboard slider).

---

## Troubleshooting

| Symptom | Things to check |
|--------|------------------|
| Skill does not appear | Folder name vs slug, `SKILL.md` present (not only `README.md`), refresh the dashboard. |
| Skill appears but model “ignores” it | It may be truncated — raise **Context** slightly or shorten the body; very long bodies are cut to fit `skills_hint_max_bytes`. |
| Toggle has no effect | Ensure the UI saved; disabled slugs live in **`skills/.disabled.json`** next to your custom skills (see [data-and-startup.md](../platform/data-and-startup.md)). Reload the dashboard after hand-editing files. |
| ClawHub install failed | Network, ClawHub availability, or disk permissions under `$APP_DATA/skills/`. |
| `brave_web_search` not offered | Gating uses the user message plus skill metadata (`requires`, `brave_allow_substrings`, long `tags`). See runtime / keywords modules for the exact policy in your build. |

---

## See also

- [Adding custom MCP tools](./custom-mcp-tools.md) — when a skill is not enough and you need a real MCP server.
- [architecture/mcp.md](../architecture/mcp.md) — how MCP integrates with the host.
- [platform/data-and-startup.md](../platform/data-and-startup.md) — where app data lives on disk.
