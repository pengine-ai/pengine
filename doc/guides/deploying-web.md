# Deploying the web app

The public production URL for the web UI is **`https://pengine.net`** (DNS A/AAAA → your host; TLS at the reverse proxy). The Vite production build embeds this via [`VITE_APP_ORIGIN`](../../.env.production) for client-side metadata.

The pengine web bundle is deployed to a remote host via the
[`Deploy web app`](../../.github/workflows/web-deploy.yml) GitHub Actions
workflow. Each run:

1. Builds [`deploy/Dockerfile`](../../deploy/Dockerfile) from the **repo root**
   (see [`/.dockerignore`](../../.dockerignore)): `bun install` → [`bun run build:web`](../../package.json)
   (same as Tauri [`build.beforeBuildCommand`](../../src-tauri/tauri.conf.json) / [`build.frontendDist`](../../src-tauri/tauri.conf.json); **no** `tauri build` or Rust) → [static-web-server](https://github.com/static-web-server/static-web-server)
   with **`SERVER_FALLBACK_PAGE`** so React Router paths resolve. CI passes
   `VITE_APP_ORIGIN=https://pengine.net` as a **build-arg** (overridable locally).
2. Pushes the image to GHCR as `ghcr.io/<owner>/pengine-web:<version>` and
   `:latest`.
3. SSHes into the deploy host, copies
   [`deploy/docker-compose.yml`](../../deploy/docker-compose.yml) to `~/pengine`,
   logs in to GHCR, then `docker compose pull && docker compose up -d`.

The container publishes on `127.0.0.1:1420`. **TLS and reverse-proxy (e.g. nginx)
for the public site are not defined in this repository** — maintain that in
your ops / infrastructure repo and point it at `http://127.0.0.1:1420` (or
adjust the published port in `docker-compose.yml` to match your layout).

## Triggers

- **Tag push** — pushing a tag matching `v*` (e.g. `v1.0.1`) deploys that tag.
  The same tag also fires [`App Release`](../../.github/workflows/app-release.yml);
  the two run in parallel.
- **Manual dispatch** — from the Actions tab, pick any existing tag to
  redeploy it.

## Required secrets

Add under *Settings → Secrets and variables → Actions*:

| Secret | Value |
| --- | --- |
| `DEPLOY_HOST` | Hostname or IP of the deploy target |
| `DEPLOY_USER` | SSH user on the target (must be in the `docker` group) |
| `DEPLOY_SSH_KEY` | Private SSH key (PEM, including `-----BEGIN`/`-----END` lines), with its public half in the target user's `~/.ssh/authorized_keys` |

Optional:

| Secret | Value |
| --- | --- |
| `DEPLOY_HOST_KNOWN_HOSTS` | One or more `known_hosts` lines for `DEPLOY_HOST` (paste output of a **verified** `ssh-keyscan`). If omitted, the workflow runs `ssh-keyscan` at deploy time instead. |

GHCR auth on the host uses the per-run `GITHUB_TOKEN` — no extra secret
needed, but the host must be able to reach `ghcr.io` on 443.

## One-time host bootstrap

On the deploy host, as `DEPLOY_USER`:

```bash
# Docker + compose plugin (Debian/Ubuntu shown; adapt for your distro).
curl -fsSL https://get.docker.com | sh
sudo usermod -aG docker "$USER"
# Log out and back in so the group takes effect.

# Directory the workflow drops docker-compose.yml into.
mkdir -p ~/pengine
```

Configure your external reverse-proxy (from your other repository) to forward
HTTPS traffic to `http://127.0.0.1:1420` — that is the contract between host
ingress and this deployment.

## Verifying a deploy

After the workflow succeeds:

```bash
ssh "$DEPLOY_USER@$DEPLOY_HOST" 'docker ps --filter name=pengine'
curl -fsSL https://pengine.net/ | head
```

To roll back, manually dispatch the workflow with the previous tag.

## Local image build

```bash
docker build -f deploy/Dockerfile --build-arg VITE_APP_ORIGIN=https://pengine.net -t pengine-web:local .
docker run --rm -p 8080:80 pengine-web:local
# Open http://127.0.0.1:8080
```
