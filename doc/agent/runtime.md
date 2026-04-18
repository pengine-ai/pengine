# Agent runtime (Telegram → Ollama → tools)

Technical reference for one **user message** through the stack. Primary implementation: `src-tauri/src/modules/bot/agent.rs`.

## Entry: `run_turn`

1. **Think override** — User prefix can force extended reasoning (`parse_think_override`); see `ollama::keywords::THINK_ON`.
2. **Memory commands** — If the text matches a **session/diary** keyword (`memory::detect_session_command`), the turn short-circuits to open/close recording or handle a diary line (no LLM). Phrase sets live in `src-tauri/src/modules/memory/mod.rs`.
3. **Diary mode** — If a diary session is active, user text is appended to memory and the turn returns **suppressed** (no Telegram reply).
4. **Normal path** — `run_model_turn` → optional **background** memory append for full chat sessions (`spawn_memory_save`).

## Model selection

- **`AppState.preferred_ollama_model`** (set via `PUT /v1/ollama/model`) wins.
- Otherwise **`ollama::active_model()`** (Ollama `/api/ps` — whatever is loaded).

Constants: `src-tauri/src/modules/ollama/constants.rs` (`OLLAMA_CHAT_URL`, etc.).

## Tool list for each step

**`ToolRegistry::select_tools_for_turn`** (`src-tauri/src/modules/mcp/registry.rs`) builds a **subset** of MCP tools for most turns:

- **Routing:** keyword scoring + **recent tool names** (`AppState.recent_tool_names`, FIFO cap 32) + always-on tools + **memory** tools when a memory server is present.
- **Full chat session recording:** When a non-diary memory session is active, memory tools stay available every turn (`chat_session_recording`).
- **`brave_web_search`:** Stripped unless `allow_brave_web_search` is true — from global search keywords (`modules/keywords`) **or** enabled skill gates (`skills::allow_brave_web_search_for_message`).

Observability: `emit_log("tool_ctx", …)` includes `routing` (`full` | `ranked` | `core_no_signal`), subset vs full counts, `select_ms`, and `brave_web`.

### Escalation to full catalog

If **step 0** used a subset, **`routing_escalated`** is false, the model returns **no tool calls** and **empty content**, the host **replaces** `tool_ctx` with `full_tool_context` and **continues** the loop (same user message). This recovers from overly aggressive routing without burning a “real” step on silence.

## System prompt

`build_system_prompt` concatenates (only when `has_tools`):

1. **`PENGINE_OUTPUT_CONTRACT_LEAD`** and tool-use instructions (`shared/text.rs`).
2. **Filesystem hint** — `cached_filesystem_paths` → `/app/…` mount list (`tool_engine::workspace_app_bind_pairs`).
3. **Memory hint** — Active session name + diary flag.
4. **Skills fragment** — All **enabled** skills via `skills::skills_prompt_hint`, then **`limit_skills_hint_bytes`** using `AppState.skills_hint_max_bytes`.

**Order is stable** (system first, then user) on purpose: Ollama KV-cache reuse across turns (`run_model_turn` comment).

## Ollama chat loop

- **`MAX_STEPS`:** hard cap on loop iterations (`agent.rs`, currently **6**).
- **`chat_with_tools`:** Sends `messages` JSON array + `tools` array; options vary by step (`chat_options_for_agent_step`: post-tool reminder, `think`, JSON-only mode when no tools exist).
- If the model does not support tools, the host retries with an empty tool list (`tools_supported = false`).

### Tool execution

- **Parallel:** Each `tool_calls[]` entry is `prepare_tool_invocation` → `tokio::spawn` → `Provider::call_tool`.
- **Policies before spawn:**
  - At most **`MAX_BRAVE_WEB_SEARCH_PER_USER_MESSAGE`** successful `brave_web_search` calls per user message (extra calls get an error string as the tool result).
  - **`fetch`** URLs deduplicated per user message (`fetch_urls_success` / `FETCH_DUPLICATE_URL_MSG`).
- **File Manager paths:** Args may be normalized in the registry (`rewrite_file_manager_path`, relative → `/app/…`).

### After tools

Results are appended as **`role: "tool"`** messages; **`tool_rounds`** increments. **`direct_return`** tools can short-circuit user-visible replies (`search_followup` / Brave flows — see `agent.rs` for the full branch).

## Related state

| Field | Role |
| --- | --- |
| `memory_session` | In-memory recording session (`MemorySession`: entity name, turn count, diary flag) |
| `recent_tool_names` | Recent tool invocations for routing |
| `tool_ctx_latency_ms` | Rolling timings for subset selection |
| `skills_hint_max_bytes` | Runtime cap on skills system text |
| `cached_filesystem_paths` | Workspace roots for `/app` hints |

## Logging

`kind` examples: `run` (think line), `tool_ctx`, `time` (per model step), `tool` (calls/results), `memory`, `mcp` (from MCP service). All go through `AppState::emit_log` → SSE `GET /v1/logs`.
