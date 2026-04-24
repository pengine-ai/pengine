# Instructions for AI coding assistants

Use **[doc/README.md](doc/README.md)** as the main documentation index (topics, feature map, API overview).

## Before you change structure

Read **[doc/architecture/README.md](doc/architecture/README.md)** before adding, moving, or renaming files under `src/` or `src-tauri/src/`. It defines module boundaries and **forbidden cross-imports** between frontend layers.

## What to read for a given change

| Task | Documentation | Then open in code (non-exhaustive) |
| --- | --- | --- |
| Telegram flow, agent loop, tools, prompts, limits | [doc/agent/runtime.md](doc/agent/runtime.md) | `src-tauri/src/modules/agent/`, `modules/bot/service.rs` |
| New or changed HTTP routes / dashboard API | [doc/reference/http-api.md](doc/reference/http-api.md) | `src-tauri/src/infrastructure/http_server.rs`, matching `src/modules/*/api/` |
| MCP client, registry, native tools, `mcp.json` | [doc/architecture/mcp.md](doc/architecture/mcp.md), [doc/guides/custom-mcp-tools.md](doc/guides/custom-mcp-tools.md) | `src-tauri/src/modules/mcp/` |
| Startup, `AppState`, disk paths, secrets | [doc/platform/data-and-startup.md](doc/platform/data-and-startup.md) | `src-tauri/src/app.rs`, `shared/state.rs`, `modules/secure_store/` |
| Skills format and injection | [doc/guides/skills.md](doc/guides/skills.md) | `src-tauri/src/modules/skills/service.rs` |
| Tool Engine / catalog / container tools | [doc/tool-engine/manual-publish.md](doc/tool-engine/manual-publish.md) | `src-tauri/src/modules/tool_engine/` |
| Memory sessions / diary keywords | `memory` section in [doc/agent/runtime.md](doc/agent/runtime.md) | `src-tauri/src/modules/memory/` |

## Limits of the docs

These files are **summaries** for navigation and context. For exact behavior (error shapes, edge cases, field names), **read the referenced Rust or TypeScript sources** and the `Router::new()` / handler definitions in `http_server.rs`.

## Product-level context

High-level product and dev commands: [README.md](README.md).
