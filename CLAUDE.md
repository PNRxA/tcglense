# CLAUDE.md

Guidance for working in this repository.

## What this is

**TCGLense** is a trading-card-game tracking application. The goal is to track:

- Retail and MSRP prices over time (sealed product)
- Singles prices over time
- A user's personal collection
- Set-completion progress (how much of a set you own)

The **auth foundation** (register / login / session, plus **email verification
and password reset** — transactional mail via Resend, sign-in blocked until the
address is verified) is built, plus a **card
catalog**: browse trading-card games → sets → cards, with search. Magic: The
Gathering is the first game, sourced from [Scryfall](https://scryfall.com).
**Singles price history** is also built: every sync captures each card's daily prices
into `card_price_history`, served as a per-card time series (`.../cards/{id}/prices`)
and charted on the card detail page. **Per-user collections** are also built: a
signed-in user records how many copies (regular + foil) of each card they own, per
game (MTG first, the model is game-agnostic), edited from the card detail page and
browsed at `/collection/{game}` with a value/count summary — a per-set landing (mirroring
the catalog's game → sets view, **nesting owned sub-sets under their parent set**) lets you
pick a set you own (`/collection/{game}/sets/{code}`) or view every owned card
(`/collection/{game}/cards`). The set-scoped view carries the **same view toggles as the
catalog set view**, scoped to what you own: a **by-drop** view for a Secret Lair-style set
(grouping your owned cards into Scryfall's drops) and an **include-related** toggle that
folds a set's related sub-sets (tokens/promos/decks) into one owned listing. Either card grid
can also **show "ghosts"** of the cards in scope you *don't* own — a per-view toggle
(`?ghosts=1`, default off) that swaps the owned-only list for the public catalog list with
unowned cards dimmed, so a set's gaps (and a "X of Y owned" completion count) read at a
glance and can be quick-added in place; it composes with the by-drop / include-related
scope. A collection can also be
**imported/synced from an external provider** (Archidekt and Moxfield; the layer is
provider-agnostic — one module per service): a one-off import with a chosen reconcile mode
(overwrite-matched / mirror-replace / add-merge / smart-incremental), or a saved
collection link re-synced on demand (mirror/replace, or smart if the link opted in).
The same reconcile engine also backs an **uploaded CSV export** (one-off, no
network fetch, so it reconciles synchronously) — the shape is sniffed from the header row:
an Archidekt export (needs only the Scryfall ID + Finish + Quantity columns) or a Moxfield
export (no card id at all — rows resolve by Edition + Collector Number, with Count + Foil).
The set-completion and
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

**Verify-first registration:** register creates the account and emails a
verification link, but mints **no session**; login answers `403 "email not
verified"` (only after the password matched, so it's no enumeration oracle) until
the link is used. Pre-feature accounts were grandfathered as verified by the
migration.

| Method & path | Body | Success | Notes |
|---------------|------|---------|-------|
| `POST /api/auth/register` | `{ email, password, display_name? }` | `201 { user }` (**no session**) + verification email | `409` taken · `422` invalid |
| `POST /api/auth/login` | `{ email, password }` | `200 { access_token, user }` + refresh cookie | `401 "invalid email or password"` (generic) · `403 "email not verified"` |
| `POST /api/auth/refresh` | — (refresh cookie) | `200 { access_token }` + **rotated** cookie | `401` if missing/invalid/expired/revoked (clears cookie) |
| `POST /api/auth/logout` | — (refresh cookie) | `204` (revokes token + clears cookie) | idempotent |
| `GET /api/auth/me` | — (`Authorization: Bearer <access_token>`) | `200 { user }` | `401` if missing/invalid/expired |
| `POST /api/auth/verify-email` | `{ token }` | `204` (stamps `users.email_verified_at`; no session) | `401 "invalid or expired token"` |
| `POST /api/auth/resend-verification` | `{ email }` | `204` **always** (anti-enumeration; async send, 60s cooldown) | `422` invalid email shape |
| `POST /api/auth/forgot-password` | `{ email }` | `204` **always** (anti-enumeration; async send, 60s cooldown) | `422` invalid email shape |
| `POST /api/auth/reset-password` | `{ token, password }` | `204` (re-hashes the password, **revokes every refresh token**, verifies a still-unverified email) | `401` bad token · `422` weak password (checked **before** the token is spent) |
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
**Email tokens** (verification 24h, reset 1h) mirror the refresh-token storage:
32 CSPRNG bytes hex-encoded, only the SHA-256 hex persisted (`email_tokens`, whose
`purpose` column is filtered in the claim so a verification token can't be spent
as a reset), single-use via the same atomic conditional `UPDATE`, pruned by the
same 6h background task. The emailed links point at the SPA
(`{PUBLIC_SITE_URL}/verify-email?token=…`, `/reset-password?token=…`); mail goes
out through Resend's HTTPS API on the shared client (10s per-request timeout).
With no `RESEND_API_KEY` sending is **disabled**: the message — including the
link — is logged instead, so offline dev and the test suites work with zero
network (the security-test harness swaps in a capturing mailbox).

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
| `GET /api/games/{game}/cards?q&page&page_size&name` | page of `Card` (optional `q` Scryfall-style search; optional `name` = exact-name equality filter, the quick-add "printings of this name" step), by name |
| `GET /api/games/{game}/card-names?q&limit` | `{ data: string[] }` — up to `limit` (default 10, max 25) **distinct** card names containing `q` (case-insensitive; names *starting* with `q` first, then alphabetical). `[]` for a blank/absent `q`. Powers the collection quick-add autocomplete |
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
every sync tick from the already-committed `cards` rows (`scryfall::price_history::snapshot_prices`),
so the *stored* series stays continuous even on a tick where the version-gated import is
skipped. The `?range` **downsampling** is response-shaping only: it never averages — it
keeps the **last real row per bucket** (one ~real day per week/fortnight/month as the window
grows), so every returned point is a genuine, internally-consistent snapshot and the newest
day is always included; the underlying `card_price_history` rows are untouched.

**Search syntax (`q`):** the MTG card-list endpoints parse `q` as (near-)full
[Scryfall syntax](https://scryfall.com/docs/syntax) (`api/src/scryfall/search/`).
Bare words / `"quoted phrases"` are card-name substrings (ANDed); `!"exact name"`
is an exact match; `/regex/` (on `name`/`t`/`o`/`ft`) runs a case-insensitive
regular expression via a registered SQLite `REGEXP` function (sqlx's `regexp`
feature, enabled by `with_regexp()` in `db.rs`). Supported filters: `name`/`n`,
`t`/`type`, `o`/`oracle`/`fulloracle`, `m`/`mana` (incl. `>`/`!=`; subset `<`/`<=`
still 422), `c`/`color` and `id`/`identity`/`commander` (set comparison, `:` means
`>=`, colour names + guild/shard nicknames), `produces`, `cmc`/`mv` (incl.
`:even`/`:odd`), `pow`/`tou`/`loy`/`pt`/`def` (numeric, incl. cross-column like
`pow>tou`), `usd`/`usdfoil`/`eur`/`tix`, `year`, `date`, `r`/`rarity` (ordered),
`s`/`set`/`e`, `st`/`settype` (game-scoped subquery on `card_sets`), `cn`/`number`,
`lang`, `layout`, `game`, `oracleid`, `f`/`legal`/`banned`/`restricted` (per-format
legality via `json_extract` over the stored `legalities` JSON), `kw`/`keyword`,
`a`/`artist` (+ `artists>N`), `ft`/`flavor`, `wm`/`watermark`, `border`, `frame`,
`stamp`, `has:` (flavor/watermark/indicator), `prints`/`sets`/`papersets` (printing
counts via an `oracle_id`-sibling subquery), and a broad `is:`/`not:` vocabulary
(layout/colour/mana/type-derived — incl. `permanent`/`spell`/`vanilla` — plus finish
`foil`/`nonfoil`/`etched`, print flags `reprint`/`fullart`/`textless`/`oversized`/
`promo`/`reserved`/…, and promo categories). Global **result-shaping** directives
`order:` (name/set/rarity/released/cmc/color/power/toughness/usd/eur/tix/edhrec/
artist/number), `direction:` (asc/desc) and `unique:` (cards/art/prints) are honoured
on the public catalog lists (URL `?sort`/`?dir` win over an in-query directive; the
collection lists parse-and-ignore them). Comparison operators `: = != > >= < <=`,
boolean `and`/`or`, `-` negation, and parentheses. Filters backed by datasets we
don't ingest — Tagger tags (`otag:`/`atag:`/`function:`, issue #140), `cube:` (issue
#141), and the curated `is:` land-cycle subjects — plus malformed queries return
**422** `{ error }` (surfaced in the UI under the search box). All user values bind
as SeaORM/SQL parameters — never interpolated into SQL.

**HTTP caching (CDN):** the router splits routes into two cache policies via
response middleware (`handlers::cache`, wired in `router.rs`). Public catalog reads
(`/api/games/...`) are the same for everyone and change at most daily, so a
successful response carries `Cache-Control: public, max-age=300, s-maxage=3600,
stale-while-revalidate=86400` — browser- and CDN-cacheable, served stale-while-
revalidate so a cache miss never blocks on the origin. Per-user, live, and error
responses are `no-store`: all `/api/auth/*` (access tokens + `Set-Cookie`), the
import-`status` route (a live progress signal the SPA polls), and any non-2xx
(so a CDN can't pin a transient `404`/`5xx`). The image/icon routes set their own
longer `immutable` header, which the layer preserves.

**Conditional requests (ETag / 304):** on top of the freshness policy, a second
public-router middleware (`cache::conditional_request_layer`, layered *outside*
`public_cache_layer` so it can read the `Cache-Control` that layer set) adds
**validators** so a revalidation of a stale cache entry transfers headers, not the
whole body. It hashes each cacheable-success body into a **weak `ETag`**
(`W/"<128-bit hex>"`, a SHA-256 prefix — weak because a downstream CDN may re-encode
the payload in transit) and turns a matching `If-None-Match` (RFC 9110 weak
comparison, incl. `*` and comma-separated lists) into a bodyless `304 Not Modified`
carrying the `ETag` + `Cache-Control`. It deliberately **skips** `immutable`
responses (the image/icon proxy — never revalidated within `max-age`, and hashing a
large binary would be wasteful) and `no-store` responses (errors / per-user), and
only runs for `GET` (axum serves `HEAD` off the same handler but strips the body, so
a `HEAD` carries no validator). Buffering the body to hash it is bounded by
`MAX_ETAG_BODY_BYTES` (a body of unknown or over-cap size is served un-`ETag`ged).

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
| `GET /api/collection/{game}?…&set&include_related` | — | page of `CollectionEntry`, most-recently-updated first (`?page`/`?page_size`, default 60 / max 200) — `{ data, page, page_size, total, has_more }`. Optional `?set=<code>` scopes to one set (ANDed with `q`) — the per-set collection view; with `?include_related=true` the scope spans the set's whole **group** (root + related sub-sets), the collection mirror of the catalog's `include_related` (resolved via the same `group_set_codes`) |
| `GET /api/collection/{game}/summary?set&include_related` | — | `{ unique_cards, total_cards, total_value_usd }` — distinct cards, total copies (regular + foil), and an estimated USD value (regular copies at `usd`, foil at `usd_foil`) as a 2-dp string (`null` if nothing owned is priced). Optional `?set=<code>` scopes the stats to one set; `?include_related=true` (with a set) spans the set's whole **group** (root + related sub-sets, same `group_set_codes` as the list) so the value matches the include-related browse view. Backs the scoped collection value shown next to the browse count (issue #119) |
| `GET /api/collection/{game}/sets` | — | `{ data: CollectionSet[] }`, newest set first — the sets the user owns cards in, each the catalog `Set` shape plus `owned_cards` (distinct owned) + `owned_copies` (regular + foil) + `owned_value_usd` (estimated USD value of the owned cards, `null` if none priced). Powers the collection's per-set landing (mirrors the catalog's game → sets view) |
| `GET /api/collection/{game}/sets/{code}/drops?q&page&page_size` | — | the signed-in user's **owned** cards in a drop-grouped set (e.g. Secret Lair), grouped by **Secret Lair drop** and **paginated by drop** — `{ data: CollectionDropGroup[], page, page_size, total, has_more }` where `CollectionDropGroup = { slug, title, card_count, cards: CollectionEntry[] }` and `total` counts drops. The collection mirror of the catalog's set-drops endpoint (owned cards only, each carrying its owned counts); a drop the user owns nothing in is absent, cards not in the snapshot fall into a trailing `"Other"` group. `404` if the set isn't drop-grouped (use `has_drops`); optional `q` filters, dropping now-empty drops |
| `GET /api/collection/{game}/cards/{id}` | — | `{ quantity, foil_quantity }` — the owned counts for one card (zeros if not owned) |
| `PUT /api/collection/{game}/cards/{id}` | `{ quantity, foil_quantity }` | `{ quantity, foil_quantity }` — sets the **absolute** counts (not a delta); both zero removes the card; a negative or oversized (`> 1_000_000`) count is `422`. Upserts on the unique key (a concurrent first-add that loses the race falls back to an update) |
| `POST /api/collection/{game}/owned` | `{ ids: string[] }` | `{ data: { [externalId]: { quantity, foil_quantity } } }` — batch owned counts for the given cards, **owned cards only** (unowned ids are absent, so nothing owned → `{ "data": {} }`). Blank/duplicate ids are trimmed away; **> 500 ids** is `422`. A `POST` (not a `GET` query) so a big browse page's id list can't blow the request-line length behind a proxy. Powers the owned-count badges overlaid on the public browse grids |
| `POST /api/collection/{game}/import` | `{ provider, source, mode }` | **`202`** `ImportJob` `{ job_id, status: "queued" }` — enqueues a one-off import (runs async; poll the job below). Validated synchronously: `422` for an unknown provider / unparseable source; `503` if too many imports are queued. `provider` is `"archidekt"` or `"moxfield"`; `source` is a collection URL or bare id; `mode` ∈ `overwrite`/`replace`/`merge`/`smart` (see below). Does not save a link |
| `POST /api/collection/{game}/import/csv?mode=` | raw CSV body (`text/csv`) | **`200`** `ImportSummary` — import an uploaded CSV export, sniffing the shape from the header row: **Archidekt** (a Scryfall ID column, plus Finish + Quantity) or **Moxfield** (no card id — Count + Edition + Collector Number + Foil, resolved against the catalog by set + collector number; `Proxy=True` rows are skipped). Runs **synchronously** (no upstream fetch → no job/rate-limiter): parses the CSV, reconciles per `?mode` (`overwrite`/`replace`/`merge`), returns the summary directly (its `provider` reflects the detected shape). A CSV is inherently one-off (no link is saved). Body is bounded by a route body limit (`MAX_CSV_UPLOAD_BYTES`, 16 MB) → `413` if larger; `422` for a bad mode / unreadable CSV / one missing a required column / a header matching neither shape / an empty upload |
| `GET /api/collection/{game}/import/jobs/{job_id}` | — | `ImportJob` `{ job_id, status, summary?, error? }` — poll an import/sync job. `status` ∈ `queued`/`running`/`complete`/`error`; `summary` (an `ImportSummary`) present on `complete`, `error` message on `error`. `404` for an unknown job or another user's |
| `GET /api/collection/{game}/source` | — | `CollectionSource` or `null` — the saved collection link for this game |
| `PUT /api/collection/{game}/source` | `{ provider, source, smart? }` | `CollectionSource` — save/upsert the link (one per user+game; validates the source resolves; does not sync). `smart` (default `false`) records whether re-syncs use smart (incremental) sync vs. a full mirror |
| `DELETE /api/collection/{game}/source` | — | `204` — forget the saved link (idempotent) |
| `POST /api/collection/{game}/sync` | — | **`202`** `ImportJob` — enqueues a re-sync from the saved link (the worker stamps `last_synced_at` on success). Uses **smart** sync when the saved link opted in, otherwise **mirror/replace**. `404` if no link is saved |

`CollectionEntry = { card: Card, quantity: number, foil_quantity: number }` — `card` is
the full catalog `Card` shape (reusing the shared `CardResponse`). `CollectionSet` is
the catalog `Set` shape (`code`, `name`, `released_at`, `icon_svg_uri`, `has_drops`, …)
plus `owned_cards` + `owned_copies` + `owned_value_usd` (built by
`collection::build_collection_sets`, which aggregates owned holdings per `set_code` — count,
copies, and estimated value the same way the summary does — dresses each with its `card_sets`
metadata — falling back to the card's own `set_name` when the set row is missing — and orders
newest set first). `CollectionDropGroup = { slug, title, card_count, cards: CollectionEntry[] }`
is the collection mirror of the catalog's `DropGroupResponse` (owned cards only, each
carrying its counts); the `.../sets/{code}/drops` handler reuses the shared generic
`group_into_drops` (generic over the grouped item, so it folds `(collection_item, card)`
pairs) and paginates by drop in memory. The batch/import
`POST`s, the `PUT`s, and the saved-source `DELETE` need CORS `POST`/`PUT`/`DELETE` (all in
the allow-list alongside `GET`); in dev/prod the SPA is same-origin so CORS isn't
exercised, but a direct cross-origin call needs it.

The **owned-count badges** (issue #85) reuse the same stacked-cards / sparkles chip the
collection grid shows (`components/cards/OwnedCountBadge.vue`): while signed in, the
catalog browse grids (all-cards, a set — flat or by-drop — and a card's other printings)
overlay each owned card with its total + foil counts. The web side looks up the visible
page's ids via `useOwnedCounts` (gated on auth, empty while signed out) — splitting them
into batches of ≤ 400 under the server cap (so even a big drop-grouped "Other" page never
trips the 422) and merging the results — and `CardGrid` renders the badge for any card
present in the map.

**Import/sync** (`handlers::collection` + the `collection_import` module) pulls a
collection from an external provider **server-side** (via the shared `AppState.http`
client) and reconciles it into `collection_items`. Because the provider enforces a strict
request cap (Archidekt ≈20 req/min), an import of a large collection takes minutes, so it
runs **asynchronously**: the handler validates synchronously, enqueues a background job
(`collection_import::jobs`), and returns `202` + a `job_id`; the SPA polls the job-status
route until `complete`/`error`. Imports run **one at a time** (a job waiting for the slot
reports `queued`), and a **per-provider** `RateLimiter` (`collection_import::rate_limit`'s
`ProviderLimiters`, one limiter per provider — Archidekt + Moxfield both 20/min ⇒ one
request every 3s for now, tunable independently) throttles **every** request to that
provider across all imports (so one provider's spacing/back-off never stalls another's).
If the provider still returns **`429`**, the fetch **backs off** that provider's limiter by
at least a minute (honoring a larger `Retry-After`, capped at 5 min) so all imports for
*that provider* pause, then retries the same page — giving up (`503`) after a few attempts.
Providers are dispatched by a `Provider` enum (Archidekt + Moxfield, one module per
service), each fetching + parsing to normalized `(external_card_id, foil,
quantity)` holdings; the provider-independent engine aggregates by card (`(uid, foil)` —
the same printing can span several provider rows), resolves each `external_card_id` to
`cards.external_id` (for both providers that's the Scryfall id: Archidekt's `card.uid`,
Moxfield's `card.scryfall_id`) in chunked `IN`
lookups, skips unmatched cards, then applies the chosen `ReconcileMode` in one transaction
(atomic `ON CONFLICT` upserts + keyed deletes). The **CSV upload** path
(`collection_import::csv_import` + `execute_csv_import`) is a second *source* of the very
same holdings: it sniffs the export shape from the header row and parses an **Archidekt**
export (only the Scryfall ID / Finish / Quantity columns) or a **Moxfield** export (no
card id — Count / Edition / Collector Number / Foil, plus optional Name for unmatched
labels and Proxy to skip proxies; rows pre-resolve to external ids by
`(set_code, collector_number)`, per-set chunked lookups, with unmatched rows keeping a
readable `"Name (set #num)"` placeholder that surfaces in the summary sample). Both paths
are defensive (the `csv` crate handles quoting/escaping, a leading BOM is stripped, a
non-UTF-8 body is rejected, rows are capped at `MAX_IMPORT_ROWS`, per-field length bounds,
and the finish is keyed off the shared foil rules) and yield `Vec<FetchedHolding>`, then
run the exact
same aggregate/resolve/reconcile/apply engine — but with no upstream fetch, so no rate
limiter or job, reconciling inline in the request (the handler bounds the body with a
route-scoped `DefaultBodyLimit`). `ImportSummary = { provider, mode,
total_rows, distinct_cards, matched_cards, unmatched_cards, unmatched_sample,
regular_copies, foil_copies, removed_cards, stopped_early }`. Import jobs live in-memory in
`AppState.imports` (lost on restart; the client just re-imports). A saved link is
`entities/collection_source.rs` (`collection_sources`, unique on `(user_id, game)`,
`user_id` FK → `users` `ON DELETE CASCADE`, stores `provider` + `external_id` +
`last_synced_at` + `smart`). Both providers are MTG-only. Archidekt is fetched at
`https://archidekt.com/api/collection/{id}/?page={n}` (25 rows/page, capped at
`MAX_IMPORT_ROWS`); the id is validated all-digits. Moxfield is fetched at
`https://api2.moxfield.com/v1/collections/search/{id}?pageNumber={n}&pageSize=100`
(paged on the envelope's `totalPages`, same row cap; `isProxy` rows skipped); the id is
validated against the base64url charset, and it sends the approved `MOXFIELD_USER_AGENT`
when configured (Moxfield's bot wall only serves approved clients — a `403` maps to a
clear "needs an approved User-Agent" error). Either way the URL is built host-side from
the validated id, so there's no SSRF surface.

The **smart** mode (`ReconcileMode::Smart`, issue #101) is an *incremental* mirror for
re-syncing a mostly-unchanged collection cheaply under the rate limit. It fetches the
provider collection **most-recently-updated first** (Archidekt `?orderBy=-updatedAt`;
Moxfield `sortType=lastUpdated&sortDirection=descending` —
an edit-aware order: a card whose count changed bubbles to the top even though its
row-visible created-at is old) and **stops paging once a whole page already matches
what we hold** (`fetch_holdings_smart` + the pure `smart_absorb_page`, judged per page
after the whole page is folded into the running aggregate so a card owning a
regular + foil finish isn't seen mid-aggregate). It then **overwrites each fetched card's
observed finishes** but **preserves any finish it never fetched** (its rows sit in the
unscanned tail) and **never deletes** (`reconcile_smart`), so an early stop can't zero a
foil we simply didn't page to. The trade-off: because it never fetches the whole
collection, smart only touches recently-changed cards — it will **not** remove cards
deleted upstream (a full `Replace` does). `stopped_early` reports whether the fetch
stopped at the already-synced tail vs. scanned everything. Smart is offered in the import
dialog as a mode, and on a saved link via its stored `smart` flag (the saved re-sync then
runs smart instead of mirror/replace).

## Backend structure (`api/src/`)

```
main.rs            bootstrap: env → tracing → DB connect → migrate → AppState::new → tasks::start → build_router → serve
router.rs          build_router(): every route + the shared middleware stack (CORS, cache layers, body limits) — kept out of main so the security tests drive the exact same router in-process
tasks.rs           background startup tasks: refresh- + email-token pruning (always, one 6h loop) + either the awaited offline dummy-catalog seed (SEED_DUMMY_DATA) or the spawned startup/periodic card-data sync
config.rs          Config from env (…auth vars…, DATA_DIR, SCRYFALL_USER_AGENT, MOXFIELD_USER_AGENT, RESEND_API_KEY, EMAIL_FROM, SYNC_ON_STARTUP, SYNC_INTERVAL_HOURS, SEED_DUMMY_DATA); Debug redacts the secrets
db.rs              SeaORM connect options with SQLite perf pragmas (WAL journal mode + cache_size=-20000) + a registered REGEXP function (sqlx `regexp` feature, for the search `/regex/` syntax), applied to every pooled connection
state.rs           AppState { db, config: Arc<Config>, dummy_password_hash, images: Arc<ImageCache>, http: reqwest::Client, imports: Arc<ImportQueue>, email: Arc<Emailer> } (cloned into handlers; `http` is the request-path provider client; `imports` is the background collection-import queue + per-provider rate limiters; `email` is the transactional-email sender); AppState::new is the one construction site, shared with the security-test harness
email.rs           outbound transactional email: Emailer enum (Resend via the shared client + per-request timeout / Disabled when RESEND_API_KEY unset, logging the message / test-only Capture mailbox) + the verification/reset message builders — enum dispatch, mirroring collection_import's Provider
error.rs           AppError enum + IntoResponse → JSON { error }, correct status codes (incl. Forbidden → 403 for the unverified-email login gate, BadGateway → 502 for a failed upstream provider)
extract.rs         JsonBody<T>: JSON body extractor whose rejections are JSON, not text/plain
test_support.rs    shared #[cfg(test)] fixtures: test_config(), a migrated in-memory SQLite DB, canonical card/set/user/holding rows (per-test tweaks via struct-update)
security_tests/    HTTP-level security tests driving the real build_router in-process (tower oneshot): refresh rotation/reuse, generic login failures, email verification + password reset (via the harness's capturing mailbox), cookie/CORS/caching contracts, request bodies, search, collection + import
entities/          SeaORM entities (user [incl. email_verified_at], refresh_token, email_token = `email_tokens`; card = `cards`, card_set = `card_sets`, ingest_state = `ingest_state`, card_price_history = `card_price_history`, collection_item = `collection_items`, collection_source = `collection_sources`)
collection_import/ provider-agnostic collection import/sync: mod.rs (execute_import/execute_csv_import orchestration [CSV shape dispatch + moxfield_rows_to_holdings set/number resolution], parse_source, ProviderContext, the incremental smart-fetch path [smart_absorb_page/load_local_by_external]), types.rs (Provider enum + ReconcileMode incl. Smart + ProviderSettings + FetchedHolding + ImportSummary), error.rs (ImportError → AppError), reconcile.rs (provider-independent engine: aggregate/plan_reconcile/atomic apply + reconcile_smart), archidekt.rs (parse collection id from URL/id, rate-limited paginated fetch [get_page] → normalized holdings; fetch_smart pages newest-updated-first with early stop; shared is_foil_finish + backoff_after), moxfield.rs (parse collection id from URL/id [base64url charset], paginated fetch on totalPages with the approved MOXFIELD_USER_AGENT + 403→"needs an approved User-Agent"; fetch_smart via sortType=lastUpdated; skips isProxy rows), csv_import.rs (sniff + parse an uploaded Archidekt or Moxfield CSV export → normalized rows; bounded + defensive), rate_limit.rs (per-provider RateLimiters via ProviderLimiters: 20 req/min spacing each + back_off on 429), jobs.rs (ImportQueue: background jobs, single-slot queue, status registry, provider settings, spawn_import_job). Another provider = add a Provider variant + a module
migrator/          MigratorTrait impl + one migration per file (m<date>_<n>_<name>.rs)
auth/
  password.rs      Argon2 hash / verify (PHC strings, random salt)
  jwt.rs           access-token Claims + encode/decode (HS256, expiry in minutes)
  refresh.rs       opaque refresh-token service: issue / rotate (single-use, successor-linked reuse detection) / revoke_one / revoke_all / prune_expired
  email_token.rs   single-use emailed-token service (verification 24h / password reset 1h): issue / issue_with_cooldown (60s, DB-backed) / consume (atomic claim, purpose-scoped) / prune_expired — same storage design as refresh.rs, no rotation lineage
  cookie.rs        build + clear the tcglense_refresh httpOnly cookie
  extractor.rs     AuthUser: FromRequestParts that validates the Bearer access token + loads the user
catalog/           game-agnostic catalog: GAMES registry + find() + refresh_all()/seed_all() (dispatch per game to its provider / offline dummy seeder)
  images.rs        ImageCache: lazy on-disk image cache/downloader (<DATA_DIR>/images/<game>/<size>/<key>.<ext>), path-sanitised, fetch-concurrency-limited
scryfall/          MTG provider (the first game)
  model.rs         serde structs for the Scryfall card/set/bulk-data shapes we consume
  client.rs        reqwest helpers: bulk-data catalog, /sets (paginated), streaming bulk download
  ingest.rs        refresh(): stream `default_cards` line-by-line, paper-only filter, batched upserts (update-column lists derived from the entity columns), ingest_state bookkeeping
  map.rs           pure Scryfall JSON → ActiveModel shaping, kept out of ingest so it (and its tests) stand alone
  price_history.rs snapshot_prices(): daily per-card price-history capture from the committed cards rows (also used by the dummy seeder)
  progress.rs      live terminal progress for the bulk import (tracing-indicatif; renders nothing when stderr isn't a TTY)
  search/          Scryfall-style query compiler: lexer.rs → parser.rs (recursive descent: and/or/-/parens/quotes/regex `/…/` → Node AST) → compile/ (one submodule per filter family → sea_orm::Condition; legality via json_extract, prints/sets via oracle_id-sibling subqueries) + order:/direction:/unique: directives (parse_query); error.rs (SearchError → 422); values always parameterised
  drops.rs         Secret Lair drop grouping: loads sld_drops.json once → (game,set)→{ordered drops, collector#→drop}; table()/has_drops()/drop_for()
  sld_drops.json   committed snapshot of Scryfall's curated Secret Lair drop titles + collector numbers (regenerate via scripts/gen-sld-drops.mjs)
  dummy/           seed(): deterministic offline dummy catalog (no network/images) reusing ingest's map/upsert path — catalog.rs (the fabricated sets/cards), prices.rs (the year of per-card seeded random-walk price history)
handlers/
  auth.rs          register (no session; sends the verification email) / login (403 gate on unverified email) / refresh / logout / me / verify-email / resend-verification / forgot-password / reset-password (the lookup-by-email pair answer generically + send off the request path)
  cache.rs         Cache-Control response middleware: public catalog reads → CDN-cacheable; auth/status/errors → no-store; plus conditional_request_layer (weak ETag + If-None-Match → 304) on cacheable public reads
  shared/          helpers both catalog + collection handlers use (dependencies only flow *into* shared): dto.rs (CardResponse + faces/prices — the ts-rs-exported card wire shape), pagination.rs (Page<T>/DataBody envelopes, page/query clamping), lookup.rs (game/set/card resolution + group_set_codes), sort.rs (the card-list sort/dir vocabulary), grouping.rs (generic Secret Lair drop grouping + by-drop pagination), search.rs (?q → sea_orm::Condition), valuation.rs (holdings → USD cents → 2-dp string)
  catalog/         public catalog endpoints, one file per concern: status.rs (games list + import status), sets.rs (set list/detail/icon + set cards incl. include_related + by-drop), cards.rs (all-cards list / card detail / other printings), prices.rs (price history + ?range downsampling), image.rs (image proxy); mod.rs holds the shared list params
  collection/      authenticated per-user collection, one file per concern: read.rs (owned list [optional `?set` scope + `?include_related` group span] / summary / single + batch owned counts [POST .../owned, for browse-grid badges]), sets.rs (owned-set landing tiles via build_collection_sets, each carrying owned value; owned cards by Secret Lair drop, paginated by drop), write.rs (PUT absolute owned counts — ON CONFLICT upsert, both-zero deletes), import.rs (one-off URL import + CSV upload [POST .../import/csv, synchronous, body-limited] + saved-source CRUD + sync via the `collection_import` module); mod.rs holds the shared params + wire types; reuses shared's CardResponse/`group_into_drops`/`group_set_codes`/`load_set`
  sitemap.rs       DB-backed XML sitemaps for crawlers: index + child sitemaps (pages / sets / chunked cards), <loc>s built against PUBLIC_SITE_URL
  health.rs        health
```

**Multi-TCG by design:** `cards`/`card_sets`/`ingest_state` carry a `game`
discriminator column; the catalog layer + routes are generic. Adding a TCG = add a
`Game` to `catalog::GAMES`, a provider module (like `scryfall/`), and one arm each
in `catalog::refresh_all` (live import) and `catalog::seed_all` (offline dummy seed).
On startup `tasks::start` spawns `catalog::refresh_all` in the
background (gated by `SYNC_ON_STARTUP`) so the server is up immediately, then
re-runs it on a fixed interval (`SYNC_INTERVAL_HOURS`, default 24 = daily) to pick
up newer prices/sets; the import streams the bulk file with bounded memory and
**skips re-import when the provider's `updated_at` is unchanged**
(`ingest_state.source_updated_at`), so a tick with no upstream change is cheap. When
`SEED_DUMMY_DATA` is set, `tasks::start` instead **awaits** `catalog::seed_all` (no
network, no images) to populate a small deterministic offline catalog and skips all
syncing — see the env-var notes below.

### Adding a backend feature (e.g. collection, prices)

1. **Entity:** add `entities/<name>.rs` (a `DeriveEntityModel`), export it from
   `entities/mod.rs` + `entities/prelude.rs`.
2. **Migration:** add `migrator/m<date>_<n>_<name>.rs` implementing `MigrationTrait`
   and register it in `migrator/mod.rs`'s `migrations()` vec. It runs on next boot.
3. **Handler:** add a module under `handlers/`, take `State(state): State<AppState>`
   for DB access and `AuthUser` to require a logged-in user.
4. **Route:** wire it in `router.rs`. Return `AppError` for failures — never
   `unwrap`/`expect`/`panic!` on a request path. Use the SeaORM query API only
   (parameterized; no hand-built SQL). For JSON request bodies use `JsonBody<T>`,
   not axum's raw `Json<T>`, so malformed-body errors stay JSON.

## Frontend structure (`web/src/`)

```
main.ts            createApp + pinia + vue-query (VueQueryPlugin) + router
App.vue            shell: top bar (brand, MainNav [Cards + Collection], theme toggle, user menu) + <RouterView>
router/index.ts    routes + global guard (requiresAuth / requiresGuest, one-time session restore)
lib/api/           typed fetch client (relative URLs, credentials:'include') + ApiError, split into client / auth / catalog (+ cardImageUrl) / collection + collection-import (authenticated, token-passing — incl. import / saved-source CRUD / sync fns) fns; generated/ holds the wire types (Card, CardSet, Page, ImportSummary, …) exported from the API's Rust DTOs by ts-rs — regenerate with `cargo test` from `api/` (committed; CI fails on drift), never edit by hand
lib/queryClient.ts createQueryClient (defaults: staleTime 5m, retry skips 4xx) + shouldRetryQuery
lib/queries.ts     useAuthedQuery / useAuthedMutation: vue-query wrappers that run through auth.authFetch (queries also auto-gate `enabled` on being signed in)
lib/seo.ts         usePageMeta(): reactive per-route <head> — title, description, canonical, Open Graph / Twitter, JSON-LD
lib/mana.ts        parseManaText(): split card text into plain-text runs + recognised {…} mana/cost symbols (→ mana-font `ms-*` classes, unknown tokens kept literal); colorLettersToText() for color_identity pips
lib/money.ts       formatUsd(): format the API's decimal USD strings as localized currency (shared by the collection value displays), null when unpriced
lib/searchQuery.ts pure helpers to read/edit individual filter tokens in a query string (tokenize/parse/read/remove/upsertFilter, read/setRange), non-destructive to free text + unrelated tokens; searchBuilder.ts the advanced-search builder's option lists + get/set domain layer mapping each control ↔ Scryfall tokens on top — both back AdvancedSearchPanel
lib/setGroups.ts   nest related sub-sets under their top-level parent (groupSets + filter/order helpers) — the one grouping the catalog + collection set landings share
lib/…              small pure helpers: cardPrice (tile price pick, foil fallback), cardSort (sort options ↔ API sort/dir), cardSize (grid density), ownership (owned-count labels), importSummary (import-result lines), quickAddFilter (client-side filter over a name's printings for the quick-add print picker — unit-tested matching rules), persistedRef (localStorage-backed ref)
stores/auth.ts     Pinia store: in-memory accessToken + user, isAuthenticated, login/logout/refresh/fetchMe/tryRestore + authFetch helper (deliberately NO register action — registration mints no session, so RegisterView calls the api fn directly; writing the user into the store would flip isAuthenticated)
stores/theme.ts    Pinia store: theme (light/dark/system, default system) persisted to localStorage; reflects the resolved theme onto <html>.dark and follows the OS in system mode
stores/cardSize.ts Pinia store: the persisted card-grid size preference (via lib/persistedRef)
components/         UserMenu (profile menu — a reka NavigationMenu matching MainNav's Cards/Collection triggers, its own root + viewport=false so it right-aligns; collapses to a Sign-in link when signed out), ThemeToggle (light/dark/system dropdown), MainNav (top-bar primary nav: Cards → /cards and Collection → /collection dropdowns under ONE reka NavigationMenu so the swipe/fade motion plays between them; both game-dropdowns from the cached registry, Collection prompts signed-out visitors to sign in on the per-game view), MobileNav (the same links as a small-screen hamburger dropdown), PageBreadcrumbs (page-level breadcrumb trail)
components/cards/  catalog UI: CardImage (lazy <img> via proxy + placeholder), CardTile (optional #badge overlay slot; optional `ghost` prop dims the image+text for a card you don't own), CardGrid (optional owned-count badges via `ownership` map; optional `ghostUnowned` dims every card absent from that map — the collection show-ghosts mode, issue #112), SetTile (optional `to` link override + `ownedCount`, reused for the collection's per-set landing), SetGroup + SetGroupGrid (nest a set's related sub-sets, `basePath`- + `ownedCounts`-parameterised so the collection landing reuses them), SetScopeBar (presentational include-related banner), DropSection + DropViewToggle (a by-drop group's grid + the by-drop/all switch), StickySearchBar, CardPagination, PriceChart (price-history line chart, public useQuery); AdvancedSearchPanel (point-and-click Scryfall-syntax filter-builder popover — colours/type/rarity/mana value/format/price, built from shadcn-vue Select/Toggle/ToggleGroup/NumberField in a Popover — mounted beside CardSearchBox/SearchSyntaxHint, v-model-bound to the same search-box `q` so hand-typed syntax and the builder read/write the one query and compose); ownership chips the catalog grids embed: OwnedCountBadge (shared total/foil chip overlay) + OwnedCountControl; ManaSymbols (renders card text with `{…}` mana/cost symbols as mana-font icons — mana cost, colour identity, oracle text); …
components/collection/  signed-in collection-domain UI: ImportCollectionDialog + CsvImportFields (reka dialog with two tabs: "Paste a link" — pick a provider [Archidekt or Moxfield] + collection URL/id, a reconcile mode incl. smart, optionally save the link with a smart re-sync toggle — or "Upload a CSV" — an exported Archidekt or Moxfield CSV file [the server sniffs which], with per-service export hints, reconciled synchronously; both show an import summary); CollectionSyncControls (mounts that dialog on GameCollectionView alongside a "Re-sync" button, labelled "Smart re-sync" when the saved link opted into smart); QuickAddBox + QuickAddPrintDialog + QuickAddPrintTile (the GameCollectionView quick-add flow: a name-autocomplete combobox → a print-picker dialog [filterable via lib/quickAddFilter] → add regular/foil copies in place); CollectionGrid (owned-card grid with count controls); CollectionControls (card-detail owned-count steppers, debounced+serialized save); CollectionSignInPrompt (shared signed-out prompt on the public collection pages, preserving ?redirect)
composables/       shared query hooks: useCatalog (games/sets registry + the shared catalog list queries — useSetQuery/useSetCardsQuery/useAllCardsQuery/useSetDropsQuery + CARD_PAGE_SIZE/DROP_PAGE_SIZE), useCollection (useCollectionQuery [optional set scope + include-related group span, `enabled` gate] / DropsQuery [owned cards by Secret Lair drop] / Summary [optional set scope + include-related group span, `enabled` gate] / Sets [per-set landing] / Entry + useOwnedCounts [browse-grid badges → `{ ownership, ready, fetching }`, the `ready` flag gating the ghost dimming, `fetching` gating the quick-add dialog's absolute-count seeding; opts `{ enabled, staleTime }`] + useSetCollectionEntryMutation + invalidateCollectionData), useCollectionImport (the import/sync flow: useCollectionSourceQuery, useSyncCollectionSourceMutation, import-job polling, and the dialog's orchestration hook), useQuickAdd (quick-add server state: useCardNameSuggestions [distinct-name autocomplete over `/card-names`] + useCardPrintingsByName [every printing of an exact name via the all-cards `name` filter]), useSetGrouping (related-set grouping + hasDrops + scope-nav, `basePath`-parameterised so the collection reuses it), useCardSearch, and small focused ones (useAuthSubmit, useCardBackLink, useClampPage, useImageLoad, useOwnedCountEditor)
views/             HomeView (/, the public landing page), LoginView ("Forgot password?" link + the 403 email-not-verified branch [via useAuthSubmit's errorStatus] offering a resend), RegisterView (no auto-signin — success swaps the form for a "check your email" card with a resend button), ProfileView; email flows (public routes — no requiresGuest, so a signed-in user clicking an emailed link still lands: VerifyEmailView (/verify-email?token=…, consumes on mount), ForgotPasswordView (/forgot-password, always-generic success), ResetPasswordView (/reset-password?token=…); all noindex + robots-Disallowed); catalog: CardsView (/cards), GameView (/cards/:game), SetView, CardsBrowseView, CardDetailView; collection: CollectionsView (/collection), GameCollectionView (/collection/:game — the per-set landing: owned-set tiles + "All cards", mirrors GameView incl. nesting owned sub-sets under their parent via SetGroup; each tile shows owned count + value; the header carries the summary stats, the quick-add box [QuickAddBox], and the import/re-sync controls [CollectionSyncControls]), CollectionBrowseView (/collection/:game/cards + /collection/:game/sets/:code — the owned-card grids, all or set-scoped; the header count line also carries the scope's owned **value** [issue #119, from the summary, tracking the set/group scope], the set-scoped view carries the catalog set view's by-drop + include-related toggles [reusing useSetGrouping (basePath `/collection`) + SetScopeBar over what the user owns], and any view offers a "Show ghosts" toggle switching the data source to the public catalog list [owned + unowned, catalog sorts / by-drop] with unowned cards dimmed — the two compose into an {owned, ghost} × {flat, by-drop} matrix)
components/ui/      shadcn-vue primitives (button, input, label, card, dropdown-menu, select, toggle, toggle-group, number-field, popover, dialog, navigation-menu, tooltip, chart — unovis-backed)
test/fixtures.ts   shared unit-test fixtures (makeCardSet) so a spec only spells out what it asserts on
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
previews would need SSR/prerender — future work). Every non-card page defaults its
`og:image`/`twitter:image` to a branded 1200×630 banner (`DEFAULT_OG_IMAGE` →
`web/public/og-image.png`, a committed asset regenerated by
`web/scripts/gen-og-image.mjs` via Playwright), upgrading those pages to a
`summary_large_image` card; card pages still override it with the card art. The
baseline copy in `index.html` is a root-relative path that the `seoDiscoveryFiles`
Vite plugin rewrites to an absolute `VITE_SITE_URL` (non-JS unfurlers need an
absolute image URL). `robots.txt` is generated by a
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

- **Server state (anything fetched from the API):** add typed fetch functions under
  `lib/api/` — the wire types come from the Rust DTOs: derive `ts_rs::TS` on the
  API struct and run `cargo test` from `api/` to regenerate `lib/api/generated/`
  (don't hand-write a twin type). Then read/write it through **vue-query** so
  caching, dedup, background refresh, and invalidation come for free. Use the
  `lib/queries.ts`
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
  `MOXFIELD_USER_AGENT` (unset; the Moxfield-approved UA for collection URL imports —
  email support@moxfield.com to get one approved, and treat it as a secret),
  `RESEND_API_KEY` (unset; the Resend API key for verification/reset email — a
  secret; unset = email sending disabled, messages logged instead), `EMAIL_FROM`
  (`TCGLense <onboarding@resend.dev>`; the outbound From address — Resend's shared
  onboarding sender only delivers to the account owner, so set a verified-domain
  sender in prod),
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
- **No rate limiting / brute-force protection** on login yet. The email endpoints
  have a narrow DB-backed guard — a 60s per-user cooldown on issuing
  verification/reset tokens (`issue_with_cooldown`) — but no per-IP limiting;
  registration itself is also unthrottled, so a bot burst could still send many
  first-registration emails (a complaint-rate risk with the email provider).
  Captcha + per-IP rate limiting on `/register` is future work.
- **Transactional email (Resend):** sends ride the shared reqwest client with a
  10s per-request timeout; there is no retry/queue — a failed registration send is
  logged and the user recovers via resend-verification, and the anti-enumeration
  endpoints (resend/forgot) **spawn** their sends off the request path and swallow
  failures (a surfaced 502 would only fire for existing accounts). Disabled mode
  (no `RESEND_API_KEY`) logs the full message including the tokened link — by
  design for dev (the only way to complete the flow offline), so don't run prod
  without a key. `onboarding@resend.dev` (the default From) only delivers to the
  Resend account owner's address; production needs a verified domain + `EMAIL_FROM`.
- **Collection import (Archidekt / Moxfield):** the import/sync endpoints fetch a public
  collection **server-side** and reconcile it. Archidekt caps requests (≈20/min) and
  pages 25 rows at a time with no page-size override, so a large collection takes minutes.
  Imports therefore run **asynchronously**: the endpoint returns `202` + a job id and the
  client polls; a **per-provider** `RateLimiter` (`ProviderLimiters` — Archidekt + Moxfield
  each 20/min ⇒ one request every 3s for now, set independently) throttles every request to
  that provider across all imports, and a single-slot queue runs one import at a
  time (others report `queued`). Jobs are **in-memory** (`AppState.imports`) — lost on
  restart (the client just re-imports) and not shared across instances, so a multi-instance
  deploy would need a shared job store + a distributed rate limiter (or a dedicated worker).
  Both providers' APIs are unofficial and undocumented (may break on their side); a
  private/missing collection is a `404`, an empty one a `422`, and a mirror/replace that
  matches **zero** catalog cards is refused (so it can't wipe a collection against a
  misresolved/unsynced source). A saved re-sync mirrors (replace) — or, when the link
  opted into **smart** sync, runs the incremental smart path — and stamps
  `last_synced_at`, but there's **no automatic background sync** — re-sync is
  user-triggered. Cards not in our catalog are skipped (surfaced in the summary's
  `unmatched_*`). The layer is provider-generic: another service is a new `Provider`
  variant + module.
- **Moxfield link import is temporarily disabled:** because we don't yet have an approved
  User-Agent (below), Moxfield's *live* import — the one-off link import and saved-link
  re-sync — is turned off for now. `Provider::network_import_enabled()` is the single
  source of truth (returns `false` for Moxfield); the import handlers reject a Moxfield
  URL import / saved-link save / re-sync with a `422` pointing the user at the CSV upload,
  and the web import dialog shows Moxfield in the link picker but greys it out. Moxfield's
  **CSV upload** needs no network and is unaffected — it's the supported way to import a
  Moxfield collection meanwhile. Re-enable by flipping that method to `true` (and dropping
  the `disabled` flag on the web picker's Moxfield entry) once the approved UA is in place.
- **Moxfield URL import needs an approved User-Agent:** since late 2024 Moxfield fronts
  `api2.moxfield.com` with bot protection that only serves allow-listed clients; they
  approve a specific User-Agent string on request (email support@moxfield.com — treat the
  granted string as a credential). Set it as `MOXFIELD_USER_AGENT`; without one a URL
  import may be rejected (`403` → a clear "needs an approved User-Agent" 502, pointing
  the user at the CSV upload, which needs no network) — or **tarpitted** (observed live:
  a page dripped over ~7 minutes, defeating per-read timeouts), which is why each page
  fetch carries a whole-request 60s deadline so a tarpitted import fails instead of
  monopolising the single import slot for hours. Moxfield pages 100 rows at a time
  (`/v1/collections/search/{id}`, paged on `totalPages`), so URL imports are much faster
  than Archidekt's 25-row pages; smart sync uses `sortType=lastUpdated`. `isProxy` rows
  are skipped. Binder URLs (`/binders/…`) are a different endpoint and are rejected —
  only collection URLs import for now.
- **Smart (incremental) sync (issue #101):** smart trades completeness for speed — it
  pages newest-updated-first and stops at the first already-synced page, so it only
  updates recently-changed cards and **never removes cards deleted upstream** (run a full
  mirror/replace for that). Two residual edges, both benign and documented in
  the `collection_import` module: (1) it relies on Archidekt's `?orderBy=-updatedAt` truly
  reflecting edit time, and on pagination staying stable mid-fetch — a collection edited
  *during* a sync could shift rows across the page boundary; (2) a card whose *same
  finish* is split across several provider rows (different condition/language/tags) where
  the recently-edited row's partial aggregate happens to equal the stale local count can
  be under-counted, since the older sibling rows sit in the unscanned tail. Both resolve
  on the next full mirror/replace, which is always authoritative.
- **Collection CSV upload:** `POST .../import/csv` reconciles an uploaded CSV
  export with **no network fetch**, so — unlike the URL import — it runs **synchronously**
  in the request (no job, no rate limiter) and needs no saved link (a CSV has nowhere to
  re-sync from). The shape is sniffed from the header row: an id column means
  **Archidekt** (Scryfall ID / Finish / Quantity — checked first, since Archidekt's
  quantity column also accepts a "Count" spelling); otherwise Count + Edition + Collector
  Number means **Moxfield**, whose rows carry no card id and pre-resolve to catalog cards
  by `(set_code, collector_number)` (exact match on the trimmed number, set code
  lowercased; validated 1058/1058 against a real export). The upload is untrusted, so
  it's defended in depth: a route-scoped
  `DefaultBodyLimit` (`MAX_CSV_UPLOAD_BYTES`, 16 MB → `413` above it), a row cap
  (`MAX_IMPORT_ROWS`), UTF-8-only parsing (a binary body is a `422`, not a partial import),
  a leading-BOM strip, per-field bounds (ids / set codes / collector numbers are
  length-capped + non-blank; quantities are parsed + clamped), and values that only ever
  bind as SQL parameters (an
  unknown one is skipped, never trusted). The same zero-match `Replace` guard applies, so an
  empty/garbage upload can't wipe a collection. The
  16 MB cap is generous for either export (Moxfield's full export is ~100 KB per 1000
  rows) but can reject a huge *all-columns* Archidekt export — its user is told to export
  only the three needed columns.
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
- **Dummy catalog seed:** `SEED_DUMMY_DATA=true` makes `tasks::start` **await**
  `catalog::seed_all` (no network, no images) before serving, populating a small
  deterministic fake catalog (a few MTG-flavoured sets — including a parent/child
  pair — and ~95 cards with a double-faced card, non-numeric collector numbers, and a
  card reprinted across two sets sharing one `oracle_id` so the card-detail "other
  printings" view has something to show),
  plus **a year** of daily price history per card so the card-detail chart has real
  movement without a network. For offline dev, CI, and `npm run test:e2e`. It reuses the real
  `map::map_card`/`ingest::import_sets`/`flush_cards`/`put_state` path (so seeded rows are
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
