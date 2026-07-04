# CLAUDE.md

Guidance for working in this repository. This file is the always-loaded core; the
detail lives in `docs/` — read the relevant one before working in its area:

- [`docs/api-contracts.md`](./docs/api-contracts.md) — every HTTP endpoint, wire shape, the search syntax, caching/ETag/sitemap behavior, import/sync mechanics
- [`docs/architecture.md`](./docs/architecture.md) — the fully annotated file map for `api/src/` and `web/src/`, plus test organization
- [`docs/operations.md`](./docs/operations.md) — running, CI, releases, Docker/deploy, and the full environment-variable reference
- [`docs/tradeoffs.md`](./docs/tradeoffs.md) — known trade-offs and the design rationale behind the invariants below

## What this is

**TCGLense** tracks trading-card games: card catalog (Magic first, via Scryfall),
singles price history with charts, per-user collections (with Archidekt/Moxfield
import + CSV upload), a wish list, and sealed products (browse/facets/price history
via TCGCSV, card→product contents via MTGJSON, buy links). Auth is email-first
registration with verification/reset mail (Resend), hardened by Turnstile CAPTCHA and
rate limiting. A self-hostable dataset mirror re-serves the upstream data files.
**Not yet built:** set-completion progress (the collection data is its foundation).

## Layout

A monorepo, two independent apps talking over HTTP:

| Dir    | App                     | Stack |
|--------|-------------------------|-------|
| `api/` | Backend (HTTP JSON API) | Rust 2024 · axum 0.8 · SeaORM 1.1 over SQLite (Postgres optional) · JWT (HS256) · Argon2 |
| `web/` | Frontend (SPA)          | Vue 3.5 · Vite 8 · Pinia · TanStack Query (vue-query) · vue-router · Tailwind 4 · shadcn-vue · TypeScript |

In dev the API runs on `:8080`, the web app on `:5173`, and the Vite dev server
proxies `/api` → the API (`web/vite.config.ts`), so the browser is same-origin and
the httpOnly refresh cookie is first-party. The API's CORS layer also allows the
`:5173` origin with credentials for direct cross-origin calls.

**Search trap:** `.claude/` is gitignored but may hold dozens of nested full-repo
worktrees (`.claude/worktrees/…`). Scope repo-wide greps/finds to `api/` and `web/`
(or exclude `.claude`), and never edit a file through a `.claude/worktrees/…` path —
that changes a different branch's checkout, not this tree.

## Running it

Two terminals:

```sh
# API, from api/
cp .env.example .env      # first run only (ships a placeholder JWT_SECRET)
cargo run                 # http://localhost:8080 — migrations run on every boot

# Web, from web/  (Node ^22.18 or ≥24.12 — CI runs 24)
npm install               # first run only
npm run dev               # http://localhost:5173
```

Or `./scripts/dev.sh` runs both (it refuses to start if `:8080`/`:5173` are already
listening — a stale server holds the port).

- The server **refuses to start without a real `JWT_SECRET`** (≥ 32 bytes, not the
  public dev constant); `ALLOW_INSECURE_DEV_SECRET=true` opts into an insecure
  compiled-in secret for local dev only. It binds `127.0.0.1` (set `HOST=0.0.0.0`
  for containers/LAN).
- SQLite is the default (`api/tcglense.db`, gitignored, WAL mode — `-wal`/`-shm`
  sidecars are normal). Point `DATABASE_URL` at `postgres://…` to switch backends
  **at runtime** — both drivers are compiled in, no cargo feature.
  `deploy/docker-compose.yml` brings up Postgres + Redis for the gated tests.
- The frontend calls relative `/api/...` URLs; set `VITE_API_URL` only when the API
  lives on a different origin.

## Commands & checks

| Where  | Command | Purpose |
|--------|---------|---------|
| `api/` | `cargo run` / `cargo check` / `cargo test` / `cargo clippy` | run · type-check · tests (also regenerates ts-rs types) · lints |
| `web/` | `npm run dev` / `npm run build` / `npm run type-check` / `npm run lint` / `npm run format` / `npm run test:unit` / `npm run test:e2e` | dev · build · vue-tsc · oxlint+eslint (both `--fix`) · oxfmt · vitest · Playwright |

**Before calling a change done:** `cargo check` + `cargo test` for `api/` work;
`npm run type-check && npm run lint && npm run test:unit -- --run` for `web/` work.

**CI** (`.github/workflows/ci.yml`) gates PRs with: `cargo test --locked` **plus a
ts-rs drift check** (fails if `web/src/lib/api/generated/` doesn't match the Rust
DTOs), the env-gated Postgres+Redis integration tests (`cargo test --locked --
--ignored` against service containers), `npm run type-check` + vitest, and a
Playwright e2e job. CI does **not** run lint/format/clippy — the checklist above is
the only thing catching those. Releases: `./scripts/release.sh` (bumps versions, tags,
`gh release create` → `release.yml` builds/pushes 3 Docker images; the workflow file
must already be on `main` for a release to fire it). See `docs/operations.md`.

**e2e gotcha:** Playwright's `webServer` starts only the *web* server (dev `:5173`;
`preview :4173` on CI). Start the API yourself with `SEED_DUMMY_DATA=true` first —
the specs probe `/api/health` and **silently skip** if it's unreachable, so the suite
can "pass" without testing anything. The seed also creates the verified
`e2e@tcglense.test` / `password123` account.

## Backend map (`api/src/`)

Full annotated map: `docs/architecture.md`.

```
main.rs            bootstrap: env → tracing → DB connect → migrate → AppState::new → tasks::start → build_router → serve
router.rs          every route + middleware stack (CORS, cache layers, body limits) — factored out of main.rs so the security tests drive the exact same router in-process
tasks.rs           6h maintenance loop (prune tokens, limiter retention) + startup/periodic card-data sync, or the awaited offline dummy seed (SEED_DUMMY_DATA)
config.rs          Config from env (Debug redacts secrets) — full env-var reference in docs/operations.md
state.rs           AppState — the one construction site, shared with the security-test harness
db.rs              connect options: SQLite (WAL + registered REGEXP UDF) or Postgres (DB_* pool sizing), picked by the DATABASE_URL scheme; also db::Dialect, the per-request SQL-fragment seam
datasets.rs        SyncSource seam: each provider fetches from the real upstream or the TCGLense mirror (SYNC_FROM_UPSTREAM / DATASET_MIRROR_URL)
captcha.rs         Turnstile CAPTCHA verification (enum: Turnstile/Disabled/test ExpectToken)
client_ip.rs       client-IP resolution for rate limiting (proxy headers only when TRUST_PROXY_HEADERS)
ratelimit.rs       per-IP limiters for auth endpoints + per-user limiters for the authed surface (in-memory, or Redis via REDIS_URL)
email.rs           transactional email (enum: Resend/Disabled/test Capture)
error.rs           AppError → JSON { error } with correct status codes
extract.rs         JsonBody<T> — JSON body extractor whose rejections are JSON (400/415/422)
auth/              Argon2 passwords · HS256 JWTs · single-use rotating refresh tokens · purpose-scoped email tokens · refresh cookie · AuthUser extractor
catalog/           game registry (GAMES) + refresh_all/seed_all dispatch per game · images.rs = lazy on-disk image cache (CDN_MODE bypasses disk)
scryfall/          MTG provider: streaming bulk ingest, the Scryfall-syntax search compiler (search/), daily price snapshots, Secret Lair drops (sld_drops.json), offline dummy seed (dummy/)
tcgcsv/            sealed-product provider: product catalog + daily product price history + the opt-in one-time historic price backfill (PRICE_BACKFILL_ENABLED)
mtgjson/           card→sealed-product contents from AllPrintings.json (ETag-gated; fallback_sealed.json fills products MTGJSON left empty)
collection_import/ provider-agnostic collection import/sync: Archidekt + Moxfield + CSV → one aggregate/resolve/reconcile engine; in-memory job queue; per-provider rate limits
handlers/          auth · cache (Cache-Control + ETag middleware) · catalog/ (incl. products.rs + pricing.rs) · collection/ · wishlist/ · shared/ (cross-cutting DTOs + holdings core) · sitemap · mirror · health
entities/          SeaORM entities (users, tokens, cards, sets, prices, collection/wishlist items, products, sealed contents, …)
migrator/          one migration per file + the migrations() registry
security_tests/    19 HTTP-level suites driving build_router in-process (tower oneshot); integration_pg.rs + the Redis tests in ratelimit.rs are env-gated behind --ignored
```

### Adding a backend feature

1. **Entity:** `entities/<name>.rs` (`DeriveEntityModel`); export from
   `entities/mod.rs` **and** `entities/prelude.rs`.
2. **Migration:** the date prefix is **frozen** — files are `m20240101_0000NN_<name>.rs`,
   so the next one increments only the counter (don't use today's date). Register it
   in `migrator/mod.rs` in **two places**: a `mod` line and a `Box::new(...)` entry in
   the `migrations()` vec. Runs on next boot.
3. **Handler + route:** module under `handlers/`, wire in `router.rs`. Return
   `AppError` — never `unwrap`/`expect`/`panic!` on a request path. SeaORM query API
   only (parameterized; no hand-built SQL — anything raw must go through the
   `db::Dialect` seam or it breaks one backend). Use `JsonBody<T>`, not raw `Json<T>`,
   so malformed-body errors stay JSON. New public routes belong in the right cache
   group (`handlers/cache.rs`), and likely deserve a `security_tests/` suite.
4. **Wire types:** derive `ts_rs::TS` on response DTOs (gated
   `#[cfg_attr(test, derive(ts_rs::TS))]`) — see the frontend recipe for regeneration.

Adding a TCG = a `Game` in `catalog::GAMES` + a provider module + one arm each in
`catalog::refresh_all` and `catalog::seed_all`.

## Frontend map (`web/src/`)

Full annotated map: `docs/architecture.md`.

```
main.ts / App.vue / router/   shell (MainNav: Products [Cards + Sealed] · Collection · Wish list · user menu) + routes; guard handles requiresAuth/requiresGuest + one-time session restore
lib/api/           typed fetch client, one module per surface (auth/catalog/products/collection/collection-import/wishlist); generated/ = ts-rs wire types
lib/queries.ts     useAuthedQuery/useAuthedMutation — vue-query wrappers routed through auth.authFetch
lib/…              pure helpers: seo, mana, money, searchQuery/searchBuilder, setGroups, buyLinks, turnstile, persistedRef, …
stores/            Pinia: auth (in-memory access token, hand-tuned single-flight refresh), theme, cardSize
composables/       query hooks: useCatalog, useProducts, useCollection, useWishlist, useCollectionImport, useQuickAdd, useSetGrouping, useTurnstile, …
components/        cards/ (grids, tiles, detail dialog, search) · products/ · collection/ · wishlist/ · legal/ · ui/ (shadcn-vue primitives)
views/             public: home, catalog (/cards…), sealed (/sealed…), auth + email-flow pages, terms/privacy; signed-in: /collection…, /wishlist…, profile
test/              vitest fixtures (the Playwright e2e specs live one level up, at web/e2e/ — not under src/)
```

### Adding a frontend feature

- **Wire types are generated**: derive `ts_rs::TS` on the Rust DTO, then run
  `cargo test` from `api/` (only `cargo test` regenerates — not check/build; config in
  `api/.cargo/config.toml`). Never hand-edit `lib/api/generated/*.ts` — **except**
  `generated/index.ts`, a hand-maintained barrel: add an export line for each new DTO
  (as is `lib/api/index.ts`). CI fails on drift.
- **Server state → vue-query** via `useAuthedQuery`/`useAuthedMutation` (don't call
  `authFetch` yourself for reads); public catalog pages use plain `useQuery`.
  Invalidate dependent queries after mutations; set a per-query `staleTime`
  (`Infinity` for static data). Footgun: reactive params go **inside**
  `queryKey` as refs/computed (`['prices', productId, range]`), never `.value`, or
  refetch-on-change breaks. **Client state → Pinia**; never duplicate a datum in both.
  Do **not** wrap `stores/auth.ts`'s refresh in vue-query — its single-flight rotation
  is hand-tuned.
- **Pages:** view under `views/` + route in `router/index.ts`; authed pages need
  `meta: { requiresAuth: true }`; auth/signed-in pages are `noindex`. Each view sets
  its own head tags via `usePageMeta()`.
- **UI primitives:** prefer `npx shadcn-vue@latest add <name>`; hand-written ones copy
  the `components/ui/button/Button.vue` idiom (reka-ui `Primitive`, `cva`, `data-slot`,
  `cn()`). `@vueuse/core` is only a transitive dep — don't import it; use Vue 3.5
  `defineModel` for v-model.

## Invariants — don't break these

The rationale lives in `docs/tradeoffs.md`; the wire-level contract detail in
`docs/api-contracts.md`.

**Auth** (contract details: `docs/api-contracts.md`):
- Anti-enumeration: `register`/`resend-verification`/`forgot-password` answer
  generically no matter whether the account exists. Login is a generic 401 with
  timing equalization — the dummy-hash verify on unknown users is load-bearing, not
  dead code.
- Refresh tokens are single-use, rotated by atomic conditional `UPDATE`, with
  lineage-based reuse detection (replay of a superseded token revokes the family).
  Cookie: `HttpOnly; SameSite=Lax; Path=/api/auth; Secure=COOKIE_SECURE`. JWTs decode
  with the algorithm pinned to HS256.
- Email tokens are purpose-scoped in the DB claim (a completion token can't spend as
  a reset, or vice versa), single-use, on a 60s DB-backed issue cooldown. Password
  rules are checked **before** a token is consumed, so a weak password doesn't burn
  the link. `reset-password` revokes **every** refresh token; a completion link is
  refused once the account has a password.
- A missing/rejected `captcha_token` is **400** (deliberately not 401/403 — it must
  never collide with login's 403), verified before any account work.
- With no `RESEND_API_KEY`, register **returns the completion token in the response**
  and login skips the unverified-403 gate — the intended dev/CI posture. **Never run
  prod without the key**: that posture lets anyone activate any address.
- Rate limiting fails **open** (Redis outage, unresolvable IP); CAPTCHA fails closed.
  Behind a proxy, `TRUST_PROXY_HEADERS=true` is required or every client keys as the
  proxy's IP — and it's only safe if the proxy strips/replaces inbound
  `X-Forwarded-For` (`deploy/edge.Caddyfile` overwrites it with the real client IP
  for exactly this reason). The per-user limiter engages only on a *valid* bearer
  token — it is not an IP-level DoS guard.

**Collection / wish list / imports:**
- A holding is `(user, game, card) → { quantity, foil_quantity }`; both counts zero
  **deletes the row**; `PUT` counts are absolute. Card ids in paths are the
  **external** (catalog) id, resolved to internal `cards.id` before storage so
  holdings survive re-imports. Caps: quantity ≤ 1,000,000; batch lookups ≤ 500 ids.
- The wish list is a fully independent table — it never touches `collection_items`
  (pinned by a security test) — but reuses the collection's ts-rs DTOs verbatim
  (`owned_*` fields read as "wanted"; its batch route is `POST …/counts`, not
  `/owned`). Editing a shared DTO in `handlers/shared/holdings.rs` changes both wire
  surfaces.
- URL imports run async (202 + job id, single-slot queue, jobs in-memory —
  lost on restart, per-process even with Redis); CSV imports run synchronously
  (16 MB body cap). URL modes: `overwrite/replace/merge/smart`; CSV accepts no
  `smart`. A **replace**-mode import (URL or CSV) matching **zero** catalog cards is
  refused so a bad source can't wipe a collection (merge/overwrite just report zero
  matches). **Smart sync never deletes** upstream-removed
  cards — only a full replace does. CSV shape sniffing checks Archidekt (id column)
  **before** Moxfield, because Archidekt's quantity column also spells "Count".
- Import URLs are built host-side from validated ids (no SSRF surface). Moxfield
  *live* import is currently disabled (`Provider::network_import_enabled()` is the
  single source of truth; CSV upload is the supported path) pending an approved
  `MOXFIELD_USER_AGENT` (treat as a credential); page fetches keep a whole-request
  60s deadline because Moxfield tarpits unapproved clients.

**Data sync & assets:**
- Card import is version-gated on `ingest_state.source_updated_at`; a zero-card run
  records as `error` (not version-locked) so it retries. The dataset is paper-only,
  English-or-sole-language.
- `SEED_DUMMY_DATA` is upsert-only — point it at a fresh/dedicated DB or you get a
  real+dummy mix. It stamps version `dummy-seed-v1`, so a later real sync re-imports.
- `datasets::SyncSource` is the one seam deciding upstream vs mirror.
  `MIRROR_ENABLED` stays off by default (an enabled mirror is a public proxy to the
  upstreams).
- MTGJSON sealed-contents is gated on the file's HTTP **ETag** (`Meta.json` bumps
  daily — useless as a gate) + the `fallback_sealed.json` content hash
  (`compose_version`); the fallback merges per-product only when MTGJSON emitted zero
  rows. The table rebuilds wholesale each run.
- Secret Lair drop titles come only from the committed `scryfall/sld_drops.json`
  snapshot (regenerate: `api/scripts/gen-sld-drops.mjs`); a post-snapshot drop falls
  into a trailing "Other" group. Nothing in the DB stores drop membership.
- Card images are cached lazily on first view — never bulk-download (Scryfall
  guideline). Fetches are host-locked to `scryfall.io`, redirects disabled,
  concurrency-capped; `CDN_MODE=true` skips disk (only sane behind a caching CDN).

**HTTP surface:**
- Response caching is two-tier middleware (`handlers/cache.rs`): public catalog reads
  get CDN-cacheable headers; auth/status/errors are `no-store`; image/icon routes set
  their own `immutable` which the layer preserves. The ETag layer sits **outside** the
  cache layer (it must read the Cache-Control already set), GET-only, and skips
  `immutable`/`no-store` responses.
- Card-list endpoints paginate `?page` (1-based) + `?page_size` (default 60, max 200)
  → `{ data, page, page_size, total, has_more }`. **Card**-list `q` is near-full
  Scryfall syntax (`scryfall/search/`); malformed/unsupported queries are 422; user
  values only ever bind as SQL parameters. The **products** list's `q` is a plain
  name substring, not Scryfall syntax. The `/regex/` filter runs Rust-regex on SQLite vs POSIX
  `~*` on Postgres (via `db::Dialect`) — exotic patterns can match differently.
- Sitemaps are served under `/api/` (the only backend-routed path) but their `<loc>`s
  are SPA routes built on `PUBLIC_SITE_URL` — never emit `/api/...` URLs in them.

**Dependency & build pins:**
- `jsonwebtoken` must keep `default-features = false, features = ["rust_crypto"]` or
  it panics at runtime. `reqwest` is pinned to rustls + gzip/stream/json with **no**
  overall timeout by design (the bulk download streams; a `read_timeout` guards
  stalls) — don't "fix" either when bumping.
- The Docker Rust stage needs `cmake` + `nasm` (aws-lc-sys); images are
  `linux/amd64`-only. The combined image's SPA fallback must stay
  `ServeDir::fallback(ServeFile(index.html))` — `not_found_service` would 404
  deep-linked SPA routes (pinned by `security_tests::web_root`).
- `web/src/assets/mana-font.css` must load **after** the package CSS (it adds the
  woff2 face that wins the cascade). `{HW}`/`{HR}` half-mana renders as literal text —
  known limitation.

## Conventions

- **TS/Vue:** no semicolons, single quotes, 2-space indent, max 100 cols (oxfmt +
  oxlint + eslint enforce); `<script setup lang="ts">`, Pinia setup stores, `@/` →
  `src/`. Run `npm run format` then `npm run lint` after editing.
- **Rust:** edition 2024; errors flow through `AppError`; `expect` only in `main.rs`
  startup. Add deps with `cargo add`.

## Environment variables

Full annotated reference: `docs/operations.md` (and `api/src/config.rs` +
`api/.env.example`, which stay authoritative). The ones that change behavior in ways
you can't guess:

- `JWT_SECRET` (required, ≥ 32 bytes) / `ALLOW_INSECURE_DEV_SECRET` (local-dev
  escape hatch).
- `RESEND_API_KEY` unset = email disabled ⇒ register returns the completion token +
  login skips the verified gate (dev/CI only — never prod).
- `TURNSTILE_SECRET_KEY` (API) pairs with `VITE_TURNSTILE_SITE_KEY` (web): both set
  or both unset.
- `TRUST_PROXY_HEADERS=true` only behind a trusted proxy; `RATE_LIMIT_ENABLED`;
  `REDIS_URL` makes the auth/user limiters cross-instance (imports stay per-process).
- `SEED_DUMMY_DATA=true` overrides `SYNC_ON_STARTUP`/`SYNC_INTERVAL_HOURS`, seeds the
  offline catalog + e2e account, no network. `SYNC_FROM_UPSTREAM` /
  `DATASET_MIRROR_URL` / `MIRROR_ENABLED` control dataset sourcing/serving.
- `WEB_ROOT` set = the API also serves the built SPA (combined image); unset = the
  API is `/api`-only. `CDN_MODE=true` disables the on-disk image cache.
- Web builds: `VITE_API_URL` (only for a cross-origin API), `VITE_SITE_URL`
  (build-time, `robots.txt`); the API's `PUBLIC_SITE_URL` builds sitemap/email links.
