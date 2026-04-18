# Data layout, secrets, and startup

Where state lives on disk and how the Tauri app boots. Wiring: `src-tauri/src/app.rs`, types: `src-tauri/src/shared/state.rs`, persistence: `modules/bot/repository.rs`, `modules/secure_store/`, `shared/user_settings.rs`, `modules/mcp/service.rs`.

## `AppState` (in memory)

Cloned into every HTTP handler and shared across async tasks. Notable fields:

| Field | Type / notes |
| --- | --- |
| `connection` | `Mutex<Option<ConnectionData>>` — **plaintext token only while running**; see below |
| `store_path` | Path to **`connection.json`** (file may be missing until first connect) |
| `mcp_config_path` | Resolved **`mcp.json`** path |
| `mcp_config_source` | `"project"` or `"app_data"` (exposed on `GET /v1/mcp/config`) |
| `mcp` | `RwLock<ToolRegistry>` — connected MCP/native providers |
| `mcp_rebuild_mutex` | Serializes full registry rebuilds (stdio connect storms) |
| `preferred_ollama_model` | User override from API / UI |
| `cached_filesystem_paths` | Workspace roots mirrored from MCP config |
| `memory_session` | Active memory recording session, if any |
| `skills_hint_max_bytes` | Loaded at startup; updated via `PUT /v1/settings` |
| `log_tx` | Broadcast channel for SSE logs (capacity 256) |

`ConnectionData` (RAM) holds `bot_token`, `bot_id`, `bot_username`, `connected_at`. **`Debug` redacts the token.**

## Files under app data

Paths are Tauri **`app_data_dir()`** (platform-specific). The app uses **`connection.json`** as the anchor path; sibling files sit in the **same directory**.

| File | Contents |
| --- | --- |
| **`connection.json`** | `ConnectionMetadata` only: `bot_id`, `bot_username`, `connected_at` — **no token** |
| **`user_settings.json`** | Optional prefs; currently `skills_hint_max_bytes` (clamped 4Ki–256Ki, default 10Ki). See `shared/user_settings.rs`. |
| **`mcp.json`** | MCP server definitions + `workspace_roots`, `custom_tools` metadata, etc. Secrets migrated out to keychain when applicable. |
| **`skills/`** | Custom skills + **`.disabled.json`** (disabled slugs). See [guides/skills.md](../guides/skills.md). |

`PENGINE_MCP_CONFIG` can relocate **`mcp.json`** only; `connection.json` / `user_settings.json` stay with app data.

## OS secure store

`secure_store` keeps a **unified JSON blob** (`AppSecretsV1`) in the platform store (macOS keychain, Linux Secret Service, Windows Credential Manager):

- Telegram **bot tokens** (keyed by `bot_id`)
- **MCP passthrough** environment values referenced from config

**Warm path:** On `setup`, a **dedicated `std::thread`** runs `warm_app_secrets` **before** Tokio-heavy work — avoids blocking-task panics during Tao launch (`app.rs` comment). Tokens are then served from RAM for normal operation.

Legacy per-item keychain entries may be merged once; duplicate prompts on first upgrade are expected.

## Startup sequence (`app.rs`)

1. Resolve **`connection.json`** path and **`mcp.json`** path (`mcp_service::resolve_mcp_config_path`).
2. **Warm secrets** thread: load connection metadata, collect MCP passthrough pairs from config, `secure_store::warm_app_secrets`.
3. **`AppState::new`** — loads `skills_hint_max_bytes` from `user_settings.json`.
4. Store `AppHandle` in state (for frontend events).
5. **`tauri::async_runtime::spawn`:** `mcp_service::rebuild_registry_into_state` — connects stdio/native servers **in background** (slow `npx`/Podman does not block the window). Until this finishes, `ToolRegistry` may be empty → early Telegram turns see **no tools**.
6. **Resume bot:** If `connection.json` exists, load token from secure store, `verify_token`, `start_bot`.

**HTTP server:** `tauri::async_runtime::spawn` → `http_server::start_server` near the end of `app.rs` setup (runs alongside MCP connect and bot resume). Default bind: **`127.0.0.1:21516`**.

## Frontend vs packaged paths

The web UI assumes **`PENGINE_API_BASE`** in `src/shared/api/config.ts` matches the embedded server. In **`tauri dev`**, MCP path resolution can prefer a **project** `mcp.json` when present; `GET /v1/mcp/config` reports the active `config_path` and `source`.
