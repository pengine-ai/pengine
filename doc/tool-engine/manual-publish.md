# Manual tool image publish (GHCR)

Tool container images for Pengine live on **GitHub Container Registry (GHCR)**. This document covers where they appear, how to authenticate, and how to build and push images manually.

## Where the images are

| Piece | Value |
|--------|--------|
| Registry host | `ghcr.io` |
| Repository path | `pengine-ai/tools/pengine-<suffix>` |

The `<suffix>` comes from the tool `id` in `tools/<slug>/pengine-tool.json`. Example: id `pengine/file-manager` → image `ghcr.io/pengine-ai/tools/pengine-file-manager`.

Pull examples:

```bash
podman pull ghcr.io/pengine-ai/tools/pengine-file-manager:0.1.0
podman pull ghcr.io/pengine-ai/tools/pengine-file-manager:latest
```

Browse packages on GitHub: **https://github.com/orgs/pengine-ai/packages**

---

## Prerequisites

- Membership on the `pengine-ai` GitHub org with package push permissions.
- A **Personal Access Token (PAT)** with **`write:packages`** scope.

```bash
podman login ghcr.io -u '<GITHUB_USERNAME>'
# Password = the PAT (not your GitHub account password)
```

---

## Build and push (Podman, multi-arch)

From the **repository root**:

```bash
SLUG=file-manager
MANIFEST_FILE="tools/${SLUG}/pengine-tool.json"
VERSION=$(jq -r '.version' "$MANIFEST_FILE")
IMAGE=ghcr.io/pengine-ai/tools/pengine-$(jq -r '.id | split("/")[1]' "$MANIFEST_FILE")
MANIFEST="${IMAGE}:${VERSION}"
```

If the tag was already used locally for a single-arch image, free it first. `podman untag` only removes **local** tags — it does not affect remote images on GHCR. For remote cleanup, use the GitHub Packages web UI or `gh api`:

```bash
podman untag "${IMAGE}:${VERSION}" 2>/dev/null || true
podman untag "${IMAGE}:latest" 2>/dev/null || true
```

Build both architectures and push:

```bash
podman manifest create "$MANIFEST"

podman build --platform linux/amd64 --manifest "$MANIFEST" "tools/${SLUG}/"
podman build --platform linux/arm64 --manifest "$MANIFEST" "tools/${SLUG}/"

podman manifest push --all "$MANIFEST" "docker://${IMAGE}:${VERSION}"
podman manifest push --all "$MANIFEST" "docker://${IMAGE}:latest"
```

On **Apple Silicon**, building `linux/amd64` requires QEMU (Podman Machine usually provides this).

**Optional:** CI signs images with **cosign**; manual pushes are unsigned unless you add signing yourself.

---

## After push: update the embedded catalog

After a successful push, get the digest:

```bash
podman image inspect "${IMAGE}:${VERSION}" --format '{{index .RepoDigests 0}}'
```

Update the `sha256:…` value in the matching `versions[]` entry in **`src-tauri/src/modules/tool_engine/tools.json`** (the embedded catalog baked into the Pengine binary).

---

## CI instead of local Podman

1. Actions → **Publish tool images** ([`tools-publish.yml`](../../.github/workflows/tools-publish.yml)).
2. **Run workflow**; set **tool** to the slug (e.g. `file-manager`) or `all`.

Or just push changes to `tools/<slug>/` on `main` — CI builds automatically.

---

## Troubleshooting: 0 commands

If a tool shows in the MCP Tools list but has **0 commands**, the `podman run` argv is wrong. For images whose Dockerfile sets `ENTRYPOINT` to the MCP server, `mcp_server_cmd` in the embedded catalog must be `[]`. Extra tokens get appended as argv and break startup.

Fix: update the embedded catalog and reinstall the tool from the Tool Engine panel.

---

## Upstream MCP npm version

For tools that install an npm package (e.g. File Manager), put `upstream_mcp_npm` in `pengine-tool.json`:

```json
"upstream_mcp_npm": {
  "package": "@modelcontextprotocol/server-filesystem",
  "version": "2026.1.14"
}
```

CI passes these as `docker build` args so you bump the npm version in the manifest instead of editing the Dockerfile.

---

## Files

- **`tools/<slug>/pengine-tool.json`** — tool manifest (id, version, limits, npm). CI reads this.
- **`tools/<slug>/Dockerfile`** — image build.
- **`src-tauri/src/modules/tool_engine/tools.json`** — embedded catalog (what the app uses at runtime). Update digests here after publish.
- **`.github/workflows/tools-publish.yml`** — CI workflow.
