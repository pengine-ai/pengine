# MCP — Model Context Protocol Module (POC)

> Status: **proof of concept**. One server, one transport, one happy path.

## What & Why

[MCP](https://modelcontextprotocol.org/) is an open JSON-RPC 2.0 protocol that lets an LLM "host" discover and call tools exposed by external "servers". Pengine adopts MCP so we can grow the agent's capabilities by dropping in new servers instead of writing bespoke Rust glue for each tool. Every tool call flows through one well-defined choke point, which is what makes it auditable.

## Roles in Pengine

| Role | Where | Responsibility |
|---|---|---|
| **Host** | Pengine (Tauri binary) | Owns the LLM (Ollama) connection, the Telegram bot, and the agent loop. |
| **Client** | `src-tauri/src/modules/mcp/` | One `McpClient` per connected server. Speaks JSON-RPC over stdio. |
| **Server** | External child process | Anything that speaks MCP — `npx @modelcontextprotocol/server-filesystem`, a Docker container, a custom binary. |

```text
Telegram message
      │
      ▼
bot::service::text_handler
      │
      ▼
bot::agent::run_turn ────► ollama::chat_with_tools (Ollama /api/chat)
      ▲                            │
      │                            │ tool_calls?
      │                            ▼
      └─────────── mcp::registry::call_tool ──► McpClient ──► child process (stdio)
```

## Module Layout

```text
src-tauri/src/modules/mcp/
├── mod.rs
├── protocol.rs   JSON-RPC 2.0 request/response types
├── types.rs      McpConfig, ServerConfig, Tool
├── transport.rs  StdioTransport — child process + line-delimited JSON
├── client.rs     McpClient — initialize / tools/list / tools/call
├── registry.rs   McpRegistry — fan-out across all connected servers
└── service.rs    load_or_init_config(), connect_all()
```

The registry lives on `AppState.mcp` (`Arc<RwLock<McpRegistry>>`) so the bot agent and any future HTTP route can reach it.

## Config

File: `$APP_DATA/mcp.json` (next to `connection.json`). Created on first launch with a sane default if missing.

**User-facing guide (stdio packages, Docker images, API):** [custom-mcp-tools.md](../custom-mcp-tools.md).

```json
{
  "servers": {
    "filesystem": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"],
      "env": {},
      "direct_return": false
    }
  }
}
```

To add a server: add another entry under `servers`. Restart Pengine.

## Protocol Subset Implemented

Four messages, that's it:

1. `initialize` — handshake. We send `{protocolVersion, capabilities, clientInfo}` and ignore most of the response.
2. `notifications/initialized` — required notification after init.
3. `tools/list` — discovery, cached on the client.
4. `tools/call` — `{name, arguments}` → `{content: [{type: "text", text}]}`.

Out of scope for the POC: resources, prompts, sampling, server-initiated requests, batch JSON-RPC, HTTP transport.

## Ollama Bridge

MCP `inputSchema` is JSON Schema, and so are Ollama's tool `parameters` — translation is just a rename. See `to_ollama_tools` in `bot/agent.rs`. Tool names are emitted as `server.tool` so the registry can route a call back to the right client.

The agent loop in `bot::agent::run_turn`:

1. Snapshot the available tools from the registry.
2. Send `system + user` plus the tool list to Ollama.
3. If the response carries `tool_calls`, run each via `registry.call_tool`, append the results as `role: "tool"` messages, loop. Capped at **5 steps**.
4. Otherwise return the assistant content as the final reply.

Use a tool-capable model (e.g. `qwen3:8b`). Check with `ollama show <model>` for the `tools` capability.

## Audit Logs

Every MCP-relevant event is emitted as a `LogEntry` with `kind = "mcp"` via `state.emit_log`. They flow through the existing SSE log stream (`GET /v1/logs`) and are visible on the dashboard:

- `loading MCP config…`
- `filesystem ready (2 tools)`
- `MCP ready (2 tools)`
- `tools available: filesystem.read_file, filesystem.list_directory`
- `tool call (0): filesystem.list_directory({"path":"/tmp"})`
- `tool result (842 bytes)`
- `tool error: …`

That single audit trail is the "auditable protocol" promise of this feature.

## Try It

1. `npx -y @modelcontextprotocol/server-filesystem /tmp` should run (Node + npm available).
2. `ollama pull qwen3:8b` (or any tool-capable model).
3. `bun run tauri dev`. On first launch, watch the dashboard for `mcp` lines confirming the filesystem server connected.
4. Connect a Telegram bot, then send: *"List the files in /tmp."*
5. Expect a `tool call` and `tool result` line in the log, followed by a coherent reply on Telegram.

## Future Work

Permission prompts, multiple servers in the default config, a frontend tools panel, hot reload of `mcp.json`, HTTP/SSE transport, resources & prompts. Not in this PR.
