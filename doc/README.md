# Pengine documentation

Short reference for developers and AI agents. Deep dives live in the linked files.

## Where to look

| Doc | Purpose |
| --- | --- |
| [design/README.md](design/README.md) | Repo layout, DDD boundaries, frontend/backend rules, tooling commands |
| [design/mcp.md](design/mcp.md) | MCP host/client flow, Ollama tool bridge, audit logging |
| [custom-mcp-tools.md](custom-mcp-tools.md) | `mcp.json`, stdio/native servers, Docker/custom tools, HTTP API snippets |
| [skills.md](skills.md) | Skill format, ClawHub, bundled vs custom dirs |
| [tool-engine/manual-publish.md](tool-engine/manual-publish.md) | GHCR images, `mcp-tools.json`, maintainer publish flow |

Project overview and user-facing product notes: [../README.md](../README.md).

## Feature map (code anchors)

Use this table to jump to the right area when changing behavior.

| Feature | What it does | Primary locations |
| --- | --- | --- |
| **Web UI** | Landing, setup wizard, dashboard | `src/pages/`, `src/App.tsx` |
| **Loopback HTTP API** | REST + SSE on `127.0.0.1:21516` | `src-tauri/src/infrastructure/http_server.rs` |
| **Telegram bot** | Token verify, dispatch, replies | `src-tauri/src/modules/bot/` |
| **Agent loop** | Ollama chat + tools, step cap, policies | `src-tauri/src/modules/bot/agent.rs` |
| **Ollama** | Models list, active/selected model | `src-tauri/src/modules/ollama/`, `GET/PUT /v1/ollama/*` |
| **MCP** | stdio transports, registry, `tools/call` | `src-tauri/src/modules/mcp/` |
| **Tool Engine** | Catalog install, custom images, runtime probe | `src-tauri/src/modules/tool_engine/`, `src/modules/toolengine/` |
| **Skills** | `SKILL.md` recipes, ClawHub, prompt hints | `src-tauri/src/modules/skills/`, `src/modules/skills/`, `tools/skills/` |
| **Memory** | Session/diary keywords → MCP memory tools | `src-tauri/src/modules/memory/` |
| **Secrets** | Keychain/OS store for tokens + MCP env | `src-tauri/src/modules/secure_store/` |
| **Keywords** | Shared phrase matching (search, memory, etc.) | `src-tauri/src/modules/keywords/`, `src-tauri/src/shared/keywords.rs` |
| **Dashboard** | Status, Ollama model, MCP tools, Tool Engine, Skills | `src/pages/DashboardPage.tsx` |
| **E2E** | Playwright setup path | `e2e/` |

## HTTP API (quick list)

Base URL: `http://127.0.0.1:21516` (see `src/shared/api/config.ts`).

- **Core:** `POST/DELETE /v1/connect`, `GET /v1/health`, `GET /v1/logs` (SSE)
- **Ollama:** `GET /v1/ollama/models`, `PUT /v1/ollama/model`
- **Settings:** `GET/PUT /v1/settings` (e.g. skills hint byte cap)
- **MCP:** `GET /v1/mcp/tools`, `GET /v1/mcp/config`, `PUT /v1/mcp/filesystem`, `GET/PUT/DELETE /v1/mcp/servers/...`
- **Tool Engine:** `GET /v1/toolengine/runtime`, `catalog`, `installed`, `POST install/uninstall`, `PUT private-folder`, `PUT passthrough-env`, `GET/POST/DELETE /v1/toolengine/custom/...`
- **Skills:** `GET/POST /v1/skills`, `DELETE /v1/skills/{slug}`, `PUT /v1/skills/{slug}/enabled`, ClawHub routes under `/v1/skills/clawhub/...`

Authoritative route list: `http_server.rs` router in `src-tauri/src/infrastructure/http_server.rs`.

## Config and data paths

- **Bot connection:** `connection.json` under app data (next to `mcp.json`); path logic in `src-tauri/src/modules/bot/repository.rs`
- **MCP:** `mcp.json` — see [custom-mcp-tools.md](custom-mcp-tools.md) and `PENGINE_MCP_CONFIG`
- **Skills (custom):** `$APP_DATA/skills/` — see [skills.md](skills.md)
