# Pengine deployment (with Pengui on the same host)

## Networking

- **`ports: 1422:1422`** — app reachable on the host at **`http://127.0.0.1:1422/`**.
- **`pengui-network`** — Pengine joins Pengui’s external Docker network so **`pengui-nginx`** can proxy to **`http://pengine-app:1422`** (no `host.docker.internal`; avoids Linux docker0 / bridge mismatches).

Deploy **Pengui** first so the network **`pengui-network`** exists, then **`docker compose up -d`** here.

If your Pengui project still uses another network name (e.g. before the `name: pengui-network` change), set **`PENGUI_NETWORK_NAME`** in `.env` to that name, or run:

`docker network connect <that-network-name> pengine-app`

## Order of operations

1. **DNS** — `pengine.net` (or your subdomain) A/AAAA → server IP (same host as Pengui if you use Pengui’s TLS + nginx).
2. **GitHub variable** on the Pengui repo: **`PENGINE_SUBDOMAIN=pengine.net`** so `deploy.sh` adds `-d pengine.net` to Let’s Encrypt and writes `pengine.conf`.
3. **Pengine** — `docker compose up -d` in this directory (after Pengui has created **`pengui-network`**).
4. **Build** — For **`https://<DOMAIN>/pengine/`** use Vite `base: '/pengine/'`. For **`https://pengine.net/`** use default `base: '/'` / `VITE_APP_ORIGIN`.

## Verify

```bash
curl -fsS http://127.0.0.1:1422/ | head
docker compose -f ~/pengui/deployment/docker-compose.yml exec nginx \
  wget -qO- --timeout=5 http://pengine-app:1422/ | head
```

## TLS

If **`curl https://pengine.net/`** fails certificate verification, expand the Let’s Encrypt cert to include **`pengine.net`** (see Pengui `deployment/scripts/deploy.sh` / Certbot `--expand`).
