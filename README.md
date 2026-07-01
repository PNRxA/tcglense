# TCGLense

A trading-card-game tracking app — track retail/MSRP and singles prices over time,
your personal collection, and set-completion progress.

> Status: MTG support initially, others to follow

## Stack

- **`api/`** — Rust API: [axum](https://github.com/tokio-rs/axum) · [SeaORM](https://www.sea-ql.org/SeaORM/) over SQLite · JWT access tokens + httpOnly refresh cookies · Argon2
- **`web/`** — Vue 3 SPA: Vite · Pinia · Vue Router · Tailwind 4 · shadcn-vue · TypeScript

## Getting started

**Prerequisites:** [Rust](https://rustup.rs/) (cargo) and [Node](https://nodejs.org/) 22.18+ / 24.12+. SQLite is embedded — nothing to install.

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
api/        Rust HTTP JSON API (auth, database, migrations)
web/        Vue single-page app
scripts/    dev helpers (dev.sh runs both servers)
deploy/     production config (Caddyfile, systemd unit)
CLAUDE.md   architecture, conventions, and how to add features
```

## Common commands

| | API (`cd api`) | Web (`cd web`) |
|---|---|---|
| Run (dev) | `cargo run` | `npm run dev` |
| Test | `cargo test` | `npm run test:unit` |
| Lint | `cargo clippy` | `npm run lint` |
| Build | `cargo build --release` | `npm run build` |

## Production (Caddy)

The frontend builds to static files; in production a [Caddy](https://caddyserver.com/)
reverse proxy serves them **and** forwards `/api/*` to the Rust API on the same
origin (keeping the httpOnly refresh cookie first-party). Config lives in
[`deploy/`](./deploy).

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
   JWT_SECRET=...          # openssl rand -hex 32 — required (API won't boot without it when COOKIE_SECURE=true)
   COOKIE_SECURE=true      # HTTPS-only refresh cookie
   HOST=127.0.0.1          # API listens on localhost; only Caddy reaches it
   DATABASE_URL=sqlite:///var/lib/tcglense/tcglense.db?mode=rwc   # dir must exist + be writable
   DATA_DIR=/var/lib/tcglense/data   # persistent + writable; holds cached card images
   ```
   On first boot the API imports MTG card data from Scryfall in the background
   (~100k paper cards, a one-off ~30s); card images are then cached to `DATA_DIR`
   on first view. Both are skippable/relocatable via `SYNC_ON_STARTUP` / `DATA_DIR`.
4. **Run the API as a service** (Linux/systemd): copy
   `deploy/tcglense-api.service` to `/etc/systemd/system/`, adjust paths/user, then
   ```sh
   sudo systemctl daemon-reload && sudo systemctl enable --now tcglense-api
   ```
5. **Run Caddy:** set your domain and site root in `deploy/Caddyfile`, then
   ```sh
   caddy run --config deploy/Caddyfile
   ```
   Caddy provisions HTTPS automatically for a real domain. Visit your domain — done.

To ship an update: rebuild, copy the new `tcglense-api` + `web/dist`, then
`sudo systemctl restart tcglense-api` (Caddy serves the new static files at once).

## Docs

See [`CLAUDE.md`](./CLAUDE.md) for the architecture, the auth API contract,
project conventions, and step-by-step guides for adding new features.
