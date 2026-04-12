# Manual tool image publish (GHCR)

Tool container images for Pengine live on **GitHub Container Registry (GHCR)**. This document describes where they appear in the browser, how to authenticate, and how to build and push images manually (including multi-arch with Podman).

## Where the images are

### Registry reference (pull / deploy)

Images follow the naming used in [`.github/workflows/tools-publish.yml`](../../.github/workflows/tools-publish.yml):

| Piece | Value |
|--------|--------|
| Registry host | `ghcr.io` |
| Repository path | `pengine-ai/tools/pengine-<suffix>` |

The `<suffix>` is the segment after `/` in the tool `id` from `tools/<slug>/pengine-tool.json`. Example: id `pengine/file-manager` → image **`ghcr.io/pengine-ai/tools/pengine-file-manager`**.

Tags pushed by CI (and recommended for manual pushes):

- **`<version>`** — e.g. `0.1.0` (from `pengine-tool.json` or from a release tag)
- **`latest`** — convenience tag (same digest target as CI may vary over time; prefer a version tag for reproducibility)

Pull examples:

```bash
podman pull ghcr.io/pengine-ai/tools/pengine-file-manager:0.1.0
podman pull ghcr.io/pengine-ai/tools/pengine-file-manager:latest
```

Digest-pinned pull (what the catalog uses internally):

```bash
podman pull ghcr.io/pengine-ai/tools/pengine-file-manager@sha256:<digest>
```

### Web UI (browse packages on GitHub)

1. Open the organization’s packages list:  
   **https://github.com/orgs/pengine-ai/packages**
2. Find the container whose name matches the image path (e.g. **`tools/pengine-file-manager`**).

Direct link pattern (URL-encoded `/` in the package name):

**https://github.com/orgs/pengine-ai/packages/container/tools%2Fpengine-file-manager**

If that URL returns 404, the package may not exist yet (nothing pushed) or the visible name may differ slightly—use the org **Packages** tab and search for `pengine-file-manager`.

---

## Prerequisites

- **Membership / role** on the **`pengine-ai`** GitHub org with permission to push packages for this image namespace (org admins configure this).
- A **Personal Access Token (PAT)** with:
  - **Classic PAT:** scope **`write:packages`** (and usually **`read:packages`**).
  - **Fine-grained PAT:** **Packages → Read and write** for the relevant org/repo.
- If the org enforces **SAML SSO:** authorize the PAT for **`pengine-ai`** (GitHub → Settings → Developer settings → your token → **Configure SSO**).

Log in with Podman:

```bash
podman logout ghcr.io
podman login ghcr.io -u '<GITHUB_USERNAME>'
# Password = the PAT (not your GitHub account password)
```

If push fails with `permission_denied` / `token provided does not match expected scopes`, the PAT is missing **`write:packages`** or SSO is not authorized for the org.

---

## Manual build and push (Podman, multi-arch)

Containers here are **Linux** images. Multi-arch for Mac / Windows / Linux **hosts** means publishing **`linux/amd64`** and **`linux/arm64`** (same as CI). You do not build separate “macOS” or “Windows” images for this Dockerfile.

From the **repository root**, with `SLUG` set to the directory under `tools/` (e.g. `file-manager`):

```bash
SLUG=file-manager
VERSION=$(jq -r '.version' "tools/$SLUG/pengine-tool.json")
IMAGE=ghcr.io/pengine-ai/tools/pengine-$(jq -r '.id | split("/")[1]' "tools/$SLUG/pengine-tool.json")
MANIFEST="${IMAGE}:${VERSION}"
```

If **`${IMAGE}:${VERSION}`** was already used for a single-arch image, free the name before creating a manifest list:

```bash
podman untag "${IMAGE}:${VERSION}" 2>/dev/null || true
podman untag "${IMAGE}:latest" 2>/dev/null || true
```

Build both architectures into one manifest, then push:

```bash
podman manifest create "$MANIFEST"

podman build \
  --platform linux/amd64 \
  --manifest "$MANIFEST" \
  -f "tools/${SLUG}/Dockerfile" \
  "tools/${SLUG}/"

podman build \
  --platform linux/arm64 \
  --manifest "$MANIFEST" \
  -f "tools/${SLUG}/Dockerfile" \
  "tools/${SLUG}/"

podman manifest push "$MANIFEST" "docker://${IMAGE}:${VERSION}"
podman manifest push "$MANIFEST" "docker://${IMAGE}:latest"
```

On **Apple Silicon**, building `linux/amd64` may require QEMU/binfmt (Podman Machine / Podman Desktop often provide this). If `build` fails with exec/format errors for the non-native arch, install or enable user-mode emulation for that platform.

**Optional:** CI signs images with **cosign**; manual pushes are unsigned unless you add signing yourself.

---

## After push: catalog digest

Pengine’s allowlisted catalog pins images by **digest** in `catalog/entries/*.json`. After a successful push, obtain the digest and update the entry (and open a PR if required by your process):

```bash
podman pull "${IMAGE}:${VERSION}"
podman image inspect "${IMAGE}:${VERSION}" --format '{{index .RepoDigests 0}}'
```

Use the `sha256:…` value in the matching `versions[]` entry for that tool.

---

## CI instead of local Podman

To publish from GitHub without a local build:

1. Actions → **Publish tool images** ([`tools-publish.yml`](../../.github/workflows/tools-publish.yml)).
2. **Run workflow**; set **tools** to the slug (e.g. `file-manager`) or `all`.

Alternatively, push an annotated-style tag such as **`file-manager-v0.2.0`** to trigger a publish for that slug only (see workflow for tag parsing).

---

## Related files

- Tool source and `Dockerfile`: `tools/<slug>/`
- Tool manifest: `tools/<slug>/pengine-tool.json`
- Catalog entry (digests): `catalog/entries/`
- CI workflow: `.github/workflows/tools-publish.yml`
