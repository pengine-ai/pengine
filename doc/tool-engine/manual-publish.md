# Manual tool image publish (GHCR)

Tool container images for Pengine live on **GitHub Container Registry (GHCR)**. This document covers where they appear, how to authenticate, and how to build and push images manually.

## Where the images are

| Piece           | Value                               |
| --------------- | ----------------------------------- |
| Registry host   | `ghcr.io`                           |
| Repository path | `pengine-ai/pengine-<suffix>` |

The `<suffix>` comes from the tool `id` in `tools/mcp-tools.json`. Example: id `pengine/file-manager` → image `ghcr.io/pengine-ai/pengine-file-manager`.

Pull examples:

```bash
podman pull ghcr.io/pengine-ai/pengine-file-manager:0.1.0
podman pull ghcr.io/pengine-ai/pengine-file-manager:latest
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
REGISTRY="tools/mcp-tools.json"
VERSION=$(jq -r --arg s "$SLUG" '.tools[] | select(.id | endswith("/" + $s)) | .current' "$REGISTRY")
IMAGE=$(jq -r --arg s "$SLUG" '.tools[] | select(.id | endswith("/" + $s)) | .image' "$REGISTRY")
MANIFEST="${IMAGE}:${VERSION}"
```

If the tag was already used locally for a single-arch image, free it first. `podman untag` only removes **local** name references in your Podman store; it does **not** delete or retag manifests on remote registries such as GHCR (remote tags and digests are unchanged). To change or remove tags in GHCR, use the GitHub Packages UI, `gh api`, or another registry-specific tool.

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

## After push: update the registry

After a successful push, get the digest:

```bash
podman image inspect "${IMAGE}:${VERSION}" --format '{{index .RepoDigests 0}}'
```

Update the `sha256:…` value in the matching `versions[]` entry in **`tools/mcp-tools.json`**. The app fetches this file from GitHub at runtime; the embedded `src-tauri/src/modules/tool_engine/tools.json` is the offline fallback.

---

## Updating upstream npm versions

Run the update script (like `npm update` for tool images):

```bash
./tools/update-upstream.sh              # check all tools
./tools/update-upstream.sh file-manager # check one tool
```

This checks the npm registry for newer versions, bumps `mcp-tools.json`, and prints a summary. Commit, push, and CI builds only the affected tools.

---

## CI instead of local Podman

1. Actions → **Publish tool images** ([`tools-publish.yml`](../../.github/workflows/tools-publish.yml)).
2. **Run workflow**; set **tool** to the slug (e.g. `file-manager`) or `all`.

Or just push changes to `tools/` on `main` — CI builds automatically for tools whose version changed.

CI runs **one job per tool** on **`ubuntu-24.04-arm`**: two `docker/build-push-action` steps (native **linux/arm64**, then **linux/amd64** via QEMU), then **`docker buildx imagetools create`** so **one** multi-arch tag (`:version` / optional `:latest`) and **one digest** appear in the job summary. Staging tags `:version-ci-amd64-<run>` / `:version-ci-arm64-<run>` may still show in the GitHub Packages UI until you delete them; use `:version` or the digest from the summary for production.

---

## Troubleshooting: 0 commands

If a tool shows in the MCP Tools list but has **0 commands**, the `podman run` argv is wrong. For images whose Dockerfile sets `ENTRYPOINT` to the MCP server, `mcp_server_cmd` in the registry must be `[]`. Extra tokens get appended as argv and break startup.

Fix: update the registry and reinstall the tool from the Tool Engine panel.

---

## Upstream MCP npm version

For tools that install an npm package (e.g. File Manager), put `upstream_mcp_npm` in the tool's entry in `mcp-tools.json`:

```json
"upstream_mcp_npm": {
  "package": "@modelcontextprotocol/server-filesystem",
  "version": "2026.1.14"
}
```

CI passes these as `docker build` args so you bump the npm version in the registry instead of editing the Dockerfile.

---

## Files

- **`tools/mcp-tools.json`** — tool registry (all tools, versions, digests, npm). CI and the app read this.
- **`tools/<slug>/Dockerfile`** — image build context.
- **`tools/update-upstream.sh`** — bump upstream npm versions (like `npm update`).
- **`src-tauri/src/modules/tool_engine/tools.json`** — embedded catalog (offline fallback). Update after publish.
- **`.github/workflows/tools-publish.yml`** — CI workflow.
