# Adding custom MCP tools

This guide is for **operators and power users**: anyone wiring MCP servers into Pengine by hand, from the dashboard, or via the HTTP API. It complements [architecture/mcp.md](../architecture/mcp.md) (how the host talks to clients) and the in-app **MCP Tools** / **Tool Engine** panels.

---

## Concepts in one minute

| Piece | What it is |
|-------|------------|
| **`mcp.json`** | Single source of truth for MCP servers, workspace roots, and Tool Engine metadata. |
| **`stdio` server** | A subprocess: Pengine spawns it and speaks MCP over stdin/stdout. |
| **`native` server** | Compiled into the app; no separate process. |
| **Tool Engine** | Curated Docker/Podman images: install from the dashboard → a `te_…` **stdio** entry is generated for you. |
| **Custom container tool** | Any image you trust, registered via **`POST /v1/toolengine/custom`** → `te_custom_<key>`. |

**MCP tools** are real runtime capabilities (containers, processes, native packs). They are **not** the same as [Skills](./skills.md) (markdown hints in the system prompt). Use MCP when the agent needs a long-lived protocol, side effects on disk, or a packaged binary you do not want to re-describe in prose.

---

## Dashboard vs file vs API

| Approach | Best when |
|----------|-----------|
| **Dashboard → MCP Tools** | Edit names, env, filesystem paths, `direct_return`, or paste a single server JSON. Good for interactive tweaks. |
| **Dashboard → Tool Engine** | Install catalog tools with one click; secrets and workspace mounts are guided. |
| **Edit `mcp.json` directly** | You use version control, templating, or CI to ship the same config to many machines. |
| **`POST /v1/toolengine/custom`** | Automating custom images without opening the UI. |

After any change, Pengine reloads MCP configuration (watch the log for `mcp` lines). If something fails to start, the server usually disappears from the tool list until the next successful spawn.

---

## Where `mcp.json` lives

| Situation | Path |
|-----------|------|
| Packaged app (recommended mental model) | Next to `connection.json` under the app data directory (same folder as Telegram token storage). |
| Local dev | Same as packaged: next to `connection.json` (see `mcp_service::resolve_mcp_config_path`). Use `PENGINE_MCP_CONFIG` if you want a repo-local file. |
| Override | Set **`PENGINE_MCP_CONFIG`** to an absolute or relative path. |

The active path is returned by **`GET http://127.0.0.1:21516/v1/mcp/config`** (`config_path` field).

---

## Rules for tool names (server keys)

Keys under `servers` must be **non-empty**, at most **64** characters, and use only **ASCII letters, digits, `-`, and `_`** (no spaces).

Tool Engine installs use deterministic keys like `te_pengine-file-manager`. Custom Docker tools registered via the API use `te_custom_<your-key>`.

---

## 1. Normal MCP package (Node / `npx`)

Pengine spawns **`command`** with **`args`** and talks MCP over stdio. For published MCP servers on npm, the usual pattern is `npx` with `-y` and the package name.

**Example — filesystem server on the host** (paths are on your Mac/Linux machine, not in Docker):

```json
{
  "workspace_roots": [],
  "servers": {
    "my-fs": {
      "type": "stdio",
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/Users/you/Documents/project"],
      "env": {},
      "direct_return": false
    }
  }
}
```

**Example — environment variable for an API key:**

```json
{
  "type": "stdio",
  "command": "npx",
  "args": ["-y", "@some-scope/some-mcp-server"],
  "env": {
    "SOME_API_KEY": "replace-me"
  },
  "direct_return": false
}
```

Requirements:

- **`type`** is required (`stdio` or `native`).
- For **`stdio`**, **`command`** must be non-empty.
- Node/npm must be on `PATH` for the process that starts Pengine (the Tauri app or your terminal if you test manually).

After saving `mcp.json` (or using the dashboard save), Pengine reconnects MCP servers; check the in-app log for `mcp` lines.

### `stdio` fields (quick reference)

| Field | Required | Notes |
|-------|----------|--------|
| `type` | Yes | `stdio` or `native`. |
| `command` | For `stdio` | Executable on `PATH` for the Pengine process (Tauri app or `pengine` server). |
| `args` | No | List of argv tokens after `command`. |
| `env` | No | Plain strings; treat values as secrets if they are API keys. |
| `direct_return` | No | If `true`, tool results can bypass an extra model summarization step (when the stack supports it). |
| `private_host_path` | No | Used by some Tool Engine tools for bind-mounted data dirs (see Tool Engine panel). |

### Common issues

- **`npx` / `node` not found** — The user or service account that launches Pengine must have the same `PATH` you use in a terminal. On macOS GUI apps often have a minimal `PATH`; prefer absolute paths to `node`/`npx` in `command` if needed.
- **Filesystem MCP shows no folders** — Ensure **`workspace_roots`** (or dashboard “allowed folders”) includes host paths; the server argv often lists those paths after the `server-filesystem` package line.
- **Container never sees your project** — For Tool Engine installs, enable workspace mount options when registering the custom tool, or add explicit `-v` binds in a hand-written `docker run` argv.
- **Image pulls hang** — Pulls run on the host; disk, network, and registry auth are all outside Pengine. Check Docker/Podman CLI manually with the same image ref.

---

## 2. Docker / Podman MCP image

There are three common ways:

### A. Tool Engine catalog (curated)

Install from the dashboard **Tool Engine** panel. That pulls a **digest-pinned** image from the allowlisted catalog and writes a `te_…` **stdio** entry whose command is `docker` or `podman` and whose args are a generated `run …` line. No manual `docker` argv needed.

### B. “Custom tool” HTTP API (any image you choose)

For images **not** in the catalog, Pengine can pull the image, append a `servers` entry **`te_custom_<key>`**, and store metadata in **`custom_tools`** inside `mcp.json`.

**List:**

```http
GET http://127.0.0.1:21516/v1/toolengine/custom
```

**Add** (requires Docker or Podman on the host):

```http
POST http://127.0.0.1:21516/v1/toolengine/custom
Content-Type: application/json

{
  "key": "my-mcp",
  "name": "My MCP container",
  "image": "ghcr.io/your-org/your-mcp-image:latest",
  "mcp_server_cmd": [],
  "mount_workspace": true,
  "mount_read_only": true,
  "append_workspace_roots": true,
  "direct_return": false
}
```

| Field | Meaning |
|-------|---------|
| `key` | Stable id; server name becomes `te_custom_<key>`. Must be unique. |
| `name` | Human-readable label (stored in config; not the MCP server key). |
| `image` | Full image reference to `pull` / `run`. |
| `mcp_server_cmd` | Extra arguments **after** the image (e.g. if the image needs a subcommand). Often `[]`. |
| `mount_workspace` | If true, bind-mount **`workspace_roots`** from `mcp.json` into the container (same `/app/<basename>` layout as File Manager). |
| `mount_read_only` | Use `:ro` on those binds when `mount_workspace` is true. |
| `append_workspace_roots` | If true, append container paths (`/app/...`) to the argv (for servers that expect roots as trailing args). |
| `direct_return` | If true, tool results go straight to the user without a second model pass. |

**Bundled catalog (`tools/mcp-tools.json`):** the Fetch entry uses `ignore_robots_txt` (default `false`) so robots.txt is honored unless you opt in. Setting it to `true` appends `--ignore-robots-txt` for that tool. The `robots_ignore_allowlist` field is reserved for future per-host behavior and is informational today.

Set **`workspace_roots`** (dashboard: File Manager / filesystem folders, or **`PUT /v1/mcp/filesystem`**) before relying on mounts.

**Remove:**

```http
DELETE http://127.0.0.1:21516/v1/toolengine/custom/my-mcp
```

### C. Hand-written `stdio` entry (`docker` or `podman` as `command`)

You can paste or type a **stdio** block where **`command`** is `docker` or `podman` and **`args`** is the full `run` argv. This is flexible but easy to get wrong; prefer **B** if you want workspace mounts kept in sync when folders change.

**Sketch** (adapt image name, volumes, and trailing args to your image’s contract):

```json
{
  "servers": {
    "docker-mcp": {
      "type": "stdio",
      "command": "docker",
      "args": [
        "run",
        "--rm",
        "-i",
        "--network=none",
        "-v=/Users/you/project:/workspace:ro",
        "your-mcp-image:tag"
      ],
      "env": {},
      "direct_return": false
    }
  }
}
```

Use **`podman`** instead of **`docker`** if that is what you have installed. Pengine’s Tool Engine uses the same flags pattern (`--rm`, `-i`, `--network=none`, optional `-v=…`) for generated lines.

---

## 3. Native built-ins (not Docker / not npx)

**Example** (`src-tauri/mcp.example.json`):

```json
{
  "servers": {
    "dice": {
      "type": "native",
      "id": "dice"
    },
    "tool_manager": {
      "type": "native",
      "id": "tool_manager"
    }
  }
}
```

- **`dice`** — sample roll-a-die tool.
- **`tool_manager`** — install/uninstall catalog tools from chat (`manage_tools`).

Unknown **`id`** values are rejected when the server connects.

---

## 4. Paste JSON in the dashboard

Under **+ Add custom tool → Paste JSON**, you can paste either:

- A **single entry**: `{ "type": "stdio", "command": "…", … }` — then fill in **Tool name**.
- A **wrapped** object: `{ "server-key": { "type": "stdio", … } }` — the outer key becomes the server name.

---

## See also

- [architecture/mcp.md](../architecture/mcp.md) — host/client flow and Ollama bridge (details also in `src-tauri/src/modules/mcp/`).
- [guides/skills.md](./skills.md) — markdown “skills” in the system prompt vs MCP tools.
- [README.md](../README.md) — doc index and `/v1` API list.
- [tool-engine/manual-publish.md](../tool-engine/manual-publish.md) — publishing catalog tools (maintainers).
