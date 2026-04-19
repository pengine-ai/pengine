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
2. Pushes the image to GHCR as `ghcr.io/<owner>/pengine-web:<version>` (see below),
   plus `:sha-<short>` and `:latest`. **Version** is the **exact git tag** when you deploy a tag (e.g. `1.0.1`), otherwise `package.json`’s `version`.
3. On the deploy host (via [appleboy/ssh-action](https://github.com/appleboy/ssh-action)): copies
   [`deploy/docker-compose.yml`](../../deploy/docker-compose.yml) into `~/pengine`, then **`docker login` → `docker compose pull` → `docker compose up`** with **`PENGINE_WEB_IMAGE`** set to the **digest** of that build (`…/pengine-web@sha256:…`). The same image is also reachable in the registry as `:1.0.1` (or whatever version was pushed).

The container publishes on `127.0.0.1:1420`. **TLS and reverse-proxy (e.g. nginx)
for the public site are not defined in this repository** — maintain that in
your ops / infrastructure repo and point it at `http://127.0.0.1:1420` (or
adjust the published port in `docker-compose.yml` to match your layout).

## Triggers

- **Manual only** — Actions → *Deploy web app* → *Run workflow*. Choose **any
  branch** from the branch dropdown, then optionally set **ref** (another
  branch, tag, or SHA). If **ref** is empty, the workflow uses the tip of the
  branch you picked.
- **App releases** — [`App Release`](../../.github/workflows/app-release.yml) is
  still tag-driven (`v*`); it does not have to run for a web deploy.

## Required secrets

Add under *Settings → Secrets and variables → Actions*:

| Secret | Value |
| --- | --- |
| `DEPLOY_HOST` | Hostname or IP of the deploy target |
| `DEPLOY_USER` | SSH user on the target (must be in the `docker` group) |
| `DEPLOY_SSH_KEY` | Private key (full file, `BEGIN`/`END` lines), **no passphrase**. Ed25519 recommended — [generate a deploy key](#ssh-deploy-key). Public key on the host in `~/.ssh/authorized_keys`. |

Optional (not wired in the workflow today; extend the `appleboy` steps if you need host-key pinning):

| Secret | Value |
| --- | --- |
| Host key fingerprint | [appleboy/ssh-action](https://github.com/appleboy/ssh-action) supports `fingerprint` (SHA256 of the server host key) for MITM protection — add to the workflow `with:` block if you use it. |

GHCR auth on the host uses the per-run `GITHUB_TOKEN` — no extra secret
needed, but the host must be able to reach `ghcr.io` on 443.

### SSH deploy key

Use a **dedicated** Ed25519 key for CI (do not reuse a personal key or a key that has a passphrase).

#### 1. Generate the key pair

On a trusted machine:

```bash
ssh-keygen -t ed25519 -N "" -f ./pengine-deploy-gha -C "pengine-github-actions"
```

- **`-N ""`** sets an empty passphrase so GitHub Actions can use the key without prompting.
- This creates **`pengine-deploy-gha`** (private — goes into GitHub) and **`pengine-deploy-gha.pub`** (public — goes on the server).

#### 2. Install the public key on the deploy host

Replace `DEPLOY_USER` and `DEPLOY_HOST` with the same values as your secrets:

```bash
ssh-copy-id -i ./pengine-deploy-gha.pub DEPLOY_USER@DEPLOY_HOST
```

Alternatively, append the **single line** from `pengine-deploy-gha.pub` to  
`~/.ssh/authorized_keys` for `DEPLOY_USER` on `DEPLOY_HOST` (create `~/.ssh` with mode `700` and `authorized_keys` with mode `600` if needed).

#### 3. Verify login with the new private key

```bash
ssh -i ./pengine-deploy-gha DEPLOY_USER@DEPLOY_HOST 'echo ok'
```

You should see `ok` without a password prompt.

#### 4. Add the private key to GitHub

1. Open the repository → **Settings** → **Secrets and variables** → **Actions**.
2. Create or update **`DEPLOY_SSH_KEY`**.
3. Paste the **entire** private key file:

   ```bash
   cat ./pengine-deploy-gha
   ```

   Include the lines **`-----BEGIN OPENSSH PRIVATE KEY-----`** and **`-----END OPENSSH PRIVATE KEY-----`**.

#### 5. Protect local key files (optional)

```bash
chmod 600 ./pengine-deploy-gha ./pengine-deploy-gha.pub
```

Do not commit the private key. If it is ever exposed, generate a new pair and repeat steps 2–4.

#### Troubleshooting

If you see **`error in libcrypto`**, validation errors, or password prompts: wrong file (`.pub` vs private), **passphrase**, **truncated** paste, or old **RSA** PEM — use a new Ed25519 key from step 1.

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

To roll back, manually dispatch the workflow and set **ref** to a previous tag
or commit SHA, or pull a prior `sha-*` image on the host.

## Local image build

```bash
docker build -f deploy/Dockerfile --build-arg VITE_APP_ORIGIN=https://pengine.net -t pengine-web:local .
docker run --rm -p 8080:80 pengine-web:local
# Open http://127.0.0.1:8080
```
