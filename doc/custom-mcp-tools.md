# Adding custom MCP tools

Pengine loads MCP servers from **`mcp.json`**. Each entry is either:

- **`stdio`** — a child process that speaks MCP over stdin/stdout (Node `npx` packages, a local binary, or `docker`/`podman run …`).
- **`native`** — an in-process tool pack built into Pengine (`dice`, `tool_manager`).

The dashboard **Tools** column lists whatever is in `mcp.json`. Use **+ Add custom tool** (paste JSON or manual form), edit an existing **stdio** entry, or edit the file directly.

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

- [design/mcp.md](design/mcp.md) — protocol and module overview (may be partially superseded by implementation details in code).
- [tool-engine/manual-publish.md](tool-engine/manual-publish.md) — publishing catalog tools (maintainers).
