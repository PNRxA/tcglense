# TCGLense

A trading-card-game tracking app — track retail/MSRP and singles prices over time,
your personal collection, and set-completion progress.

> Status: MTG support initially, others to follow

## Stack

- **`api/`** — Rust API: [axum](https://github.com/tokio-rs/axum) · [SeaORM](https://www.sea-ql.org/SeaORM/) over SQLite (Postgres optional) · JWT access tokens + httpOnly refresh cookies · Argon2
- **`web/`** — Vue 3 SPA: Vite · Pinia · Vue Router · Tailwind 4 · shadcn-vue · TypeScript

## Getting started

**Prerequisites:** [Rust](https://rustup.rs/) (cargo) and [Node](https://nodejs.org/) 22.18+ / 24.12+. SQLite is embedded — nothing to install. Postgres is optionally supported (set `DATABASE_URL=postgres://…`); a `deploy/docker-compose.yml` brings up Postgres + Redis for local parity.

Run both servers (API on `:8080`, web on `:5173`) with one command:

```sh
./scripts/dev.sh
```

It auto-creates `api/.env` (from `api/.env.example`) and installs web deps on first run.
Then open http://localhost:5173.

<details>
<summary>Run them manually instead</summary>

```sh
# terminal 1 — API
cd api && cp .env.example .env && cargo run

# terminal 2 — web
cd web && npm install && npm run dev
```
</details>

## Layout

```
api/         Rust HTTP JSON API (auth, database, migrations)
web/         Vue single-page app
scripts/     dev + release helpers (dev.sh runs both servers, release.sh cuts a release)
deploy/      production config (Caddyfile, systemd unit, docker-compose)
Dockerfile   multi-stage build for the api / web / combined images
CLAUDE.md    architecture, conventions, and how to add features
```

## Common commands

| | API (`cd api`) | Web (`cd web`) |
|---|---|---|
| Run (dev) | `cargo run` | `npm run dev` |
| Test | `cargo test` | `npm run test:unit` |
| Lint | `cargo clippy` | `npm run lint` |
| Build | `cargo build --release` | `npm run build` |

## Deployment

Two supported Docker topologies (plus a bare-metal option). Both need a `JWT_SECRET`
— the API refuses to boot without one (generate with `openssl rand -hex 32`). Images
are published to GHCR + Docker Hub on every release (see [Docker images](#docker-images--releases)).

### Homelab — one container + SQLite

The **combined** image runs the API and SPA together (the API serves the SPA via
`WEB_ROOT`), backed by embedded SQLite. The database and cached card images persist
in one volume — nothing else to run.

```sh
export JWT_SECRET=$(openssl rand -hex 32)
docker compose -f deploy/docker-compose.homelab.yml up -d
# then open http://<host>:8080
```

<details>
<summary>…or a plain <code>docker run</code></summary>

```sh
docker run -d --name tcglense -p 8080:8080 \
  -e JWT_SECRET="$(openssl rand -hex 32)" \
  -v tcglense_data:/data \
  ghcr.io/pnrxa/tcglense:latest
```
</details>

On first boot the API imports MTG card data from Scryfall in the background (a
one-off ~30s; set `SYNC_ON_STARTUP=false` to skip, or `SEED_DUMMY_DATA=true` for an
offline catalog). For HTTPS, put a reverse proxy / tunnel in front and set
`COOKIE_SECURE=true` + `PUBLIC_SITE_URL=https://your-host`.

### Production — Caddy + web + api + Postgres + Redis

The scalable split: an edge **Caddy** terminates TLS and forwards to the **web**
container (serves the SPA, proxies `/api`), which talks to the **api** container,
backed by **Postgres** and **Redis** (shared rate-limiter state across instances).
Config lives in [`deploy/docker-compose.prod.yml`](./deploy/docker-compose.prod.yml)
+ [`deploy/edge.Caddyfile`](./deploy/edge.Caddyfile).

```
internet ──443──▶ caddy (TLS) ──▶ web (SPA + /api proxy) ──▶ api ──▶ db    (Postgres)
                                                                 └──▶ cache (Redis)
```

```sh
export SITE_ADDRESS=tcglense.example.com          # your domain — Caddy auto-provisions HTTPS
export JWT_SECRET=$(openssl rand -hex 32)
export POSTGRES_PASSWORD=$(openssl rand -hex 24)
docker compose -f deploy/docker-compose.prod.yml up -d
```

Point DNS for `SITE_ADDRESS` at the host with ports 80+443 reachable and Caddy
obtains a Let's Encrypt certificate on first boot; the API runs migrations on start.
Pin a release across the whole stack with `IMAGE_TAG=vX.Y.Z` (defaults to `latest`).

#### Behind a CDN (Cloudflare)

The public catalog (`/api/games/*`) and the image/icon proxy already emit CDN-friendly
`Cache-Control` (`s-maxage` / `immutable`), so a CDN caches them with no code change.
To put Cloudflare in front:

- Set **`CDN_MODE=true`** on the `api` service — the origin then skips caching card
  images to disk (the CDN absorbs the repeats), so it needs no writable image dir.
  Leave it off if no CDN sits in front, or every view re-fetches upstream.
- Add a **cache rule** — Cloudflare doesn't cache API paths by default; add a rule to
  cache eligible responses (honoring the origin `Cache-Control`) for `/api/games/*` and
  the image/icon routes. Keep `/api/auth/*` uncached (they're already `no-store`).
- **TLS:** set Cloudflare SSL/TLS to **Full (strict)** and give Caddy a valid origin
  cert — a Cloudflare Origin CA cert, or Let's Encrypt via the DNS-01 challenge (the
  HTTP-01 challenge can be interfered with by Cloudflare's proxy).

> **Real client IP (important).** With Cloudflare in front, the edge Caddy's immediate
> peer is *Cloudflare*, so the default `edge.Caddyfile` line
> `header_up X-Forwarded-For {http.request.remote.host}` makes **every** request key to
> a Cloudflare IP — collapsing per-IP auth rate limiting. Switch it to Cloudflare's
> real-client header **and restrict origin ingress to Cloudflare's IP ranges** (a
> firewall allowlist or a Cloudflare Tunnel) so it can't be spoofed by hitting the
> origin directly:
>
> ```caddyfile
> reverse_proxy web:80 {
> 	header_up X-Forwarded-For {http.request.header.CF-Connecting-IP}
> }
> ```

### Bare metal (systemd + Caddy)

<details>
<summary>Run the binary + static SPA directly, no containers</summary>

1. **Build both:**
   ```sh
   cd web && npm ci && npm run build       # -> web/dist/
   cd ../api && cargo build --release       # -> api/target/release/tcglense-api
   ```
2. **Place the artifacts** on the server, e.g.:
   ```sh
   install -D api/target/release/tcglense-api /srv/tcglense/tcglense-api
   mkdir -p /srv/tcglense/web && cp -r web/dist/. /srv/tcglense/web/
   ```
3. **Configure** `/srv/tcglense/api.env` (start from `api/.env.example`) with
   production values — at minimum:
   ```sh
   JWT_SECRET=...          # openssl rand -hex 32 — required
   COOKIE_SECURE=true      # HTTPS-only refresh cookie
   HOST=127.0.0.1          # API listens on localhost; only Caddy reaches it
   DATABASE_URL=sqlite:///var/lib/tcglense/tcglense.db?mode=rwc   # dir must exist + be writable
   DATA_DIR=/var/lib/tcglense/data   # persistent + writable; holds cached card images
   ```
4. **Run the API as a service** (Linux/systemd): copy
   `deploy/tcglense-api.service` to `/etc/systemd/system/`, adjust paths/user, then
   ```sh
   sudo systemctl daemon-reload && sudo systemctl enable --now tcglense-api
   ```
5. **Run Caddy:** set your domain and site root in `deploy/Caddyfile`, then
   `caddy run --config deploy/Caddyfile` (HTTPS is automatic for a real domain).

To ship an update: rebuild, copy the new `tcglense-api` + `web/dist`, then
`sudo systemctl restart tcglense-api`.
</details>

## Docker images & releases

The three images are built from one multi-stage [`Dockerfile`](./Dockerfile)
(`linux/amd64`) and published to **GHCR** (`ghcr.io/pnrxa/…`) and **Docker Hub**
(`docker.io/pnrxa/…`) on every release:

| Image | Target | What it is |
|-------|--------|------------|
| `tcglense-api` | `api` | the Rust API only (serves `/api`) |
| `tcglense-web` | `web` | the built SPA served by Caddy (proxies `/api`) |
| `tcglense` | `combined` | the API **and** SPA in one image (API serves the SPA via `WEB_ROOT`) |

Pull any of them by tag (`vX.Y.Z`, `X.Y`, or `latest`):

```sh
docker pull ghcr.io/pnrxa/tcglense:latest
docker pull docker.io/pnrxa/tcglense-api:latest
```

**Cutting a release:**

```sh
./scripts/release.sh          # prompts for the version, then bumps + tags + publishes
```

It bumps `api/Cargo.toml` + `web/package.json` (and the lockfiles), commits, tags
`vX.Y.Z`, pushes, and publishes a GitHub Release — which triggers the **Release
images** workflow ([`.github/workflows/release.yml`](./.github/workflows/release.yml))
to build and push all three images. A pre-release version (`X.Y.Z-rc.1`) is flagged
as a GitHub pre-release and does **not** move the `latest` tag.

**Registry auth:** GHCR needs nothing to *push* (the built-in `GITHUB_TOKEN`), but the
first push creates each package **private** — to allow the anonymous `docker pull`s
above, set each package's visibility to Public once (GitHub → your profile → Packages →
the package → settings). For Docker Hub, add repo secrets `DOCKERHUB_USERNAME` and
`DOCKERHUB_TOKEN` (a Docker Hub [access token](https://hub.docker.com/settings/security))
— the Docker Hub push then turns on automatically; until then only GHCR is pushed.
Optional build-time web config (`VITE_SITE_URL`, `VITE_TURNSTILE_SITE_KEY`) comes from
repo **variables** of the same name.

## Docs

See [`CLAUDE.md`](./CLAUDE.md) for the architecture, the auth API contract,
project conventions, and step-by-step guides for adding new features.
