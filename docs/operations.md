# Operations: running, testing, CI, releases, deployment, env vars

This is the on-demand operations reference for TCGLense. `CLAUDE.md` is the
always-loaded core (a slim overview); this file holds the encyclopedic
detail on how to run the two apps locally, the full test/command matrix, the CI and
release pipelines, the Docker images and deployment topologies, the `scripts/` and
`deploy/` inventories, and the complete annotated environment-variable reference.
Read it when you need to run, test, ship, or deploy the app — or to look up exactly
what an env var does. It is self-contained for that scope.

## Running it

Two terminals (or `./scripts/dev.sh`, which runs both together — see the scripts
inventory below).

**API** (from `api/`):

```sh
cp .env.example .env      # first run only; edit JWT_SECRET
cargo run                 # serves http://localhost:8080, runs DB migrations on boot
```

The SQLite file is created automatically (`tcglense.db`, gitignored) and runs in **WAL
journal mode** with a ~20 MB per-connection page cache (`db.rs`) so reads and writes
don't block each other at the SQLite layer and hot pages stay resident in RAM; WAL adds
`tcglense.db-wal`/`-shm` sidecar files (also gitignored). Migrations run on every
startup via `Migrator::up`. The server **refuses to start without a real `JWT_SECRET`**
(≥ 32 bytes, and not the public dev constant). For local dev
without a secret, set `ALLOW_INSECURE_DEV_SECRET=true` to opt into a publicly-known
insecure secret (logged as a warning) — never set that outside local dev. The shipped
`.env.example` includes a placeholder `JWT_SECRET`, so `cp .env.example .env` then
`cargo run` works out of the box. The server binds `127.0.0.1` by default (set
`HOST=0.0.0.0` for containers/LAN).

**Optional Postgres.** Point `DATABASE_URL` at a `postgres://…` URL and the same
`Database::connect` picks the Postgres driver at runtime (both `sqlx-sqlite` and
`sqlx-postgres` are compiled in — no cargo feature). Migrations run on boot against
either backend (the two backend-coupled migrations, m001's case-insensitive email index
and m017's nullable `password_hash`, branch on `manager.get_database_backend()`).
Postgres pool sizing comes from `DB_MAX_CONNECTIONS`/`DB_MIN_CONNECTIONS`/
`DB_CONNECT_TIMEOUT_SECS`/`DB_ACQUIRE_TIMEOUT_SECS` (defaults 10/0/15/30); SQLite stays
single-connection with WAL + the REGEXP function. The startup log line reports the
selected backend. SQLite remains the default — nothing to install. (`deploy/docker-compose.yml`
brings up Postgres + Redis for local parity / the gated integration tests.)

**Web** (from `web/`):

```sh
npm install               # first run only
npm run dev               # serves http://localhost:5173
```

The frontend calls **relative `/api/...` URLs** (routed through the Vite proxy in
dev, same-origin in prod). Set `VITE_API_URL` only when the API lives on a
different origin. The API's CORS layer deliberately allows the `:5173` origin **with
credentials**, so the httpOnly refresh cookie also flows on *direct* cross-origin
calls (e.g. `VITE_API_URL` pointed straight at `:8080`) — the Vite proxy merely makes
dev same-origin; don't "tighten" that allowance away.

## Commands

**API** (run from `api/`):

| Command | Purpose |
|---------|---------|
| `cargo run` | Run the server (migrations included) |
| `cargo check` | Fast type/borrow check |
| `cargo test` | The full default suite: unit tests + the 19 in-process HTTP security suites; as a side effect it **regenerates the ts-rs wire types** in `web/src/lib/api/generated/` |
| `cargo build --release` | Optimized build |
| `cargo clippy` | Lints |

**Web** (run from `web/`):

| Command | Purpose |
|---------|---------|
| `npm run dev` | Dev server with HMR (Vite, `:5173`) |
| `npm run build` | Type-check + production build (`run-p type-check build-only`) |
| `npm run build-only` | `vite build` only (no type-check) |
| `npm run preview` | Serve the production build locally (`vite preview`, `:4173`) |
| `npm run type-check` | `vue-tsc --build` only |
| `npm run lint` | oxlint + eslint (both `--fix`; `run-s lint:oxlint lint:eslint`) |
| `npm run format` | oxfmt over `src/` |
| `npm run test:unit` | Vitest |
| `npm run test:e2e` | Playwright (needs the app built/served + the API running — see below) |

Before calling a change done, run `cargo check`/`cargo test` for `api/` work and
`npm run type-check && npm run lint && npm run test:unit -- --run` for `web/` work.

## Continuous integration

`.github/workflows/ci.yml` runs on every push to `main`, every pull request, and manual
`workflow_dispatch`. A newer push to the same branch/PR cancels the in-flight run
(`concurrency` group, `cancel-in-progress: true`); the workflow has `contents: read`
permission only. Four independent jobs:

| Job | Runs from | What it does |
|-----|-----------|--------------|
| `backend-tests` | `api/` | `cargo test --locked` (the default in-memory SQLite suite), then a **ts-rs drift check**: `cargo test` regenerates `web/src/lib/api/generated/` from the Rust DTOs, so the job fails if `git status --porcelain -- web/src/lib/api/generated` is non-empty (the API contract drifted — run `cargo test` in `api/` and commit the regenerated files) |
| `postgres-redis-tests` | `api/` | `cargo test --locked -- --ignored` — the **only** `#[ignore]`-gated tests in the crate (the `src/integration_pg.rs` Postgres suite + `ratelimit.rs`'s Redis suite), pointed at service containers via `TCGLENSE_TEST_POSTGRES_URL`/`TCGLENSE_TEST_REDIS_URL`. Service containers: `postgres:17` (user/pw/db all `postgres`, `:5432`) and `redis:7` (`:6379`), both health-checked. The ts-rs export only fires for the non-ignored DTO tests (which this job doesn't run), so that drift check stays owned by `backend-tests` |
| `web-unit-tests` | `web/` | Node 24 + `npm ci`, then `npm run type-check` and `npm run test:unit -- --run` (Vitest) |
| `e2e-tests` | repo root | Playwright — see the build/run detail below |

Rust jobs use `dtolnay/rust-toolchain@stable` + `Swatinem/rust-cache@v2` (workspace
`api`). Node jobs use `actions/setup-node@v5` (Node 24, npm cache keyed on
`web/package-lock.json`).

**The e2e job** builds and runs a real API so the SPA talks to a live backend serving
the deterministic offline dummy catalog (no Scryfall network calls):

1. `cargo build --locked` in `api/` (debug binary).
2. `npm ci`, `npx playwright install --with-deps`, `npm run build` in `web/`.
3. Starts the API in the background with the offline dummy catalog and waits (up to 60s,
   polling `/api/health`) for it to be healthy, failing fast with the API log if the
   process exits early. Env: `JWT_SECRET=<throwaway ≥32-byte constant>`,
   `SEED_DUMMY_DATA=true`, `SYNC_ON_STARTUP=false`,
   `DATABASE_URL=sqlite://$RUNNER_TEMP/tcglense-ci.db?mode=rwc`,
   `DATA_DIR=$RUNNER_TEMP/api-data`, `HOST=127.0.0.1`, `PORT=8080`, `RUST_LOG=warn`.
4. `npm run test:e2e` with `CI=true`. Playwright's own `webServer` config serves the
   **built SPA** via `npm run preview` on `:4173` (which proxies `/api` to the API on
   `:8080`); on CI Playwright retries failed tests twice and runs 1 worker,
   headless, against all three browsers (chromium/firefox/webkit).

The Playwright HTML report is uploaded as an artifact whenever the job isn't cancelled;
the API log is uploaded on failure (both 7-day retention).

### Running the e2e tests locally

`web/playwright.config.ts`'s `webServer` starts **only the web server** — `npm run dev`
(`:5173`) locally, `npm run preview` (`:4173`) on CI (`CI` env set). It does **not**
start the API. So a local `npm run test:e2e` needs the API running separately, seeded
with the offline dummy catalog:

```sh
# terminal 1 (from api/):
JWT_SECRET=dev-secret-please-change-0123456789abcdef \
SEED_DUMMY_DATA=true SYNC_ON_STARTUP=false cargo run

# terminal 2 (from web/):
npm run test:e2e
```

The specs probe `/api/health` first and **silently skip** if the API is unreachable
rather than failing (the `apiReachable()` guard in `web/e2e/security.spec.ts`), so a
bare `npm run test:e2e` with no API doesn't red-fail — it just skips the API-dependent
tests. `SEED_DUMMY_DATA` also seeds a verified dev account, **`e2e@tcglense.test` /
`password123`** (`api/src/tasks.rs`), used by the session/login flows.

Two spec files under `web/e2e/`:
- `security.spec.ts` — the security e2e suite: the httpOnly + SameSite refresh cookie,
  generic (non-enumerable) login failures, bearer-protected routes, malformed-body
  handling, and that the access token never reaches JS-readable storage. Registration is
  driven through the **no-email bypass** (the API runs with no email provider, so
  `register` returns the completion token in its response body and the test spends it on
  `complete-registration`); the emailed-link path + login's verification gate are covered
  by the Rust security tests (mail sink), not here.
- `vue.spec.ts` — the public welcome page renders for everyone; unauthenticated visitors
  are redirected off protected pages to `/login?redirect=…`.

## Docker images & releases

One multi-stage [`Dockerfile`](../Dockerfile) at the repo root builds **three**
images via named targets (shared `web-builder` [node → `web/dist`] and `api-builder`
[`cargo build --release --locked`; the Rust stage installs `cmake` + `nasm` for
`aws-lc-sys`, everything else — bundled SQLite, the RustCrypto stack — is vendored;
the runtime is `debian:bookworm-slim` + `ca-certificates` + `tini`, non-root, `/data`
volume] stages):

| Target | Image | What it is |
|--------|-------|------------|
| `api` | `tcglense-api` | the Rust API only (serves `/api`) |
| `web` | `tcglense-web` | the built SPA served by Caddy (`deploy/web.Caddyfile`: SPA + `/api` reverse-proxy to `{$API_UPSTREAM:api:8080}`) |
| `combined` | `tcglense` | the API **and** SPA in one process — the API serves the SPA from `WEB_ROOT=/srv/web` (see the `WEB_ROOT` config field + the `fallback_service` in `router.rs`) |

The **`WEB_ROOT` static-SPA fallback** is the one code seam the combined image needs:
when set, `build_router` adds a `tower-http` `ServeDir` fallback so any non-`/api`
request serves the SPA — a real file directly, and an unknown path via
`ServeDir::fallback(ServeFile(index.html))` (**not** `not_found_service`, which would
force a 404), so a deep-linked SPA route returns `index.html` with a **200**. A
lowest-priority `/api/{*rest}` catch-all keeps an *unknown* API path a JSON 404
(registered routes + their handler 404s still win). Unset leaves the API `/api`-only,
so the split (Caddy) deployment and existing installs are untouched. Pinned by the
`security_tests::web_root` suite.

**Release CD** (`.github/workflows/release.yml`): `on: release: published` (+ manual
`workflow_dispatch` with an optional `ref` input, tagged `:edge`) builds all three
images in a matrix (`fail-fast: false`) and pushes them to **GHCR** (`ghcr.io/pnrxa/…`,
via the built-in `GITHUB_TOKEN` — needs `packages: write`) and, when the
`DOCKERHUB_USERNAME`/`DOCKERHUB_TOKEN` repo secrets are set, **Docker Hub**
(`docker.io/pnrxa/…`; the Docker Hub push is gated on the secrets' presence — resolved
into a job step so GHCR-only works out of the box; the first GHCR push creates each
package **private** — make it Public once, per package, to allow anonymous pulls).
`docker/metadata-action` derives the image tags: for a release `vX.Y.Z`, the exact git
tag (`vX.Y.Z`) plus the semver-normalized `X.Y.Z` + `X.Y`, and `latest` only on a
**non-pre-release** release; a manual `workflow_dispatch` is tagged `:edge`. Builds are
`linux/amd64` only (multi-arch is future work — Rust under QEMU is slow). Per-target
layer caching in the GitHub Actions cache (`combined` also reads the `api`/`web` scopes
so it can reuse shared builder stages across runs). Optional build-time public web
config comes from the repo **variable** `VITE_SITE_URL`
(`VITE_API_URL` is baked empty → relative `/api`). The Turnstile site key is no
longer baked in — it's the API's runtime `TURNSTILE_SITE_KEY`, served via
`GET /api/config`.

Two compose files run the published images: `deploy/docker-compose.homelab.yml` (the
combined image + SQLite, one container) and `deploy/docker-compose.prod.yml` (the full
split: edge Caddy [`deploy/edge.Caddyfile`] + web + api + Postgres + Redis).

**Cutting a release:** `./scripts/release.sh` prompts for the version, bumps it in
`api/Cargo.toml` (+ `Cargo.lock` via `cargo update -p tcglense-api`) and
`web/package.json` (+ lock via `npm version`), commits, tags `vX.Y.Z`, pushes, and
`gh release create`s the GitHub Release that triggers the workflow. Prerequisites: a
clean working tree, and `git`/`cargo`/`npm`/`gh` on `PATH` with `gh` authenticated. The
workflow file must already be on the default branch for the release to fire it, so land
it on `main` before the first release.

## scripts/ inventory

Repo-root `scripts/`:

| Script | What it does |
|--------|--------------|
| `scripts/release.sh` | Cut a release: prompt for a version, bump it in `api/Cargo.toml` (+ `Cargo.lock`) and `web/package.json` (+ `package-lock.json`), commit, tag `vX.Y.Z`, push, and publish the GitHub Release that fires the "Release images" workflow. Run from anywhere; needs a clean tree + authenticated `gh` |
| `scripts/dev.sh` | Run both dev servers together — API (`api/`, `cargo run`, `:8080`) + web (`web/`, `npm run dev`, `:5173`) — streaming both to one terminal (colors/HMR intact). Ctrl+C stops both; if either exits, the other is torn down |

`api/scripts/`:

| Script | What it does |
|--------|--------------|
| `api/scripts/gen-sld-drops.mjs` | Regenerate `api/src/scryfall/sld_drops.json` — the committed snapshot of Scryfall's curated Secret Lair Drop titles (they aren't in the bulk card API; only the set's gallery page carries them). Parses the page once and commits the result. Node 18+ (global `fetch`), no npm deps: `node api/scripts/gen-sld-drops.mjs` |

`web/scripts/`:

| Script | What it does |
|--------|--------------|
| `web/scripts/gen-og-image.mjs` | Regenerate `web/public/og-image.png` — the branded 1200×630 social/link-unfurl banner used as the site-wide default OG/Twitter image (card pages override it with card art). Rendered from an inline HTML/SVG template via Playwright's bundled Chromium and screenshotted at exactly 1200×630, so the committed PNG is reproducible without a design tool. Re-run after a brand/wordmark/tagline change |
| `web/scripts/gen-favicons.mjs` | Regenerate the raster favicon assets (`favicon.ico`, `apple-touch-icon.png`, `icon-192.png`, `icon-512.png`) from the one hand-authored source `web/public/favicon.svg`, so a brand tweak only needs editing the SVG + re-running this. Each PNG is rendered from the SVG via Playwright's bundled Chromium at its exact size, transparent corners preserved; `favicon.ico` is assembled from the 16px + 32px PNGs |

Both `web/scripts/*` require the Playwright browsers installed
(`npx playwright install chromium`).

## deploy/ inventory

| File | What it is |
|------|------------|
| `deploy/docker-compose.yml` | Local Postgres + Redis for optional-backend development and the gated integration tests (`cargo test -- --ignored` from `api/`). Not used by the default SQLite dev flow |
| `deploy/docker-compose.homelab.yml` | Homelab stack: the single `combined` image + SQLite in one container (API serves `/api` **and** the SPA via `WEB_ROOT`); DB + cached images persist in the `tcglense_data` volume. `JWT_SECRET` required; put a reverse proxy in front for HTTPS + set `COOKIE_SECURE=true` + `PUBLIC_SITE_URL`. Pin a release with `IMAGE_TAG=vX.Y.Z` (defaults `:latest`) |
| `deploy/docker-compose.prod.yml` | Production stack: edge Caddy (TLS) → `web` (Caddy: SPA + `/api` proxy) → `api` → Postgres (`db`) + Redis (`cache`). only `JWT_SECRET` is hard-required (compose `:?` refuses to start without it); `SITE_ADDRESS` defaults to `localhost` and `POSTGRES_PASSWORD` to `tcglense` — set all three for a real deployment. Images from GHCR, pin with `IMAGE_TAG` |
| `deploy/docker-compose.do.yml` + `deploy/do.Caddyfile` + `deploy/do.env.example` | Managed-cloud stack for the DigitalOcean + Upstash + Cloudflare deploy: one Droplet runs Caddy + the `combined` image, backed by DO Managed Postgres (private VPC, `?sslmode=require`) and Upstash Redis (`rediss://`), fronted by a free Cloudflare CDN. The Caddyfile terminates origin TLS (Cloudflare Origin cert) and rewrites `X-Forwarded-For` from `CF-Connecting-IP`. Full walkthrough: [`deploy-digitalocean.md`](./deploy-digitalocean.md) |
| `.do/app.yaml` | DigitalOcean **App Platform** (PaaS) spec: one `combined`-image instance + DO Managed Postgres, **no Redis, single instance**. Ephemeral disk ⇒ `CDN_MODE=true` (Cloudflare holds the images). The release workflow's `deploy-app-platform` job auto-redeploys it on a published Release when `DIGITALOCEAN_ACCESS_TOKEN` + `DIGITALOCEAN_APP_ID` secrets are set. Walkthrough (incl. the per-IP rate-limit caveat): [`deploy-app-platform.md`](./deploy-app-platform.md) |
| `deploy/web.Caddyfile` | Caddy config baked into the `tcglense-web` image: serve the built SPA + reverse-proxy `/api/*` to the API (upstream defaults to compose service `api:8080`, override with `API_UPSTREAM`). Terminate TLS in front of this container |
| `deploy/edge.Caddyfile` | Edge reverse proxy for the prod compose stack: terminates TLS for `{$SITE_ADDRESS:localhost}` and forwards everything to the `web` container. **Overwrites** (not appends) `X-Forwarded-For` with the real client IP so the API can safely key per-IP rate limiting on the left-most XFF entry (`TRUST_PROXY_HEADERS=true`) without spoofing |
| `deploy/Caddyfile` | Standalone single-origin production reverse proxy (Caddy v2): serves the built SPA **and** forwards `/api/*` to the Rust API on the same origin (keeps the httpOnly refresh cookie first-party). Auto-provisions HTTPS for a real domain. `caddy run --config deploy/Caddyfile` |
| `deploy/tcglense-api.service` | systemd unit for the API: copy to `/etc/systemd/system/`, adjust `User`/paths, `systemctl enable --now`. Production env (`JWT_SECRET`, `COOKIE_SECURE=true`, `HOST=127.0.0.1`, `DATABASE_URL`, …) supplied by the unit |

## Environment variables

- **API:** `DATABASE_URL` (default `sqlite://tcglense.db?mode=rwc`; the scheme selects
  the backend at runtime — `sqlite://` or `postgres://`, both drivers compiled in),
  `DB_MAX_CONNECTIONS` (10) / `DB_MIN_CONNECTIONS` (0) / `DB_CONNECT_TIMEOUT_SECS` (15) /
  `DB_ACQUIRE_TIMEOUT_SECS` (30) — Postgres connection-pool sizing, ignored on SQLite,
  `REDIS_URL` (unset; a `redis://` — or `rediss://` for TLS — URL backing the per-IP +
  per-user rate limiters — shared across instances when set, in-memory otherwise; fails
  open on outage; a secret if it embeds a password; `rediss://` TLS **is** supported
  (rustls with bundled Mozilla roots, so hosted providers like Upstash work over the
  public internet); requires **Redis 5.0+** — the limiter Lua script uses server `TIME` +
  `SET`, rejected by older Redis's script effect replication), `JWT_SECRET`
  (**required**, ≥ 32 bytes, not the dev constant), `ALLOW_INSECURE_DEV_SECRET`
  (false; opt-in to the insecure compiled-in secret for local dev only),
  `ACCESS_TOKEN_EXPIRY_MINUTES` (15), `REFRESH_TOKEN_EXPIRY_DAYS` (30),
  `COOKIE_SECURE` (false), `HOST` (`127.0.0.1`), `PORT` (8080),
  `PUBLIC_SITE_URL` (`http://localhost:5173`; public SPA origin used for the sitemap
  `<loc>`s **and** every emailed link — completion/verify/reset — so set it to the real
  site origin in prod), `RUST_LOG` (`info`),
  `DATA_DIR` (`./data`; holds cached card images under
  `images/`), `WEB_ROOT` (unset; when set, the API also serves the built SPA static
  files from this dir with an `index.html` fallback for client-side routes — the
  single-process "combined" Docker image sets `WEB_ROOT=/srv/web`; unset = the API
  serves only `/api`, so existing deployments are unaffected — see `router.rs`),
  `CDN_MODE` (`false`; when `true` the image/icon proxy skips the
  on-disk cache and only fetch-and-serves — for origins behind a caching CDN),
  `SCRYFALL_USER_AGENT` (descriptive UA Scryfall requires),
  `TCGCSV_USER_AGENT` (descriptive UA sent on **every** TCGCSV request — the daily
  sealed-product sweep and the one-time historic price backfill; TCGCSV blocks generic
  UAs; defaults to the same fallback as the Scryfall UA),
  `PRICE_BACKFILL_ENABLED` (`false`; the one-time TCGCSV historic price backfill is
  **opt-in** — set `true` to walk TCGCSV's daily archives once and fill
  `card_price_history` for the days before the app began capturing its own snapshots. Off
  by default because the walk is slow and hits an external service; it's internally gated
  by an `ingest_state` row so it only ever runs once and never overwrites an existing
  `(card, date)` row), `PRICE_BACKFILL_DAYS` (`0` = every archive day since 2024-02-08;
  `N` = only the most recent `N` days, to bound a first run),
  `MOXFIELD_USER_AGENT` (unset; the Moxfield-approved UA for collection URL imports —
  email support@moxfield.com to get one approved, and treat it as a secret),
  `RESEND_API_KEY` (unset; the Resend API key for registration-completion/verification/reset email — a
  secret; unset = email sending disabled, messages logged instead), `EMAIL_FROM`
  (`TCGLense <onboarding@resend.dev>`; the outbound From address — Resend's shared
  onboarding sender only delivers to the account owner, so set a verified-domain
  sender in prod),
  `TURNSTILE_SECRET_KEY` (unset; Cloudflare Turnstile secret for the auth-endpoint
  CAPTCHA — a secret; unset = CAPTCHA disabled/checks pass), `TURNSTILE_SITE_KEY`
  (unset; the **public** Turnstile site key the browser widget renders with —
  served to the SPA at runtime via `GET /api/config`, so the published web image
  needs no rebuild to change it. Pairs with `TURNSTILE_SECRET_KEY`: set both or
  neither, or the server refuses to boot), `TRUST_PROXY_HEADERS`
  (`false`; trust `X-Forwarded-For`/`Forwarded` for the rate-limiter client IP —
  set `true` **only** behind a trusted proxy, else clients can spoof their IP),
  `RATE_LIMIT_ENABLED` (`true`; per-IP auth rate limiting — set `false` to defer to
  an upstream WAF),
  `SYNC_ON_STARTUP` (`true`; import card data on boot — set `false` for offline
  dev/tests), `SYNC_INTERVAL_HOURS` (`24`; re-import cadence after the startup
  import — `0` disables the periodic refresh; only applies when `SYNC_ON_STARTUP`
  is on), `SEED_DUMMY_DATA` (`false`; seed a deterministic offline dummy catalog
  instead of importing real data — **takes precedence** over `SYNC_ON_STARTUP`/
  `SYNC_INTERVAL_HOURS`, does no network sync, upsert-only so point it at a
  fresh/dedicated DB),
  `SYNC_FROM_UPSTREAM` (`false`; fetch the raw datasets straight from
  Scryfall/MTGJSON/TCGCSV — the mirror host's posture — instead of from the mirror at
  `DATASET_MIRROR_URL`), `DATASET_MIRROR_URL` (`https://tcglense.com`; the TCGLense
  mirror a self-host reads datasets from by default; trailing slash trimmed),
  `MIRROR_ENABLED` (`false`; serve the `/api/mirror/*` dataset endpoints so other
  instances can pull the datasets from this one — off by default so a self-host isn't
  an open proxy to the upstreams; the public mirror sets it with
  `SYNC_FROM_UPSTREAM=true`). See `api/.env.example`.
- **Web:** `VITE_API_URL` (default empty → relative `/api`, via the dev proxy).
  (The Turnstile site key is no longer a web build var — it's the API's runtime
  `TURNSTILE_SITE_KEY`, fetched by the SPA from `GET /api/config`.)
  `VITE_SITE_URL` (public site origin, default `http://localhost:5173`) — used at
  **build time** for the absolute `Sitemap:` URL in `robots.txt`; canonical and OG
  URLs are resolved at runtime from the live origin, and the sitemap itself is
  API-served (so the API's `PUBLIC_SITE_URL` builds its `<loc>`s). Set it in
  production CI (alongside the API's matching `PUBLIC_SITE_URL`).
