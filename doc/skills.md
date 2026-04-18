# Skills

Skills are lightweight context templates the agent can read before making a request. They are **secondary** to MCP tools — MCP servers run in containers and offer real capabilities, while skills are just markdown that tells the agent *how to call a public endpoint and what the response will look like*.

Think **OpenAPI-lite in a SKILL.md**.

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

On macOS the custom dir resolves to `~/Library/Application Support/pengine/skills/`. The exact path is shown in the Dashboard panel.

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
2. Click **Add custom skill**.
3. Provide a slug and the full skill markdown. The app writes it to `$APP_DATA/skills/<slug>/SKILL.md`.

### By hand

Drop a folder into `$APP_DATA/skills/` with a `SKILL.md` inside. The Dashboard picks it up on reload. (Legacy folders with only `README.md` are ignored — rename to `SKILL.md`.)

### From ClawHub

1. Click **Browse ClawHub** in the Skills panel.
2. Pick a skill and hit **Install**. The app fetches `SKILL.md` from ClawHub and writes it to your custom dir.

> ClawHub is the community registry. Skills there are plain markdown with the same frontmatter shape as local ones — no special magic.

---

## Editing a skill

Open the file at `$APP_DATA/skills/<slug>/SKILL.md` in any editor. Changes are picked up on the next dashboard refresh. There is no compile step.

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
