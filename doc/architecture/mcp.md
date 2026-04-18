# MCP — Model Context Protocol in Pengine

Pengine acts as an **MCP host**: it runs one JSON-RPC **client** per configured server, merges tool lists for Ollama, and routes `tools/call` by `server.tool` name.

## Roles

| Role | Where | Responsibility |
| --- | --- | --- |
| **Host** | Tauri binary | Telegram bot, loopback HTTP API, agent loop, Ollama |
| **Client** | `src-tauri/src/modules/mcp/` | `McpClient` per server — stdio JSON-RPC |
| **Server** | Child process or native provider | MCP over stdio, or built-in native tools |

```text
Telegram -> teloxide dispatcher -> bot::service::text_handler -> bot::agent::run_turn
  run_turn calls ollama::chat_with_tools using tool definitions from mcp::registry
  each model tool_call -> mcp::registry::call_tool -> McpClient (stdio) or native provider
```

## Module layout

```text
src-tauri/src/modules/mcp/
├── mod.rs
├── protocol.rs   # JSON-RPC types
├── types.rs      # McpConfig, ServerConfig, ToolDef, …
├── transport.rs  # StdioTransport
├── client.rs     # initialize / tools/list / tools/call
├── registry.rs   # ToolRegistry — fan-out + routing
├── native.rs     # Built-in servers (e.g. dice, tool_manager)
├── tool_metadata.rs
└── service.rs    # load/reload mcp.json, connect_all, workspace sync
```

Registry state lives on `AppState` (see `shared/state.rs`) so HTTP handlers and the agent share the same view.

## Config

**File:** `mcp.json` next to `connection.json` (app data), unless **`PENGINE_MCP_CONFIG`** overrides. **`GET /v1/mcp/config`** returns the resolved path and metadata.

**User guide:** [guides/custom-mcp-tools.md](../guides/custom-mcp-tools.md) (stdio `npx`, hand-written Docker argv, Tool Engine catalog, **`POST /v1/toolengine/custom`**, native entries).

**Multiple servers** are supported: each key under `servers` becomes a client; tool names exposed to the model are `server_key.tool_name`.

Secrets (bot tokens, MCP passthrough env) are stored via **`secure_store`**, not inline in `mcp.json`, when configured.

## Protocol subset (stdio clients)

Implemented messages:

1. `initialize` / `notifications/initialized`
2. `tools/list` (cached on the client)
3. `tools/call`

Not implemented here: resources, prompts, sampling, server-initiated requests, HTTP/SSE MCP transport.

## Ollama bridge

MCP `inputSchema` maps to Ollama tool `parameters`. **`to_ollama_tools`** (in `bot/agent.rs` and MCP helpers) rewrites names to `server.tool` so **`registry.call_tool`** can dispatch.

**Agent loop** (`run_turn`):

1. Snapshot tools from the registry (+ native providers).
2. Send system + user + tool list to Ollama.
3. On `tool_calls`, execute via the registry, append `role: "tool"` messages, repeat until done or **step cap** (see `MAX_STEPS` in `agent.rs`).
4. Otherwise return assistant text to Telegram.

Use a **tool-capable** model (`ollama show <model>` — `tools` capability).

## Dashboard and HTTP API

The **Tools** / **Tool Engine** UI (`src/modules/mcp/`, `src/modules/toolengine/`) calls the loopback API to list tools, edit `mcp.json` entries, install catalog images, and manage workspace roots. Saving config triggers **MCP reconnect / reload** in the backend (`mcp::service`).

## Audit logs

MCP events emit `LogEntry` with `kind = "mcp"` to **`GET /v1/logs`** (SSE) and the in-app log view.

Examples: config load, server ready, `tools/list` sizes, each `tools/call` and result size, errors.

## Try it

1. Node on `PATH` if using `npx` stdio servers.
2. `ollama pull` a tool-capable model.
3. `bun run tauri dev` — dashboard should show MCP status lines.
4. Connect a bot; ask something that triggers a configured tool; confirm log lines and Telegram reply.

## Future work

Finer-grained permission prompts, optional HTTP/SSE MCP transport, resources and prompts in the client, richer multi-server defaults for first-run.
