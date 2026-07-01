# CLAUDE.md

Guidance for working in this repository.

## What this is

**TCGLense** is a trading-card-game tracking application. The goal is to track:

- Retail and MSRP prices over time (sealed product)
- Singles prices over time
- A user's personal collection
- Set-completion progress (how much of a set you own)

The **auth foundation** (register / login / session) is built, plus a **card
catalog**: browse trading-card games → sets → cards, with search. Magic: The
Gathering is the first game, sourced from [Scryfall](https://scryfall.com).
**Singles price history** is also built: every sync captures each card's daily prices
into `card_price_history`, served as a per-card time series (`.../cards/{id}/prices`)
and charted on the card detail page. **Per-user collections** are also built: a
signed-in user records how many copies (regular + foil) of each card they own, per
game (MTG first, the model is game-agnostic), edited from the card detail page and
browsed at `/collection/{game}` with a value/count summary. A collection can also be
**imported/synced from an external provider** (Archidekt first; the layer is
provider-agnostic, Moxfield planned): a one-off import with a chosen reconcile mode
(overwrite-matched / mirror-replace / add-merge), or a saved collection link re-synced
on demand (always mirror/replace). The set-completion and
sealed-product price-tracking features are not yet implemented — they are the next
things to build on top of this scaffold (the collection gives set-completion the
owned-card data to hang off).

## Layout

A monorepo with two independent apps:

| Dir    | App           | Stack |
|--------|---------------|-------|
| `api/` | Backend (HTTP JSON API) | Rust 2024 · axum 0.8 · SeaORM 1.1 over SQLite · JWT (HS256) · Argon2 |
| `web/` | Frontend (SPA) | Vue 3.5 · Vite 8 · Pinia · TanStack Query (vue-query) · vue-router · Tailwind 4 · shadcn-vue (new-york) · TypeScript |

The two talk over HTTP. In dev the API runs on `:8080` and the web app on
`:5173`, and the **web dev server proxies `/api` to the API** (`web/vite.config.ts`)
so the browser is same-origin — the httpOnly refresh cookie is first-party. The
API's CORS layer also allows the `:5173` origin **with credentials** for any direct
cross-origin calls (needed so the browser sends the refresh cookie cross-origin).

## Running it

Two terminals.

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

**Web** (from `web/`):

```sh
npm install               # first run only
npm run dev               # serves http://localhost:5173
```

The frontend calls **relative `/api/...` URLs** (routed through the Vite proxy in
dev, same-origin in prod). Set `VITE_API_URL` only when the API lives on a
different origin.

## Commands

**API** (run from `api/`):

| Command | Purpose |
|---------|---------|
| `cargo run` | Run the server (migrations included) |
| `cargo check` | Fast type/borrow check |
| `cargo test` | Unit tests (password, JWT, refresh rotation/reuse) |
| `cargo build --release` | Optimized build |
| `cargo clippy` | Lints |

**Web** (run from `web/`):

| Command | Purpose |
|---------|---------|
| `npm run dev` | Dev server with HMR |
| `npm run build` | Type-check + production build |
| `npm run type-check` | `vue-tsc` only |
| `npm run lint` | oxlint + eslint (both `--fix`) |
| `npm run format` | oxfmt over `src/` |
| `npm run test:unit` | Vitest |
| `npm run test:e2e` | Playwright (needs the app built/served) |

Before calling a change done, run `cargo check`/`cargo test` for `api/` work and
`npm run type-check && npm run lint && npm run test:unit -- --run` for `web/` work.

## Auth API contract

Base URL `http://localhost:8080`, all routes under `/api`. Every response —
success or error — is JSON. Errors are always `{ "error": string }`.

`User` shape: `{ id: number, email: string, display_name: string | null, created_at: string (RFC3339 UTC) }`

**Two-token model:** a short-lived **access token** (JWT, 15 min, returned as
`access_token`, kept in memory on the client) plus a long-lived **refresh token**
(opaque, 30 days, delivered only as the `tcglense_refresh` httpOnly cookie, stored
server-side as a SHA-256 hash).

| Method & path | Body | Success | Notes |
|---------------|------|---------|-------|
| `POST /api/auth/register` | `{ email, password, display_name? }` | `201 { access_token, user }` + refresh cookie | `409` taken · `422` invalid |
| `POST /api/auth/login` | `{ email, password }` | `200 { access_token, user }` + refresh cookie | `401 "invalid email or password"` (generic) |
| `POST /api/auth/refresh` | — (refresh cookie) | `200 { access_token }` + **rotated** cookie | `401` if missing/invalid/expired/revoked (clears cookie) |
| `POST /api/auth/logout` | — (refresh cookie) | `204` (revokes token + clears cookie) | idempotent |
| `GET /api/auth/me` | — (`Authorization: Bearer <access_token>`) | `200 { user }` | `401` if missing/invalid/expired |
| `GET /api/health` | — | `200 { status: "ok" }` | — |

All responses (success or error) are JSON; errors are `{ "error": string }`. A
malformed JSON body is `400`, a missing/wrong `Content-Type` is `415`, and a
schema/validation failure is `422` (the `JsonBody` extractor maps each kind to its
correct status; the client message is fixed and the parser detail is logged only).

Security rules baked in: emails trimmed + lowercased (case-insensitive accounts,
also enforced at the DB via `COLLATE NOCASE`); password 8–1024 chars, email must
contain `@` and be ≤ 254 chars; login returns a **generic** 401 (with timing
equalization on user-not-found, against a dummy hash precomputed at startup).
Access JWTs are HS256 with `exp`, decoded with the algorithm pinned to HS256.
**Refresh tokens are single-use** — every `/refresh` rotates them (claimed via an
atomic conditional `UPDATE`) with **lineage-based reuse detection**: each token
records its successor, so replaying a *superseded* token (whose successor has itself
been consumed) revokes the user's whole token family, while a benign concurrent
double-submit (successor still active) is just rejected. A revoked token is never
exchanged for a new one. The cookie is `HttpOnly; SameSite=Lax; Path=/api/auth;
Secure=COOKIE_SECURE` (SameSite=Lax mitigates CSRF on `/refresh` and `/logout`).

## Card catalog API contract

Public (no auth), game-agnostic reads under `/api/games`. `{game}` is a slug like
`mtg`; an unknown game/set/card is `404`. The **card-list** endpoints (`.../cards`)
paginate with `?page` (1-based) + `?page_size` (default 60, max 200) and return
`{ data, page, page_size, total, has_more }`; `/api/games` and `/sets` return a
plain `{ data: [...] }`.

| Method & path | Returns |
|---------------|---------|
| `GET /api/games` | `{ data: Game[] }` — `Game = { id, name, publisher, data_source }` |
| `GET /api/games/{game}/status` | import status `{ status, detail, sets_imported, cards_imported, source_updated_at, finished_at }` (`status`: `idle`/`running`/`complete`/`error`) |
| `GET /api/games/{game}/sets` | `{ data: Set[] }`, newest first — `Set = { code, name, set_type, released_at, card_count, icon_svg_uri, parent_set_code, has_drops }` |
| `GET /api/games/{game}/sets/{code}` | one `Set` |
| `GET /api/games/{game}/sets/{code}/icon` | the set's SVG icon (cached image proxy) |
| `GET /api/games/{game}/sets/{code}/cards?q&page&page_size&include_related` | page of `Card` (optional `q` Scryfall-style search), by collector number. `include_related=true` spans the set's whole **group** — its top-level root plus every related sub-set (tokens/promos/decks) — grouped by set (set-code order), each set in collector order |
| `GET /api/games/{game}/sets/{code}/drops?q&page&page_size` | a drop-grouped set's cards broken into **Secret Lair drops** (Scryfall's curated drop titles), **paginated by drop** — `{ data: DropGroup[], page, page_size, total, has_more }` where `DropGroup = { slug, title, card_count, cards: Card[] }` and `total` counts drops. Drops keep Scryfall's order; within a drop, cards are by collector number. Cards not in the snapshot fall into a trailing `"Other"` group (`slug: null`). `404` if the set isn't drop-grouped (use `has_drops`); optional `q` filters cards, dropping now-empty drops |
| `GET /api/games/{game}/cards?q&page&page_size` | page of `Card` (optional `q` Scryfall-style search), by name |
| `GET /api/games/{game}/cards/{id}` | one `Card` |
| `GET /api/games/{game}/cards/{id}/image?size&face` | the card image bytes (image proxy, see below) |
| `GET /api/games/{game}/cards/{id}/prices?range` | `{ data: PricePoint[] }` — the card's price history, **oldest first** (`[]` if none in range). No `range` = the full daily series; an explicit `range` (`7d`/`30d`/`1y`/`2y`/`3y`/`all`) windows it and returns a **downsampled subset** (coarser the longer the window). Unknown `range` = `422` |
| `GET /api/games/{game}/cards/{id}/prints` | `{ data: Card[] }` — the card's **other** printings (same `oracle_id`), **newest printing first**, capped at 200 (`[]` if none, or the card has no `oracle_id`) |

`Card = { id, name, set_code, set_name, collector_number, rarity, lang, released_at,
mana_cost, cmc, type_line, oracle_text, power, toughness, loyalty,
color_identity: string[], colors: string[], layout,
prices: { usd, usd_foil, eur, tix }, has_image,
drop_name: string | null, drop_slug: string | null,
faces: { name, mana_cost, type_line, oracle_text, power, toughness, loyalty }[] }`.
The `drop_*` fields name the card's Secret Lair drop (for drop-grouped sets only;
`null` elsewhere) — see the `/sets/{code}/drops` endpoint above.

`PricePoint = { date (YYYY-MM-DD), usd, usd_foil, eur, tix }` — prices are the decimal
strings exactly as stored (any may be `null`). One row per `(card, day)` is captured on
every sync tick from the already-committed `cards` rows (`scryfall::ingest::snapshot_prices`),
so the *stored* series stays continuous even on a tick where the version-gated import is
skipped. The `?range` **downsampling** is response-shaping only: it never averages — it
keeps the **last real row per bucket** (one ~real day per week/fortnight/month as the window
grows), so every returned point is a genuine, internally-consistent snapshot and the newest
day is always included; the underlying `card_price_history` rows are untouched.

**Search syntax (`q`):** the MTG card-list endpoints parse `q` as a subset of
[Scryfall syntax](https://scryfall.com/docs/syntax) (`api/src/scryfall/search.rs`).
Bare words / `"quoted phrases"` are card-name substrings (ANDed); `!"exact name"`
is an exact match. Supported filters: `name`/`n`, `t`/`type`, `o`/`oracle`,
`m`/`mana`, `c`/`color` and `id`/`identity` (set comparison, `:` means `>=`),
`cmc`/`mv` (incl. `:even`/`:odd`), `pow`/`tou`/`loy` (numeric, incl. cross-column
like `pow>tou`), `usd`/`usdfoil`/`eur`/`tix`, `year`, `date`, `r`/`rarity`
(ordered), `s`/`set`/`e`, `st`/`settype` (the set's Scryfall `set_type`, resolved
via a game-scoped subquery on `card_sets`), `cn`/`number`, `lang`, `layout`,
`is:`/`not:` (layout/colour/mana/type-derived — incl. `permanent`/`spell`/`vanilla`),
`game`, `oracleid` — with comparison operators
`: = != > >= < <=`, boolean `and`/`or`, `-` negation, and parentheses. Filters we
don't ingest (`f:` legality, `kw:`, `a:` artist, `ft:` flavour, …) and malformed
queries return **422** `{ error }` (surfaced in the UI under the search box). All
user values bind as SeaORM parameters — never interpolated into SQL.

**HTTP caching (CDN):** the router splits routes into two cache policies via
response middleware (`handlers::cache`, wired in `main.rs`). Public catalog reads
(`/api/games/...`) are the same for everyone and change at most daily, so a
successful response carries `Cache-Control: public, max-age=300, s-maxage=3600,
stale-while-revalidate=86400` — browser- and CDN-cacheable, served stale-while-
revalidate so a cache miss never blocks on the origin. Per-user, live, and error
responses are `no-store`: all `/api/auth/*` (access tokens + `Set-Cookie`), the
import-`status` route (a live progress signal the SPA polls), and any non-2xx
(so a CDN can't pin a transient `404`/`5xx`). The image/icon routes set their own
longer `immutable` header, which the layer preserves.

**Sitemaps (crawlers):** a DB-backed XML sitemap advertises the public catalog
(`handlers::sitemap`). `GET /api/sitemap.xml` is a **sitemap index** pointing at
child sitemaps: `/api/sitemaps/pages.xml` (static + per-game routes),
`/api/sitemaps/sets.xml` (every set), and `/api/sitemaps/cards-{n}.xml` (cards,
chunked at 50 000 URLs/file since one sitemap is capped there). The `<loc>`s are the
SPA's own routes (e.g. `/cards/mtg/sets/blb`), built against `PUBLIC_SITE_URL` —
not the API's `/api/...` URLs — with a `<lastmod>` from the set/card `released_at`
or the latest sync. Served under `/api/` (the one path routed to the backend in dev
and same-origin prod); the web build's `robots.txt` points crawlers at
`/api/sitemap.xml`. Each success carries a long `Cache-Control` (a day fresh, a week
stale-while-revalidate, preserved by the cache layer); an unknown/out-of-range child
is a `no-store` `404`.

**Image proxy:** `size` ∈ `small|normal|large|png|art_crop` (default `normal`),
`face` is a 0-based face index for double-faced cards. On first request the image
is downloaded from Scryfall (HTTPS, host allow-listed to `scryfall.io`, redirects
disabled), written under `<DATA_DIR>/images/<game>/<size>/`, and served from disk
thereafter (`Cache-Control: immutable`). We never bulk-download the whole image
catalogue — images are cached lazily, on view. Set icons are cached the same way
(`.../sets/{code}/icon`, served as `image/svg+xml`).

## Collection API contract

Per-user, **authenticated** (`Authorization: Bearer <access_token>`, via `AuthUser`),
game-namespaced under `/api/collection/{game}`. Every route is in the router's
`private` group, so responses are `Cache-Control: no-store` (per-user data is never
shared-cached). Ownership is always scoped to the token's user id, so one user can
neither read nor mutate another's collection. Card ids in the path are the **external**
card id (the same id the public catalog exposes); the handler resolves it to the
internal `cards.id` before storage (so a holding survives a catalog re-import). A
missing token is `401`; an unknown game/card is `404`.

A "holding" is `(user, game, card) → { quantity, foil_quantity }`; there is no row for
a card you don't own (setting both counts to zero deletes the row), so the table holds
only owned cards. Model: `entities/collection_item.rs` (`collection_items`, unique on
`(user_id, game, card_id)`, `user_id` FK → `users` `ON DELETE CASCADE`).

| Method & path | Body | Returns |
|---------------|------|---------|
| `GET /api/collection/{game}` | — | page of `CollectionEntry`, most-recently-updated first (`?page`/`?page_size`, default 60 / max 200) — `{ data, page, page_size, total, has_more }` |
| `GET /api/collection/{game}/summary` | — | `{ unique_cards, total_cards, total_value_usd }` — distinct cards, total copies (regular + foil), and an estimated USD value (regular copies at `usd`, foil at `usd_foil`) as a 2-dp string (`null` if nothing owned is priced) |
| `GET /api/collection/{game}/cards/{id}` | — | `{ quantity, foil_quantity }` — the owned counts for one card (zeros if not owned) |
| `PUT /api/collection/{game}/cards/{id}` | `{ quantity, foil_quantity }` | `{ quantity, foil_quantity }` — sets the **absolute** counts (not a delta); both zero removes the card; a negative or oversized (`> 1_000_000`) count is `422`. Upserts on the unique key (a concurrent first-add that loses the race falls back to an update) |
| `POST /api/collection/{game}/import` | `{ provider, source, mode }` | **`202`** `ImportJob` `{ job_id, status: "queued" }` — enqueues a one-off import (runs async; poll the job below). Validated synchronously: `422` for an unknown provider / unparseable source; `503` if too many imports are queued. `provider` is `"archidekt"`; `source` is a collection URL or bare id; `mode` ∈ `overwrite`/`replace`/`merge`. Does not save a link |
| `GET /api/collection/{game}/import/jobs/{job_id}` | — | `ImportJob` `{ job_id, status, summary?, error? }` — poll an import/sync job. `status` ∈ `queued`/`running`/`complete`/`error`; `summary` (an `ImportSummary`) present on `complete`, `error` message on `error`. `404` for an unknown job or another user's |
| `GET /api/collection/{game}/source` | — | `CollectionSource` or `null` — the saved collection link for this game |
| `PUT /api/collection/{game}/source` | `{ provider, source }` | `CollectionSource` — save/upsert the link (one per user+game; validates the source resolves; does not sync) |
| `DELETE /api/collection/{game}/source` | — | `204` — forget the saved link (idempotent) |
| `POST /api/collection/{game}/sync` | — | **`202`** `ImportJob` — enqueues a re-sync from the saved link using **mirror/replace** (the worker stamps `last_synced_at` on success). `404` if no link is saved |

`CollectionEntry = { card: Card, quantity: number, foil_quantity: number }` — `card` is
the full catalog `Card` shape (reusing the catalog's `CardResponse`). The `PUT` needs
CORS `PUT` and the saved-source `DELETE` needs CORS `DELETE` (both added to the
allow-list alongside `GET`/`POST`); in dev/prod the SPA is same-origin so CORS isn't
exercised, but a direct cross-origin write needs it.

**Import/sync** (`handlers::collection` + the `collection_import` module) pulls a
collection from an external provider **server-side** (via the shared `AppState.http`
client) and reconciles it into `collection_items`. Because the provider enforces a strict
request cap (Archidekt ≈20 req/min), an import of a large collection takes minutes, so it
runs **asynchronously**: the handler validates synchronously, enqueues a background job
(`collection_import::jobs`), and returns `202` + a `job_id`; the SPA polls the job-status
route until `complete`/`error`. Imports run **one at a time** (a job waiting for the slot
reports `queued`), and a process-wide `RateLimiter` (`collection_import::rate_limit`,
20/min ⇒ one request every 3s) throttles **every** provider request across all imports.
If the provider still returns **`429`**, the fetch **backs off** the shared limiter by at
least a minute (honoring a larger `Retry-After`, capped at 5 min) so *all* imports pause,
then retries the same page — giving up (`503`) after a few attempts.
Providers are dispatched by a `Provider` enum (Archidekt today, one module per service —
Moxfield planned), each fetching + parsing to normalized `(external_card_id, foil,
quantity)` holdings; the provider-independent engine aggregates by card (`(uid, foil)` —
the same printing can span several provider rows), resolves each `external_card_id` to
`cards.external_id` (for Archidekt the `card.uid` is the Scryfall id) in chunked `IN`
lookups, skips unmatched cards, then applies the chosen `ReconcileMode` in one transaction
(atomic `ON CONFLICT` upserts + keyed deletes). `ImportSummary = { provider, mode,
total_rows, distinct_cards, matched_cards, unmatched_cards, unmatched_sample,
regular_copies, foil_copies, removed_cards }`. Import jobs live in-memory in `AppState.imports`
(lost on restart; the client just re-imports). A saved link is
`entities/collection_source.rs` (`collection_sources`, unique on `(user_id, game)`,
`user_id` FK → `users` `ON DELETE CASCADE`, stores `provider` + `external_id` +
`last_synced_at`). Archidekt is MTG-only and its API is fetched at
`https://archidekt.com/api/collection/{id}/?page={n}` (25 rows/page, capped at
`MAX_IMPORT_ROWS`); the id is validated all-digits and the URL is built host-side, so
there's no SSRF surface.

## Backend structure (`api/src/`)

```
main.rs            bootstrap: env → tracing → DB connect → migrate → build HTTP client + image cache → seed dummy catalog (SEED_DUMMY_DATA) or spawn periodic card-data import (daily) → router → serve
config.rs          Config from env (…auth vars…, DATA_DIR, SCRYFALL_USER_AGENT, SYNC_ON_STARTUP, SYNC_INTERVAL_HOURS, SEED_DUMMY_DATA); Debug redacts the secret
db.rs              SeaORM connect options with SQLite perf pragmas (WAL journal mode + cache_size=-20000), applied to every pooled connection
state.rs           AppState { db, config: Arc<Config>, dummy_password_hash, images: Arc<ImageCache>, http: reqwest::Client, imports: Arc<ImportQueue> } (cloned into handlers; `http` is the request-path provider client; `imports` is the background collection-import queue + provider rate limiter)
error.rs           AppError enum + IntoResponse → JSON { error }, correct status codes (incl. BadGateway → 502 for a failed upstream provider)
extract.rs         JsonBody<T>: JSON body extractor whose rejections are JSON, not text/plain
entities/          SeaORM entities (user, refresh_token; card = `cards`, card_set = `card_sets`, ingest_state = `ingest_state`, card_price_history = `card_price_history`, collection_item = `collection_items`, collection_source = `collection_sources`)
collection_import/ provider-agnostic collection import/sync: mod.rs (Provider enum + ReconcileMode + execute_import/aggregate/plan_reconcile/apply engine + ImportError→AppError), archidekt.rs (parse collection id from URL/id, rate-limited paginated fetch → normalized holdings), rate_limit.rs (global RateLimiter: 20 req/min spacing + back_off on 429), jobs.rs (ImportQueue: background jobs, single-slot queue, status registry, spawn_import_job). Moxfield = add a Provider variant + a module
migrator/          MigratorTrait impl + one migration per file (m<date>_<n>_<name>.rs)
auth/
  password.rs      Argon2 hash / verify (PHC strings, random salt)
  jwt.rs           access-token Claims + encode/decode (HS256, expiry in minutes)
  refresh.rs       opaque refresh-token service: issue / rotate (single-use, successor-linked reuse detection) / revoke_one / revoke_all / prune_expired
  cookie.rs        build + clear the tcglense_refresh httpOnly cookie
  extractor.rs     AuthUser: FromRequestParts that validates the Bearer access token + loads the user
catalog/           game-agnostic catalog: GAMES registry + find() + refresh_all()/seed_all() (dispatch per game to its provider / offline dummy seeder)
  images.rs        ImageCache: lazy on-disk image cache/downloader (<DATA_DIR>/images/<game>/<size>/<key>.<ext>), path-sanitised, fetch-concurrency-limited
scryfall/          MTG provider (the first game)
  model.rs         serde structs for the Scryfall card/set/bulk-data shapes we consume
  client.rs        reqwest helpers: bulk-data catalog, /sets (paginated), streaming bulk download
  ingest.rs        refresh(): stream `default_cards` line-by-line, paper-only filter, batched upserts, ingest_state bookkeeping; snapshot_prices(): daily per-card price-history capture from the committed cards rows
  search.rs        Scryfall-style query parser: lexer + recursive-descent (and/or/-/parens/quotes) → sea_orm::Condition; SearchError → 422; values always parameterised
  drops.rs         Secret Lair drop grouping: loads sld_drops.json once → (game,set)→{ordered drops, collector#→drop}; table()/has_drops()/drop_for()
  sld_drops.json   committed snapshot of Scryfall's curated Secret Lair drop titles + collector numbers (regenerate via scripts/gen-sld-drops.mjs)
  dummy.rs         seed(): deterministic offline dummy catalog (fake sets/cards, no network/images) reusing ingest's map/upsert path, plus a year of per-card seeded random-walk price history
handlers/
  auth.rs          register / login / refresh / logout / me
  cache.rs         Cache-Control response middleware: public catalog reads → CDN-cacheable; auth/status/errors → no-store
  catalog.rs       games / status / sets / set cards / set drops / all cards (search+paginate) / card detail / image proxy / price history / other printings
  collection.rs    authenticated per-user collection: list (paginate) / summary / get + set (PUT upsert, both-zero deletes) one card's owned counts; import (one-off, chosen mode) + saved-source CRUD + sync (mirror/replace) via the `collection_import` module; reuses catalog's CardResponse
  sitemap.rs       DB-backed XML sitemaps for crawlers: index + child sitemaps (pages / sets / chunked cards), <loc>s built against PUBLIC_SITE_URL
  health.rs        health
```

**Multi-TCG by design:** `cards`/`card_sets`/`ingest_state` carry a `game`
discriminator column; the catalog layer + routes are generic. Adding a TCG = add a
`Game` to `catalog::GAMES`, a provider module (like `scryfall/`), and one arm each
in `catalog::refresh_all` (live import) and `catalog::seed_all` (offline dummy seed).
On startup `main.rs` spawns `catalog::refresh_all` in the
background (gated by `SYNC_ON_STARTUP`) so the server is up immediately, then
re-runs it on a fixed interval (`SYNC_INTERVAL_HOURS`, default 24 = daily) to pick
up newer prices/sets; the import streams the bulk file with bounded memory and
**skips re-import when the provider's `updated_at` is unchanged**
(`ingest_state.source_updated_at`), so a tick with no upstream change is cheap. When
`SEED_DUMMY_DATA` is set, `main.rs` instead **awaits** `catalog::seed_all` (no
network, no images) to populate a small deterministic offline catalog and skips all
syncing — see the env-var notes below.

### Adding a backend feature (e.g. collection, prices)

1. **Entity:** add `entities/<name>.rs` (a `DeriveEntityModel`), export it from
   `entities/mod.rs` + `entities/prelude.rs`.
2. **Migration:** add `migrator/m<date>_<n>_<name>.rs` implementing `MigrationTrait`
   and register it in `migrator/mod.rs`'s `migrations()` vec. It runs on next boot.
3. **Handler:** add a module under `handlers/`, take `State(state): State<AppState>`
   for DB access and `AuthUser` to require a logged-in user.
4. **Route:** wire it in `main.rs`. Return `AppError` for failures — never
   `unwrap`/`expect`/`panic!` on a request path. Use the SeaORM query API only
   (parameterized; no hand-built SQL). For JSON request bodies use `JsonBody<T>`,
   not axum's raw `Json<T>`, so malformed-body errors stay JSON.

## Frontend structure (`web/src/`)

```
main.ts            createApp + pinia + vue-query (VueQueryPlugin) + router
App.vue            shell: top bar (brand, MainNav [Cards + Collection], theme toggle, user menu) + <RouterView>
router/index.ts    routes + global guard (requiresAuth / requiresGuest, one-time session restore)
lib/api/           typed fetch client (relative URLs, credentials:'include') + ApiError + types, split into client / auth / catalog (+ cardImageUrl) / collection (authenticated, token-passing — incl. import / saved-source CRUD / sync fns + types) fns
lib/queryClient.ts createQueryClient (defaults: staleTime 5m, retry skips 4xx) + shouldRetryQuery
lib/queries.ts     useAuthedQuery / useAuthedMutation: vue-query wrappers that run through auth.authFetch
lib/seo.ts         usePageMeta(): reactive per-route <head> — title, description, canonical, Open Graph / Twitter, JSON-LD
lib/mana.ts        parseManaText(): split card text into plain-text runs + recognised {…} mana/cost symbols (→ mana-font `ms-*` classes, unknown tokens kept literal); colorLettersToText() for color_identity pips
stores/auth.ts     Pinia store: in-memory accessToken + user, isAuthenticated, login/register/logout/refresh/fetchMe/tryRestore + authFetch helper
stores/theme.ts    Pinia store: theme (light/dark/system, default system) persisted to localStorage; reflects the resolved theme onto <html>.dark and follows the OS in system mode
components/         UserMenu (profile dropdown), ThemeToggle (light/dark/system dropdown), MainNav (top-bar primary nav: Cards → /cards and Collection → /collection dropdowns under ONE reka NavigationMenu so the swipe/fade motion plays between them; both game-dropdowns from the cached registry, Collection prompts signed-out visitors to sign in on the per-game view)
components/cards/  catalog UI: CardImage (lazy <img> via proxy + placeholder), CardTile (optional #badge overlay slot), CardGrid, SetTile, CardPagination, PriceChart (price-history line chart, public useQuery); collection UI: CollectionGrid (owned-count badges), CollectionControls (card-detail owned-count steppers, debounced+serialized save); ManaSymbols (renders card text with `{…}` mana/cost symbols as mana-font icons — mana cost, colour identity, oracle text)
components/collection/  ImportCollectionDialog (reka dialog: paste an Archidekt URL/id, pick a reconcile mode, optionally save the link; shows an import summary) — mounted on GameCollectionView alongside a "Re-sync" button
composables/       shared query hooks: useCatalog (games/sets), useCollection (useCollectionQuery/Summary/Entry + useSetCollectionEntryMutation + useCollectionSourceQuery + useImport/Save/Delete/SyncCollectionSourceMutation via useAuthed*), useCardSearch, …
views/             LoginView, RegisterView, DashboardView; catalog: CardsView (/cards), GameView (/cards/:game), SetView, CardsBrowseView, CardDetailView; collection: CollectionsView (/collection), GameCollectionView (/collection/:game)
components/ui/      shadcn-vue primitives (button, input, label, card, dropdown-menu, chart — unovis-backed)
assets/main.css    Tailwind 4 theme + CSS variables (light/dark, keyed off the .dark class)
```

The theme is applied before Vue mounts by a tiny inline script in `index.html`
(reads the same `tcglense_theme` localStorage key) so there's no flash of the wrong
theme on load; `stores/theme.ts` then owns it reactively for the rest of the session.

**SEO / social previews:** this is a CSR SPA, so each view calls `usePageMeta()`
(`lib/seo.ts`) to set a route-specific title, description, canonical URL, Open
Graph / Twitter tags, and (card pages) JSON-LD `Product` data — picked up by
JS-executing crawlers (Googlebot) and the browser tab. `index.html` carries a
baseline copy of those tags for crawlers that **don't** run JS (most social/link
unfurlers), so shared links still get a decent preview (full per-URL social
previews would need SSR/prerender — future work). `robots.txt` is generated by a
small Vite plugin (`vite.config.ts`) from `VITE_SITE_URL` (so its `Sitemap:` line
tracks the deploy origin) and served in dev/preview too; its `Sitemap:` line points
at the API's DB-backed sitemap (`/api/sitemap.xml` — see the backend "Sitemaps"
note), which covers games, sets and every card rather than just the static routes.
Auth + signed-in pages are `noindex` and `Disallow`ed.

The card-catalog pages are **public** (no `requiresAuth`) and read **public**
endpoints, so they use `useQuery` from vue-query directly (not the `useAuthedQuery`
wrapper, which routes through `authFetch`). Card images are plain `<img :src>`
pointing at the proxy URL from `cardImageUrl()` — they don't go through the fetch
client.

On first navigation the guard calls `auth.tryRestore()` once: it hits
`/api/auth/refresh` (the httpOnly cookie is sent automatically) to mint an access
token and hydrate the user, so a session survives a page reload. The access token
lives **only in memory** (no localStorage). The exported `authFetch` helper
transparently refreshes once on a 401 and retries, logging out if that still fails.

### Adding a frontend feature

- **Server state (anything fetched from the API):** add typed functions + types to
  `lib/api.ts`, then read/write it through **vue-query** so caching, dedup,
  background refresh, and invalidation come for free. Use the `lib/queries.ts`
  wrappers — `useAuthedQuery({ queryKey, queryFn })` for reads and
  `useAuthedMutation({ mutationFn, onSettled })` for writes — which run the call
  through the `auth` store's `authFetch` so access-token expiry refreshes
  transparently (don't call `authFetch` yourself for server reads). After a mutation,
  `queryClient.invalidateQueries({ queryKey: [...] })` (via `useQueryClient()`) to
  refresh dependent views (e.g. a collection write → set-completion %, valuation).
  Footgun: put reactive params **inside** `queryKey` as refs/computed
  (`['prices', productId, range]`), never `productId.value`, or refetch-on-change
  breaks. Set a per-query `staleTime` (`Infinity` for static set definitions).
- **Client state (auth/session, UI toggles, filters):** stays in **Pinia** setup
  stores under `stores/`. Auth in particular (`stores/auth.ts`) stays on Pinia — its
  single-flight refresh of the rotating cookie is hand-tuned; do **not** wrap it in
  vue-query. Rule of thumb: server state → vue-query, client state → Pinia, never
  duplicate the same datum in both.
- **Pages:** add a view under `views/` and a route in `router/index.ts`. Mark
  authenticated pages with `meta: { requiresAuth: true }`.
- **UI primitives:** prefer adding shadcn-vue components
  (`npx shadcn-vue@latest add <name>`). Hand-written ones must match the existing
  `components/ui/button/Button.vue` idiom (reka-ui `Primitive`, `cva` variants,
  `data-slot`, `cn()` from `@/lib/utils`). Note `@vueuse/core` is only a transitive
  dep — don't import it; use Vue 3.5 `defineModel` for v-model instead.

## Conventions

- **TS/Vue:** no semicolons, single quotes, 2-space indent, max 100 cols (oxfmt +
  oxlint + eslint enforce this). `<script setup lang="ts">`, Pinia setup-style
  stores, `@/` → `src/`. Run `npm run format` then `npm run lint` after editing.
- **Rust:** edition 2024; errors flow through `AppError`; `expect` only in
  `main.rs` startup. Add deps with `cargo add` so versions resolve cleanly.

## Environment variables

- **API:** `DATABASE_URL` (default `sqlite://tcglense.db?mode=rwc`), `JWT_SECRET`
  (**required**, ≥ 32 bytes, not the dev constant), `ALLOW_INSECURE_DEV_SECRET`
  (false; opt-in to the insecure compiled-in secret for local dev only),
  `ACCESS_TOKEN_EXPIRY_MINUTES` (15), `REFRESH_TOKEN_EXPIRY_DAYS` (30),
  `COOKIE_SECURE` (false), `HOST` (`127.0.0.1`), `PORT` (8080),
  `PUBLIC_SITE_URL` (`http://localhost:5173`; public SPA origin for the sitemap
  `<loc>`s — set to the real site origin in prod), `RUST_LOG` (`info`),
  `DATA_DIR` (`./data`; holds cached card images under
  `images/`), `SCRYFALL_USER_AGENT` (descriptive UA Scryfall requires),
  `SYNC_ON_STARTUP` (`true`; import card data on boot — set `false` for offline
  dev/tests), `SYNC_INTERVAL_HOURS` (`24`; re-import cadence after the startup
  import — `0` disables the periodic refresh; only applies when `SYNC_ON_STARTUP`
  is on), `SEED_DUMMY_DATA` (`false`; seed a deterministic offline dummy catalog
  instead of importing real data — **takes precedence** over `SYNC_ON_STARTUP`/
  `SYNC_INTERVAL_HOURS`, does no network sync, upsert-only so point it at a
  fresh/dedicated DB). See `api/.env.example`.
- **Web:** `VITE_API_URL` (default empty → relative `/api`, via the dev proxy).
  `VITE_SITE_URL` (public site origin, default `http://localhost:5173`) — used at
  **build time** for the absolute `Sitemap:` URL in `robots.txt`; canonical and OG
  URLs are resolved at runtime from the live origin, and the sitemap itself is
  API-served (so the API's `PUBLIC_SITE_URL` builds its `<loc>`s). Set it in
  production CI (alongside the API's matching `PUBLIC_SITE_URL`).

## Known trade-offs / future work

- **Token storage:** the refresh token is an `HttpOnly` cookie (not readable by JS)
  and the access token is held in memory only, so an XSS can't exfiltrate the
  long-lived credential. In production set `COOKIE_SECURE=true` (HTTPS) and serve
  web + API same-origin (or configure cross-origin CORS credentials).
- **No rate limiting / brute-force protection** on login yet.
- **Collection import (Archidekt):** the import/sync endpoints fetch a public
  collection **server-side** and reconcile it. Archidekt caps requests (≈20/min) and
  pages 25 rows at a time with no page-size override, so a large collection takes minutes.
  Imports therefore run **asynchronously**: the endpoint returns `202` + a job id and the
  client polls; a process-wide `RateLimiter` (20/min ⇒ one request every 3s) throttles
  every provider request across all imports, and a single-slot queue runs one import at a
  time (others report `queued`). Jobs are **in-memory** (`AppState.imports`) — lost on
  restart (the client just re-imports) and not shared across instances, so a multi-instance
  deploy would need a shared job store + a distributed rate limiter (or a dedicated worker).
  It relies on Archidekt's unofficial, undocumented API (may break on their side); a
  private/missing collection is a `404`, an empty one a `422`, and a mirror/replace that
  matches **zero** catalog cards is refused (so it can't wipe a collection against a
  misresolved/unsynced source). A saved re-sync always mirrors (replace) and stamps
  `last_synced_at`, but there's **no automatic background sync** — re-sync is
  user-triggered. Cards not in our catalog are skipped (surfaced in the summary's
  `unmatched_*`). Moxfield is planned; the `collection_import` layer is already
  provider-generic so adding it is a new `Provider` variant + module.
- **Atomic rotation:** `/refresh` claims a token via a single conditional `UPDATE`
  gated on `rows_affected`, so it's race-safe across connections. The
  revoke-then-issue pair isn't wrapped in a transaction, so a DB error mid-rotation
  degrades to a forced re-login (no security impact).
- **Concurrent refresh:** reuse detection is lineage-based (a token stores its
  successor's id) so a benign concurrent double-submit of the same token doesn't
  burn the family — only replay of a token whose successor was itself consumed does.
  The client also **single-flights** `refresh()`/`tryRestore()` so concurrent 401s
  coalesce into one rotation. Residual caveat: two browser *tabs* refreshing at the
  same instant still race (one wins, the other's request clears its cookie); fixing
  that fully needs cross-tab coordination (e.g. `BroadcastChannel`) — not done yet.
- **Refresh-token pruning:** a background task deletes rows past `expires_at` every
  6h so the table can't grow unbounded; revoked-but-unexpired rows are retained so
  reuse detection still works.
- **Gotcha — `jsonwebtoken`:** v10 needs a crypto provider feature or it panics at
  runtime; this crate pins `default-features = false, features = ["rust_crypto"]`
  (pure Rust, no C toolchain). Don't drop that when bumping it.
- **Card data import:** runs in the background on boot, streaming Scryfall's
  `default_cards` bulk file (~550 MB gz) line-by-line and upserting paper cards in
  batches (~100k rows, ~30s, bounded memory). It's idempotent and version-gated
  (`ingest_state.source_updated_at`), so reboots are cheap; a run that imports zero
  cards is recorded as `error` (not version-locked) so it retries next boot. A
  background task re-runs the import every `SYNC_INTERVAL_HOURS` (default 24 = daily)
  to pick up newer prices/sets without a restart; because it's version-gated, a tick
  with no upstream change costs just the small bulk-data catalog fetch. Set
  `SYNC_INTERVAL_HOURS=0` to keep the old startup-only behaviour. `default_cards` is
  English-or-sole-language and **paper-only**
  (digital Arena/MTGO printings filtered out); switch datasets/filters in `scryfall/`.
  The parser assumes Scryfall's one-object-per-line bulk format; per-line length
  isn't capped, so a format change to a single-line array would not be parsed safely
  (it'd hit the zero-card guard but only after buffering).
- **Dummy catalog seed:** `SEED_DUMMY_DATA=true` makes `main.rs` **await**
  `catalog::seed_all` (no network, no images) before serving, populating a small
  deterministic fake catalog (a few MTG-flavoured sets — including a parent/child
  pair — and ~95 cards with a double-faced card, non-numeric collector numbers, and a
  card reprinted across two sets sharing one `oracle_id` so the card-detail "other
  printings" view has something to show),
  plus **a year** of daily price history per card so the card-detail chart has real
  movement without a network. For offline dev, CI, and `npm run test:e2e`. It reuses the real
  `ingest::map_card`/`import_sets`/`flush_cards`/`put_state` path (so seeded rows are
  shaped exactly like imported ones) and reuses the same `(game, "default_cards")`
  `ingest_state` row, marking it `complete` with a synthetic `source_updated_at`
  (`dummy-seed-v1`) — a later real sync's version gate sees the mismatch and
  re-imports, so dummy mode never locks out real data. It is **upsert-only** (never
  deletes), so toggling it on a DB that already holds real cards leaves a real+dummy
  mix; point it at a fresh/dedicated DB (or `:memory:` in tests). The seeded price
  history is a per-card **seeded random walk** (`StdRng` seeded from the card id,
  anchored so day 0 equals the card's current price): random-looking yet byte-identical
  on a same-day reseed, but because the window ends at "today" and nothing is deleted,
  an on-disk dummy DB rebooted on a *later* calendar day re-stamps the shifted older
  dates and leaves rows past the year mark in place (harmless drift on fabricated data —
  another reason to keep dummy mode on a fresh/dedicated DB). Tests call
  `scryfall::dummy::seed` directly against an in-memory DB rather than booting the
  binary.
- **Secret Lair drop snapshot:** Scryfall breaks the Secret Lair Drop set (`sld`)
  into named "drops" (e.g. "Wild in Bloom") on its gallery page, but those curated
  titles are **not** in the bulk card API we ingest — only the page's
  collector-number groupings carry them. We capture them once into a committed
  snapshot (`scryfall/sld_drops.json`, regenerated by
  `scripts/gen-sld-drops.mjs`) and match each card to its drop by `(game, set_code,
  collector_number)` — no runtime scraping. The snapshot **goes stale**: a drop
  released after it was taken has no title, so its cards surface under a trailing
  `"Other"` group (graceful, but re-run the generator to pick up new drops). The
  snapshot is the only drop source; nothing in the DB stores drop membership (the
  `drop_*` fields and `/drops` endpoint derive it live from the embedded table), so
  no migration/re-import is needed to update it. A unit test guards that the shipped
  JSON parses and covers `sld`.
- **Image caching:** card images download lazily on first view to `<DATA_DIR>/images`
  and are served from disk after — deliberately *not* a bulk image download (that
  would be hundreds of GB and against Scryfall's guidelines). Fetches go through a
  redirect-disabled, host-allow-listed (`scryfall.io`) client with a concurrency cap
  of 8. The image route is public and card ids are enumerable, so it's open to
  scripted disk-fill / bandwidth-amplification abuse — there's no per-IP rate limit,
  cache budget, or eviction yet (same posture as the unfinished login rate-limiting).
  Set icons go through the same cache (`.../sets/{code}/icon`, `image/svg+xml`), so
  the provider is hit only once per asset rather than hotlinked on every view.
- **Gotcha — `reqwest`:** pinned `default-features = false, features = ["rustls",
  "gzip", "stream", "json"]` to use rustls (matching SeaORM's `runtime-tokio-rustls`)
  and to stream + auto-decompress the gzip bulk file. No overall request timeout on
  the client (the bulk download streams for a while); a `read_timeout` guards stalls.
- **Mana symbols (`mana-font`):** card `{…}` symbols render via the bundled `mana-font`
  icon font (`ManaSymbols.vue` + `lib/mana.ts`), so they work offline and aren't
  hotlinked from Scryfall. Two trade-offs: (1) the package's shipped `@font-face` lists
  only woff/ttf/eot/svg, so `web/src/assets/mana-font.css` adds a woff2 source loaded
  *after* the package CSS to win the cascade (browsers fetch only the ~187 KB woff2) —
  but importing the whole package CSS still makes Vite emit the shadowed legacy Mana
  formats **and** the entirely-unused MPlantin family (~3.5 MB) into `dist`; those are
  never downloaded by any browser (deploy-size only). Trimming to a generated
  symbol-only CSS (à la `sld_drops.json`) is possible future work. (2) `{HW}`/`{HR}`
  half-coloured mana (a couple of Un-set joke cards) has no single mana-font class —
  mana-font renders it via a wrapper `<span class="ms-half">`, which our one-`<i>`-per-
  symbol component doesn't emit, so those tokens fall back to literal `{HW}` text.
