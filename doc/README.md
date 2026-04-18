# Pengine documentation

Short reference for developers and AI agents. Content is grouped **by topic** in subfolders.

## Folder layout

```text
doc/
├── README.md                 # This index + feature map
├── architecture/             # Codebase layout, DDD, MCP module design
├── agent/                    # Telegram → Ollama → tools behavior
├── platform/                 # Startup, AppState, files, secrets
├── reference/                # Machine-oriented API tables
├── guides/                   # How-to: skills, custom MCP tools
└── tool-engine/              # Maintainers: image publish, registry
```

Product overview: [../README.md](../README.md).

**Coding assistants:** Repo root **[../AGENTS.md](../AGENTS.md)** points here and lists a **task → doc → source file** reading order. Use it so changes respect DDD boundaries (`architecture/README.md` before moving code).

---

## Topics

### Architecture and codebase

| Doc | Purpose |
| --- | --- |
| [architecture/README.md](architecture/README.md) | Repo layout, DDD boundaries, frontend/backend rules, tooling commands |
| [architecture/mcp.md](architecture/mcp.md) | MCP host/client flow, Ollama tool bridge, audit logging |

### Agent and runtime

| Doc | Purpose |
| --- | --- |
| [agent/runtime.md](agent/runtime.md) | `run_turn`, tool routing, system prompt, Ollama loop, limits |

### Platform (data and startup)

| Doc | Purpose |
| --- | --- |
| [platform/data-and-startup.md](platform/data-and-startup.md) | `AppState`, on-disk files, keychain, `app.rs` boot order |

### Reference

| Doc | Purpose |
| --- | --- |
| [reference/http-api.md](reference/http-api.md) | Loopback REST + SSE endpoints (method/path tables) |

### Guides (how-to)

| Doc | Purpose |
| --- | --- |
| [guides/skills.md](guides/skills.md) | Skills vs MCP, Dashboard behavior, `SKILL.md` / `mandatory.md`, context cap, troubleshooting |
| [guides/custom-mcp-tools.md](guides/custom-mcp-tools.md) | Concepts, dashboard vs API, `mcp.json` paths, stdio fields, Docker/custom tools, pitfalls |

### Tool Engine (maintainers)

| Doc | Purpose |
| --- | --- |
| [tool-engine/manual-publish.md](tool-engine/manual-publish.md) | GHCR images, `mcp-tools.json`, publish workflow |

---

## Feature map (code anchors)

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

---

## HTTP API (quick list)

Base URL: `http://127.0.0.1:21516` (see `src/shared/api/config.ts`). Details: **[reference/http-api.md](reference/http-api.md)**.

- **Core:** `POST/DELETE /v1/connect`, `GET /v1/health`, `GET /v1/logs` (SSE)
- **Ollama:** `GET /v1/ollama/models`, `PUT /v1/ollama/model`
- **Settings:** `GET/PUT /v1/settings` (e.g. skills hint byte cap)
- **MCP:** `GET /v1/mcp/tools`, `GET /v1/mcp/config`, `PUT /v1/mcp/filesystem`, `GET/PUT/DELETE /v1/mcp/servers/...`
- **Tool Engine:** `GET /v1/toolengine/runtime`, `catalog`, `installed`, `POST install/uninstall`, `PUT private-folder`, `PUT passthrough-env`, `GET/POST/DELETE /v1/toolengine/custom/...`
- **Skills:** `GET/POST /v1/skills`, `DELETE /v1/skills/{slug}`, `PUT /v1/skills/{slug}/enabled`, ClawHub routes under `/v1/skills/clawhub/...`

Authoritative route list: `http_server.rs` in `src-tauri/src/infrastructure/http_server.rs`.

---

## Config and data paths

- **Bot connection:** `connection.json` under app data (next to `mcp.json`); see `src-tauri/src/modules/bot/repository.rs` and [platform/data-and-startup.md](platform/data-and-startup.md).
- **MCP:** `mcp.json` — [guides/custom-mcp-tools.md](guides/custom-mcp-tools.md) and `PENGINE_MCP_CONFIG`.
- **Skills (custom):** `$APP_DATA/skills/` — [guides/skills.md](guides/skills.md).
