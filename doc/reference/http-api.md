# Loopback HTTP API reference

**Base:** `http://127.0.0.1:21516` (see `DEFAULT_PORT` in `src-tauri/src/infrastructure/http_server.rs` and `PENGINE_API_BASE` in `src/shared/api/config.ts`).

**CORS:** Permissive (`Any` origin/methods/headers) for local browser dev.

Authoritative list: **`Router::new()`** in `http_server.rs`. Below: method, path, and typical JSON shapes. Errors often use `{ "error": "…" }` with 4xx/5xx.

## Core

| Method | Path | Notes |
| --- | --- | --- |
| POST | `/v1/connect` | Body: `{ "bot_token": "…" }`. Success: `{ "status", "bot_id", "bot_username" }`. |
| DELETE | `/v1/connect` | Disconnect bot; clears in-memory connection. |
| GET | `/v1/health` | `{ "status", "bot_connected", "bot_username?", "bot_id?" }`. |
| GET | `/v1/logs` | **SSE** stream of `LogEntry` JSON (`timestamp`, `kind`, `message`). |

## Ollama

| Method | Path | Notes |
| --- | --- | --- |
| GET | `/v1/ollama/models` | Reachability, `active_model`, `selected_model`, `models[]`. |
| PUT | `/v1/ollama/model` | Body: `{ "model": "<name>" \| null }` — `null` clears preference (use loaded model). |

## User settings

| Method | Path | Notes |
| --- | --- | --- |
| GET | `/v1/settings` | `skills_hint_max_bytes` + min/max/default. |
| PUT | `/v1/settings` | Body: `{ "skills_hint_max_bytes": <u32> }` — clamped; persists `user_settings.json`. |

## CLI (terminal surface)

| Method | Path | Notes |
| --- | --- | --- |
| GET | `/v1/cli/commands` | JSON: `{ "commands": [ { "name", "summary" }, … ] }` — native command metadata (same registry as `pengine help`). |

## MCP

| Method | Path | Notes |
| --- | --- | --- |
| GET | `/v1/mcp/tools` | Flat list of tools: `server`, `name`, `description?`. |
| GET | `/v1/mcp/config` | `config_path`, `source`, `filesystem_allowed_paths`. |
| PUT | `/v1/mcp/filesystem` | Body: `{ "paths": ["…"] }` — workspace roots; triggers MCP sync/reconnect as implemented. |
| GET | `/v1/mcp/servers` | Server entries snapshot (shape matches internal DTO). |
| PUT | `/v1/mcp/servers/{name}` | Upsert one server; body is server config JSON. |
| DELETE | `/v1/mcp/servers/{name}` | Remove server by MCP config key. |

## Tool Engine

| Method | Path | Notes |
| --- | --- | --- |
| GET | `/v1/toolengine/runtime` | Docker/Podman availability for UI wizard. |
| GET | `/v1/toolengine/catalog` | Curated tool list from registry fetch / embed. |
| GET | `/v1/toolengine/installed` | Installed catalog tools / keys. |
| POST | `/v1/toolengine/install` | Install by catalog id (body per handler struct in `http_server.rs`). |
| POST | `/v1/toolengine/uninstall` | Remove catalog tool. |
| PUT | `/v1/toolengine/private-folder` | Host path for tool private data. |
| PUT | `/v1/toolengine/passthrough-env` | MCP env passthrough (secrets path). |
| GET | `/v1/toolengine/custom` | List custom Docker MCP tools. |
| POST | `/v1/toolengine/custom` | Add custom image tool (see [guides/custom-mcp-tools.md](../guides/custom-mcp-tools.md)). |
| DELETE | `/v1/toolengine/custom/{key}` | Remove custom tool. |

## Skills

| Method | Path | Notes |
| --- | --- | --- |
| GET | `/v1/skills` | List bundled + custom skills, enabled flags, `custom_dir`, bodies; optional JSON field `mandatoryMarkdown` when `mandatory.md` exists. |
| POST | `/v1/skills` | Create/update custom skill: JSON `slug`, `markdown` (full `SKILL.md`), optional `mandatory_markdown` (omit to leave `mandatory.md` unchanged; empty string removes the file). |
| DELETE | `/v1/skills/{slug}` | Delete custom skill. |
| PUT | `/v1/skills/{slug}/enabled` | Toggle enabled (`.disabled.json`). |
| GET | `/v1/skills/clawhub/plugins` | ClawHub plugin listing (paginated / search params as defined in handlers). |
| GET | `/v1/skills/clawhub` | Search ClawHub. |
| POST | `/v1/skills/clawhub/install` | Install skill from ClawHub into custom dir. |

For exact request/response types, grep **`handle_`** in `http_server.rs` or read the `Deserialize`/`Serialize` structs at the top of that file.
