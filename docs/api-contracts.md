# HTTP API contracts

The full HTTP JSON API reference for the TCGLense backend (`api/`): auth, the public
card catalog, sealed products, per-user collection (incl. import/sync/CSV), and the wish
list. `CLAUDE.md` is the always-loaded core (a short pointer lives there); **this file is
the on-demand detail** — read it when you're adding or changing an endpoint, wiring a new
client call, or need the exact wire shape, status codes, params, or ordering of a route.

Base URL `http://localhost:8080`, all routes under `/api`. Every response — success or
error — is JSON. Errors are always `{ "error": string }`. A malformed JSON body is `400`,
a missing/wrong `Content-Type` is `415`, and a schema/validation failure is `422` (the
`JsonBody` extractor maps each kind to its correct status; the client message is fixed and
the parser detail is logged only).

## Auth API contract

`User` shape: `{ id: number, email: string, display_name: string | null, created_at: string (RFC3339 UTC) }`

**Two-token model:** a short-lived **access token** (JWT, 15 min, returned as
`access_token`, kept in memory on the client) plus a long-lived **refresh token**
(opaque, 30 days, delivered only as the `tcglense_refresh` httpOnly cookie, stored
server-side as a SHA-256 hash).

**Email-first registration (issue #176):** register takes only `{ email }` and always
answers a generic `200 { completion_token: null }` — a new, a pending, and an
already-registered address are indistinguishable (no enumeration oracle; the pre-#176
duplicate-email `409` is gone). A new address creates a **pending** account
(`users.password_hash` is now nullable — `NULL` = pending, cannot sign in) and emails a
**completion link** (`{PUBLIC_SITE_URL}/complete-registration?token=…`, a
`complete_registration` email-token, 24h, single-use, issued through the 60s cooldown,
sent fire-and-forget off the request path); re-POSTing register re-sends it for a
pending address (the cooldown collapses bursts) and sends nothing for an activated one.
`POST /api/auth/complete-registration` then consumes the token, sets the first password
(+ optional display name), stamps the email verified (using the link proves mailbox
ownership), and **signs the user in** (`access_token` + refresh cookie) — the two-step
account creation. The password rules are checked **before** the token is spent (a weak
password doesn't burn the single-use link), and the token is refused (`401`) once the
account already has a password, so a completion link can never double as a password
reset. Accounts created before #176 keep working unchanged: they already hold a
password (the nullable-`password_hash` migration touches no existing rows), and any
still-unverified ones are the accounts login's `403` gate and `resend-verification`
continue to serve (accounts predating email verification itself were grandfathered as
verified by *that* feature's migration).

**No-email dev bypass:** when **no email provider is configured** (no
`RESEND_API_KEY` — `Emailer::is_enabled()` is false), the completion link can't be
delivered, so register hands it back instead: the response's `completion_token` carries
the token (its **only** non-null case), and the SPA drives straight to the set-password
page — the offline dev/CI/e2e journey stays completable through the real two-step UI.
Login's unverified-`403` gate is likewise skipped in this posture. This is the dev/CI
posture (the e2e suite runs this way); the test suites use a mail *sink* (which counts
as enabled) so `completion_token` stays null and they exercise the real email-first
flow end to end.

| Method & path | Body | Success | Notes |
|---------------|------|---------|-------|
| `POST /api/auth/register` | `{ email }` | `200 { completion_token }` — `completion_token` is **always `null`** (the link is emailed) unless email is disabled (dev bypass: the token is returned so the SPA can drive the set-password step) | generic (a new, pending, or already-registered address are identical — no `409`) · `422` invalid email · `403` when `SIGNUPS_ENABLED=false` (message from `SIGNUPS_DISABLED_MESSAGE` or generic; checked **before** the CAPTCHA — see `GET /api/config`) |
| `POST /api/auth/complete-registration` | `{ token, password, display_name? }` | `200 { access_token, user }` + refresh cookie — finishes registration: sets the first password, verifies the email, and signs in | `401 "invalid or expired token"` (also once the account already has a password) · `422` weak password (checked **before** the token is spent) · `403` when `SIGNUPS_ENABLED=false` (so a link minted before signups closed can't finalise a new account) |
| `POST /api/auth/login` | `{ email, password }` | `200 { access_token, user }` + refresh cookie | `401 "invalid email or password"` (generic — incl. a pending password-less account, same dummy-hash timing) · `403 "email not verified"` (skipped when email is disabled; now only reachable by grandfathered accounts) |
| `POST /api/auth/refresh` | — (refresh cookie) | `200 { access_token }` + **rotated** cookie | `401` if missing/invalid/expired/revoked (clears cookie) |
| `POST /api/auth/logout` | — (refresh cookie) | `204` (revokes token + clears cookie) | idempotent |
| `GET /api/auth/me` | — (`Authorization: Bearer <access_token>`) | `200 { user }` | `401` if missing/invalid/expired |
| `POST /api/auth/verify-email` | `{ token }` | `204` (stamps `users.email_verified_at`; no session) | `401 "invalid or expired token"` |
| `POST /api/auth/resend-verification` | `{ email }` | `204` **always** (anti-enumeration; async send, 60s cooldown; only re-sends for a grandfathered password-bearing unverified account — a pending registration re-sends via `register`) | `422` invalid email shape |
| `POST /api/auth/forgot-password` | `{ email }` | `204` **always** (anti-enumeration; async send, 60s cooldown) | `422` invalid email shape |
| `POST /api/auth/reset-password` | `{ token, password }` | `204` (re-hashes the password, **revokes every refresh token**, verifies a still-unverified email — so forgot/reset also activates a pending password-less account) | `401` bad token · `422` weak password (checked **before** the token is spent) · `403` when `SIGNUPS_ENABLED=false` **and** the account is still pending (password-less) — this reset-activation is the same new-account creation the signup gate refuses; a genuine reset for an already-active account still works |
| `GET /api/health` | — | `200 { status: "ok" }` | — |
| `GET /api/config` | — | `200 { turnstile_site_key: string \| null, signups_enabled: bool, signups_disabled_message: string \| null }` — public runtime config the SPA reads before rendering the auth forms; `signups_disabled_message` is non-null only when `signups_enabled` is `false`; `no-store` | — |

**Anti-abuse (all seven auth mutation endpoints above):** each request body may carry
a `captcha_token` (Cloudflare Turnstile). When `TURNSTILE_SECRET_KEY` is set the
token is **required** — a missing/rejected one is `400 "captcha …"` (deliberately
not 401/403, so it never collides with login's `403`), verified **before** any
account lookup so it leaks nothing. When the key is unset, CAPTCHA is disabled and
the field is ignored (dev/tests). The paired **public** site key
(`TURNSTILE_SITE_KEY`, set together with the secret) is served to the SPA via
`GET /api/config`, so the browser renders the widget with a runtime-supplied key —
the published web bundle needs no rebuild. Independently, a **per-IP rate limiter** guards
these endpoints (login/register/email-send/token classes, each its own quota);
over-limit is `429` + `Retry-After`. The client IP is the socket peer by default,
or the `X-Forwarded-For`/`Forwarded` client when `TRUST_PROXY_HEADERS=true` (set
only behind a trusted proxy). `refresh`/`logout`/`me` are neither CAPTCHA-gated nor
*per-IP* rate-limited (legitimate high-frequency session ops); `me`, being an
authenticated (bearer) route, does still carry the generous **per-user** general
limit (~300/min) — see the per-user rate-limiting note in `docs/tradeoffs.md`.

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
**Email tokens** (registration-completion 24h, verification 24h, reset 1h) mirror the
refresh-token storage:
32 CSPRNG bytes hex-encoded, only the SHA-256 hex persisted (`email_tokens`, whose
`purpose` column is filtered in the claim so a completion token can't be spent
as a reset — nor vice versa), single-use via the same atomic conditional `UPDATE`,
pruned by the same 6h background task. The emailed links point at the SPA
(`{PUBLIC_SITE_URL}/complete-registration?token=…`, `/verify-email?token=…`,
`/reset-password?token=…`); mail goes
out through Resend's HTTPS API on the shared client (10s per-request timeout).
With no `RESEND_API_KEY` sending is **disabled**: the message — including the
link — is logged instead, so offline dev and the test suites work with zero
network (the security-test harness swaps in a capturing mailbox).

## Public API & API keys (issue #284)

TCGLense exposes a **public HTTP API**. Two surfaces:

- **Anonymous, read-only** — the whole card catalog and sealed-product data
  (`/api/games/...`, documented in the two sections below) needs no
  authentication. These are the same CDN-cacheable reads the SPA uses.
- **Per-user, key-authenticated** — a signed-in user mints an **API key** to
  reach *their own* collection + wish list programmatically (the `/api/collection/*`
  and `/api/wishlist/*` routes).

**Interactive docs:** an OpenAPI 3.1 document is served at `GET /api/openapi.json`
(machine-readable — import into Postman/Insomnia/codegen; a public, CDN-cacheable read
in the public router group). The SPA renders it as an interactive Scalar "try it out"
reference at the `/docs` route (`DocsView.vue`, `@scalar/api-reference`) — a branded
page linked from the homepage, top nav, and footer, and listed in the sitemap. Each
operation carries a short `summary` (the sidebar title) plus the fuller `description`;
keep both when adding an endpoint.

**How a key authenticates.** A key is presented exactly like an access token —
`Authorization: Bearer tcgl_<hex>` — and rides the **same** collection/wish-list
endpoints (no separate route surface). The `AuthUser` extractor branches on the
`tcgl_` label: a `tcgl_`-prefixed credential is resolved against the `api_keys`
table (one indexed lookup on the SHA-256 hash) to the owning user; anything else
is decoded as a JWT. So every existing per-user endpoint accepts either credential
with no per-handler change.

**Scopes.** A key is `read` (GET only) or `read_write`. Enforcement is at the
extractor: read endpoints use `AuthUser` (accepts a session or **any** valid key);
mutating endpoints use `WritableUser` (a session or a `read_write` key — a
**read-only key is `403 Forbidden`**, since the credential is real but lacks the
scope). The batch-count `POST`s (`/owned`, `/counts`) are reads, so a read-only key
may call them. Note a bad/expired/revoked key is `401` (the credential itself is
invalid), while a valid read-only key on a write is `403` (valid but unauthorized).

**Key management** (`/api/auth/api-keys`) is **session-only** — the `SessionUser`
extractor rejects an API-key credential with `403`, so a leaked key can neither
mint more keys nor enumerate/revoke its siblings.

| Method & path | Body | Success | Notes |
|---------------|------|---------|-------|
| `POST /api/auth/api-keys` | `{ name, scope, expires_in_days? }` | `201 CreatedApiKey` `{ id, name, scope, key, key_prefix, created_at, expires_at }` — `key` is the **plaintext**, returned **once** (never again) | `scope` ∈ `read`/`read_write` (else `422`); `expires_in_days` optional (omit/null = never; `0` or `> 3650` = `422`); a blank/over-100-char `name` = `422`; exceeding the per-user cap (**25** active keys) = `409`. Session-only (an API-key credential = `403`) |
| `GET /api/auth/api-keys` | — | `200 ApiKeyList` `{ data: ApiKeyInfo[] }` — active keys, newest first; **metadata only** (never the plaintext or hash) | `ApiKeyInfo = { id, name, scope, key_prefix, created_at, last_used_at, expires_at }`. Session-only |
| `DELETE /api/auth/api-keys/{id}` | — | `204` — soft-revoke (idempotent for an already-revoked key) | `404` if the key doesn't exist or belongs to another user (ids don't leak across accounts). Session-only |

**Storage & lifecycle** mirror the refresh/email-token design (`auth::api_key`):
`tcgl_` + 32 CSPRNG bytes hex-encoded, only the **SHA-256 hex** persisted (fast hash
is correct for a high-entropy secret — not argon2, which is for passwords), the
plaintext returned once and never logged; `key_prefix` (`tcgl_<8 hex>`) is stored so
the UI can identify a key after the secret is gone. `last_used_at` is a best-effort,
throttled (≤ once/60s) stamp; revocation is a `revoked_at` soft delete (audit trail,
an in-flight request sees it); `expires_at` is optional. Dead keys (expired or
revoked) are pruned by the 6h maintenance loop. Model: `entities/api_key.rs`
(`api_keys`, unique on `token_hash`, `user_id` FK → `users` `ON DELETE CASCADE`).

**Rate limiting.** Key-authenticated traffic is throttled under the **same per-user
quota** as session traffic: the per-user limiter (`ratelimit::user_rate_limit`) tries
the JWT fast-path first and, on a `tcgl_` token, resolves the key to its user id (one
indexed lookup) so keyed requests can't bypass the cap. Key *creation* rides the
generous `general` per-user quota (session-authed).

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
| `GET /api/games/{game}/sets` | `{ data: Set[] }`, newest first — `Set = { code, name, set_type, released_at, card_count, icon_svg_uri, parent_set_code, has_drops, has_subtypes }`. `has_subtypes` (data-derived) flags a set with special-treatment cards, browsable by sub-type — see the `/subtypes` endpoint below |
| `GET /api/games/{game}/sets/{code}` | one `Set` |
| `GET /api/games/{game}/sets/{code}/icon` | the set's SVG icon (cached image proxy) |
| `GET /api/games/{game}/sets/{code}/cards?q&page&page_size&include_related` | page of `Card` (optional `q` Scryfall-style search), by collector number. `include_related=true` spans the set's whole **group** — its top-level root plus every related sub-set (tokens/promos/decks) — grouped by set (set-code order), each set in collector order |
| `GET /api/games/{game}/sets/{code}/drops?q&page&page_size` | a drop-grouped set's cards broken into **Secret Lair drops** (Scryfall's curated drop titles), **paginated by drop** — `{ data: DropGroup[], page, page_size, total, has_more }` where `DropGroup = { slug, title, card_count, cards: Card[] }` and `total` counts drops. Drops keep Scryfall's order; within a drop, cards are by collector number. Cards not in the snapshot fall into a trailing `"Other"` group (`slug: null`). `404` if the set isn't drop-grouped (use `has_drops`); optional `q` filters cards, dropping now-empty drops |
| `GET /api/games/{game}/sets/{code}/subtypes?q&page&page_size` | a set's cards grouped by **sub-type** (card treatment: Borderless, Showcase, Extended Art, Full Art, …), **paginated by sub-type** — `{ data: SubtypeGroup[], page, page_size, total, has_more }` where `SubtypeGroup = { slug, title, card_count, cards: Card[] }` and `total` counts sub-types. The sub-type is **derived** from the card's print attributes (see `crate::scryfall::subtypes`); every card classifies, so `Normal` heads the list, then treatments. Unlike `/drops` this never `404`s (any set groups — one `Normal` group if plain; the SPA gates the view on `has_subtypes`); optional `q` filters cards, dropping now-empty sub-types |
| `GET /api/games/{game}/cards?q&page&page_size&name` | page of `Card` (optional `q` Scryfall-style search; optional `name` = exact-name equality filter, the quick-add "printings of this name" step), by name |
| `GET /api/games/{game}/card-names?q&limit` | `{ data: string[] }` — up to `limit` (default 10, max 25) **distinct** card names containing `q` (case-insensitive; names *starting* with `q` first, then alphabetical). `[]` for a blank/absent `q`. Powers the collection quick-add autocomplete |
| `GET /api/games/{game}/cards/{id}` | one `Card` |
| `GET /api/games/{game}/cards/{id}/image?size&face` | the card image bytes (image proxy, see below) |
| `GET /api/games/{game}/cards/{id}/prices?range` | `{ data: PricePoint[] }` — the card's price history, **oldest first** (`[]` if none in range). No `range` = the full daily series; an explicit `range` (`7d`/`30d`/`1y`/`2y`/`3y`/`all`) windows it and returns a **downsampled subset** (coarser the longer the window). Unknown `range` = `422` |
| `GET /api/games/{game}/cards/{id}/prints` | `{ data: Card[] }` — the card's **other** printings (same `oracle_id`), **newest printing first**, capped at 200 (`[]` if none, or the card has no `oracle_id`) |
| `GET /api/games/{game}/cards/{id}/sealed` | `{ data: SealedProductRef[] }` — the **sealed products** this card is found in / can be pulled from, sourced from MTGJSON (see `crate::mtgjson`). See the **Sealed products** section below for `SealedProductRef` / `Product` shapes. `membership` is `"contains"` (definitely in — decks/promos/Secret Lair), `"booster"` (can be pulled from a booster sheet), or `"variable"` (may be in a randomized product) — the "found in / can be in / may be in" split. Ordered `contains` → `booster` → `variable`, then by product name; `[]` if in none |

`Card = { id, name, set_code, set_name, collector_number, rarity, lang, released_at,
mana_cost, cmc, type_line, oracle_text, power, toughness, loyalty,
color_identity: string[], colors: string[], layout,
prices: { usd, usd_foil, eur, tix }, has_image,
drop_name: string | null, drop_slug: string | null, secret_lair_bonus: boolean,
faces: { name, mana_cost, type_line, oracle_text, power, toughness, loyalty }[] }`.
The `drop_*` fields name the card's Secret Lair drop (for drop-grouped sets only;
`null` elsewhere) — see the `/sets/{code}/drops` endpoint above. `secret_lair_bonus`
is `true` for a Secret Lair **chase/bonus** card (Scryfall's `sldbonus` promo type) —
the optional card given with a qualifying drop purchase, which has no sealed product of
its own, so the SPA marks it and links to its drop rather than a "found in" section.

**Visual scanner (authed).** `POST /api/games/{game}/scan` — identify a photographed
card from its perceptual hash. Body `{ fingerprint: number[], top_k?: number }` where
`fingerprint` is exactly **32 bytes** (the client-computed 256-bit pHash of the cropped
card — only this small, non-reversible vector is uploaded, never the image). Returns
`{ data: ScanMatch[] }`, `ScanMatch = { card: Card, distance }`, nearest first
(`distance` = Hamming distance, 0..256; smaller is a closer visual match). Empty `data`
= no card within the confidence radius (`FINGERPRINT_MAX_DISTANCE`). Auth-gated
(`Authorization: Bearer …`) and `no-store`; `422` if the fingerprint isn't 32 bytes;
`404` if this instance has no fingerprint index built/imported yet (distinct from an
empty-`data` "no match"). Matching is an in-memory Hamming scan — no per-request DB
work. See `docs/tradeoffs.md` → *Visual card scanner*.

`PricePoint = { date (YYYY-MM-DD), usd, usd_foil, eur, tix }` — prices are the decimal
strings exactly as stored (any may be `null`). One row per `(card, day)` is captured on
every sync tick from the already-committed `cards` rows (`scryfall::price_history::snapshot_prices`),
so the *stored* series stays continuous even on a tick where the version-gated import is
skipped. The `?range` **downsampling** is response-shaping only: it never averages — it
keeps the **last real row per bucket** (one ~real day per week/fortnight/month as the window
grows), so every returned point is a genuine, internally-consistent snapshot and the newest
day is always included; the underlying `card_price_history` rows are untouched. The read
is a single index range scan over `card_price_history` (the unique `(game, card, date)`
index seeks one card's rows and yields them already date-ordered — no separate sort, no
timeseries extension); the storage model + why-not-Timescale rationale is in
`docs/tradeoffs.md` ("Price history & the historic-price chart", issue #297).

### Search syntax (`q`)

The MTG card-list endpoints parse `q` as (near-)full
[Scryfall syntax](https://scryfall.com/docs/syntax) (`api/src/scryfall/search/`).
Bare words / `"quoted phrases"` are card-name substrings (ANDed); `!"exact name"`
is an exact match; `/regex/` (on `name`/`t`/`o`/`ft`) runs a case-insensitive
regular expression — on SQLite via the registered `REGEXP` UDF (Rust `regex` crate;
sqlx's `regexp` feature, enabled by `with_regexp()` in `db.rs`), on Postgres via POSIX
`~*`, chosen by the `db::Dialect` seam. The pattern is pre-validated by the `regex`
crate on both backends (a bad pattern → 422); a Rust-valid but POSIX-invalid pattern
is a 422 on Postgres too (its `~*` raises SQLSTATE `2201B`, mapped to the same 422 in
`error.rs`). The two engines parse exotic constructs differently, so a given regex can
match different rows per backend. Supported filters: `name`/`n`,
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

### HTTP caching (CDN)

The router splits routes into two cache policies via response middleware
(`handlers::cache`, wired in `router.rs`). Public catalog reads (`/api/games/...`)
are the same for everyone and change at most daily, so a successful response carries
`Cache-Control: public, max-age=300, s-maxage=3600, stale-while-revalidate=86400` —
browser- and CDN-cacheable, served stale-while-revalidate so a cache miss never blocks
on the origin. Per-user, live, and error responses are `no-store`: all `/api/auth/*`
(access tokens + `Set-Cookie`), the import-`status` route (a live progress signal the
SPA polls), and any non-2xx (so a CDN can't pin a transient `404`/`5xx`). The image/icon
routes set their own longer `immutable` header, which the layer preserves.

**Cloudflare (issue #284 bullet 3).** These directives are all standard and
Cloudflare-honored: `public` makes a response edge-storable, `s-maxage` sets the edge
TTL (Cloudflare respects it over `max-age` at the edge), `stale-while-revalidate` is
supported, and the weak `ETag` + `If-None-Match → 304` revalidation (below) lets the
edge revalidate cheaply. Public reads set **no** `Set-Cookie` and **no** `Vary`, so
nothing defeats shared caching, and errors are forced to `no-store` so the edge never
pins a negative. The origin is therefore Cloudflare-ready as shipped. Operational
caveat: Cloudflare does **not** cache `/api/...` (JSON) paths by default — the edge only
stores a response once a **Cache Rule** marks its path eligible. Those rules already
exist and are documented in the README (*Behind a CDN (Cloudflare)*): its catalog rule
(extended to also match `/api/openapi.json`) makes the public reads
edge-cacheable, and its bypass rule keeps the per-user `/api/auth/*` (incl. the API-key
management routes), `/api/collection/*`, and `/api/wishlist/*` responses off the edge.
The header work is done; enabling edge caching is a dashboard step per deployment.

### Conditional requests (ETag / 304)

On top of the freshness policy, a second public-router middleware
(`cache::conditional_request_layer`, layered *outside* `public_cache_layer` so it can
read the `Cache-Control` that layer set) adds **validators** so a revalidation of a
stale cache entry transfers headers, not the whole body. It hashes each
cacheable-success body into a **weak `ETag`** (`W/"<128-bit hex>"`, a SHA-256 prefix —
weak because a downstream CDN may re-encode the payload in transit) and turns a matching
`If-None-Match` (RFC 9110 weak comparison, incl. `*` and comma-separated lists) into a
bodyless `304 Not Modified` carrying the `ETag` + `Cache-Control`. It deliberately
**skips** `immutable` responses (the image/icon proxy — never revalidated within
`max-age`, and hashing a large binary would be wasteful) and `no-store` responses
(errors / per-user), and only runs for `GET` (axum serves `HEAD` off the same handler
but strips the body, so a `HEAD` carries no validator). Buffering the body to hash it is
bounded by `MAX_ETAG_BODY_BYTES` (a body of unknown or over-cap size is served
un-`ETag`ged).

### Sitemaps (crawlers)

A DB-backed XML sitemap advertises the public catalog (`handlers::sitemap`).
`GET /sitemap.xml` is a **sitemap index** pointing at child sitemaps:
`/sitemaps/pages.xml` (static + per-game routes, the sealed hubs, and the legal pages),
`/sitemaps/sets.xml` (every set), `/sitemaps/cards-{n}.xml` (cards), and
`/sitemaps/products-{n}.xml` (sealed products). Cards and products are chunked at
5 000 URLs/file — well under the protocol's 50 000 cap, because Google timed out
fetching the full-size chunks (issues #294, #318). The `<loc>`s are the SPA's own routes
(e.g. `/cards/mtg/sets/blb`, `/sealed/mtg/{id}`), built against `PUBLIC_SITE_URL` —
not the API's `/api/...` URLs — with a `<lastmod>` from the set/card/product
`released_at` or the latest sync. Served at the **site root** so the sitemap-protocol
scope rule covers the whole site (dev Vite and the split-deploy Caddyfiles proxy
`/sitemap.xml` + `/sitemaps/*` to the API; the combined image routes them natively);
`/api/sitemap.xml` and `/api/sitemaps/{name}` still answer as aliases for
already-submitted URLs. The web build's `robots.txt` points crawlers at
`/sitemap.xml`. Each success carries a long `Cache-Control` (a day fresh, a week
stale-while-revalidate, preserved by the cache layer); an unknown/out-of-range child
is a `no-store` `404`.

### Image proxy

`size` ∈ `small|normal|large|png|art_crop` (default `normal`), `face` is a 0-based face
index for double-faced cards. On first request the image is downloaded from Scryfall
(HTTPS, host allow-listed to `scryfall.io`, redirects disabled), written under
`<DATA_DIR>/images/<game>/<size>/`, and served from disk thereafter (`Cache-Control:
immutable`). We never bulk-download the whole image catalogue — images are cached lazily,
on view. Set icons are cached the same way (`.../sets/{code}/icon`, served as
`image/svg+xml`). With **`CDN_MODE=true`** the on-disk step is skipped entirely: the
origin still fetches each asset on demand and serves it with the same `immutable`
headers, but never persists it — for deployments fronted by a CDN that caches those
responses, so the origin needs no writable image dir and is only hit on a CDN cache miss
(leave it off when no CDN sits in front, or every view re-fetches upstream).

Not every id has an image upstream — the TCGplayer product CDN, for one, `403`s the URL
for a product with no art (its `has_image` flag is only a best-effort hint). When the
provider says an asset isn't there (a definitive `4xx`), the proxy returns **404** and
remembers the miss in-process for 6h (the negative cache), so a dead URL isn't re-fetched
or re-logged on every view; a *transient* upstream failure (`5xx` / rate-limit / network)
is a **502** and isn't cached, so it retries next request. The SPA falls back to a
placeholder on any non-2xx either way.

## Sealed products API contract

Public (no auth), game-namespaced reads under `/api/games/{game}/products`, sourced from
TCGCSV (booster boxes, bundles, decks, …). Handlers live in
`api/src/handlers/catalog/products.rs`; wired in `router.rs` (the `facets` route is a
static sibling of `/products/{id}`, so it never collides with a product id). `{id}` in a
path is the product's **external** (TCGplayer) id (a string). An unknown game/product is
`404`. The list endpoint paginates like the card lists (`?page` 1-based, `?page_size`
default 60 / max 200, `{ data, page, page_size, total, has_more }`).

Unlike cards, products don't wire the Scryfall search compiler — `q` is a plain
case-insensitive **name substring** (ASCII-folded on both backends). Set names are
resolved against `card_sets` (falling back to `null` when a product's group has no
matching catalog set), mirroring the collection set builder's graceful degradation.

| Method & path | Returns |
|---------------|---------|
| `GET /api/games/{game}/products?q&set&type&sort&dir&page&page_size` | page of `Product` (`{ data, page, page_size, total, has_more }`). `q` = case-insensitive name substring; `set` = one set code (matched case-insensitively); `type` = one `product_type`; `sort` ∈ `name` / `price`(=`usd`) / `released`(=`date`), `dir` ∈ `asc`/`desc`. Unknown `sort`/`dir` = `422` |
| `GET /api/games/{game}/products/facets` | `{ data: ProductFacets }` — the distinct filter values that actually occur among the game's products, so the SPA builds dropdowns without hardcoding. `ProductFacets = { types: string[], sets: ProductSetRef[] }`; `ProductSetRef = { code, name: string | null }`. `types` alphabetical; `sets` are the sets that have products, in resolved-name-then-code order (a blank set code is excluded) |
| `GET /api/games/{game}/products/{id}` | one `Product` |
| `GET /api/games/{game}/products/{id}/image?size` | the product image bytes, proxied + cached from the TCGplayer CDN (`tcgplayer-cdn.tcgplayer.com`, host allow-listed). `size` ∈ `normal` (1000×1000, default) / `small` (200w); the on-disk cache + `Cache-Control: immutable` + `CDN_MODE` behave exactly like the card image proxy |
| `GET /api/games/{game}/products/{id}/prices?range` | `{ data: ProductPricePoint[] }` — the product's price history, **oldest first** (`[]` if none in range). Reuses the exact `?range` windowing/downsampling as the card price endpoint (`api/src/handlers/catalog/pricing.rs`): no `range` = the full daily series, an explicit `range` (`7d`/`30d`/`1y`/`2y`/`3y`/`all`) windows + downsamples it, unknown `range` = `422` |
| `GET /api/games/{game}/products/{id}/cards?page&page_size&section` | page of `ProductCardEntry` — the cards this product is found to contain / can be pulled from, the **reverse** of `.../cards/{id}/sealed` (issue #204). Ordered by membership (`contains` → `booster` → `variable`, so the guaranteed cards lead) and, within the booster pool, **family-exclusive cards first** (a collector booster's special printings no other booster in the set can pull — each flagged `exclusive`, PR #221), then set code + collector number; each card deduped to its **strongest** membership with a foil-only flag. Optional `?section` (`contains`/`exclusive`/`booster`/`variable`) pages just one display section so the SPA paginates each on its own (issue #224); omit it for the whole ordered list — `total`/`has_more` then describe the selected section. Empty page when the product has no ingested contents; `404` for an unknown game/product, `422` for an unknown section |
| `GET /api/games/{game}/products/{id}/cards/sections` | `{ data: ProductCardSection[] }` — the **non-empty** display sections of the cards above, in display order (`contains` → `exclusive` → `booster` → `variable`) with per-section counts, so the SPA knows which independently-paginated blocks to render (issue #224) before fetching any card. `[]` when the product has no ingested contents; `404` for an unknown game/product |
| `GET /api/games/{game}/products/{id}/contents` | `{ data: ProductComponent[] }` — the product's **structural composition** ("what's in the box"): the nested packs/boxes it bundles (each linked to its own product page), precon decks, fixed promo cards (linked to the card), and physical extras, in display order with quantities. Sourced from MTGJSON's sealed-product `contents` via `sealed_components` (with curated fallback). `[]` when the product has no ingested composition (a bare booster pack, or a product neither MTGJSON nor the fallback describes); `404` for an unknown game/product |

`Product = { id, name, set_code, set_name: string | null, product_type, url: string |
null, has_image, prices: { usd, usd_foil }, msrp: string | null, released_at: string |
null }`. `id` is the external (TCGplayer) product id; `url` is the tcgplayer.com product
page (for buy-links); `has_image` is whether an image is available through the product
image proxy. Prices are **USD only** — TCGCSV carries no eur/tix (`ProductPrices = { usd,
usd_foil }`, decimal strings or `null`). `msrp` is the manufacturer's suggested **retail
list** price (USD decimal string, or `null`) — a separate, curated field from the market
`prices`: no feed carries sealed-product MSRP, so it comes from a committed, hand-curated
map (`tcgcsv::msrp`, keyed by TCGplayer product id) and is `null` for products not listed
there (issue #296).

`ProductPricePoint = { date (YYYY-MM-DD), usd, usd_foil }` — decimal strings exactly as
stored (any may be `null`), USD only. Same one-row-per-`(product, day)` model + last-real-
row-per-bucket downsampling as `PricePoint`.

The product price sort orders on a **numeric** cast of `usd` (falling back to `usd_foil`),
NULL/empty-guarded so truly-unpriced products sort **last** regardless of direction. The
natural default direction is `asc` for `name`, `desc` for `price`/`released` (priciest /
newest first reads better); an `id` tiebreak makes pagination deterministic.

The card→sealed-product membership endpoint (`GET .../cards/{id}/sealed`, in the card
catalog table above) returns `SealedProductRef = { product: Product, membership: string,
foil: bool }` — it wraps this same `Product` DTO (so the SPA reuses the product tile/grid)
with the membership bucket (`"contains"` / `"booster"` / `"variable"`) and a `foil` flag
(true when the card appears **only** as a foil in that product). Sourced from MTGJSON via
`sealed_contents`; the product-list/detail/price endpoints above are sourced from TCGCSV
(`products` / `product_price_history`).

`ProductCardEntry = { card: Card, membership, foil, exclusive }` is the reverse wrapper
(the `.../products/{id}/cards` endpoint above): the shared `Card` shape plus the membership
bucket, the foil-only flag, and `exclusive` (a `booster` card the product's booster line yields
but no *other* booster family in the set can — a collector booster's special printings). The
family judged is the product's **own** for a standalone booster, or, for a **bundle / gift box**,
the premium booster it *contains* (Collector, else a generic special booster like Final
Fantasy's Chocobo booster — issue #290): so a gift bundle's collector-only cards split out ahead
of the shared play pool. `false` for any non-`booster` card, a bundle wrapping only
play/set/draft boosters, or a set with no other booster family to compare against. A card that
is both contained in and pullable from the same product reports its **strongest** membership
(lowest rank), so it shows once, in the "found in" group. The by-card-id lookups are chunked
(`PRODUCT_CARDS_IN_CHUNK`, 900) so a giant product — Secret Lair "festival" bundles reference
thousands of cards — can't blow SQLite's per-statement bind limit.

`ProductCardSection = { key, total, booster_family }` (the `.../cards/sections` endpoint) is
the display-section manifest: `key` is `contains` / `exclusive` / `booster` / `variable` (the
value the `?section` filter takes), `total` its card count. `booster_family` is set **only on
the `exclusive` section** — a representative `product_type` slug (e.g. `collector_pack`) naming
the family those cards are exclusive to, so the SPA titles the block after the *contained*
booster ("Collector Booster exclusives") even for a bundle whose own type carries no family;
`null` on every other section. Only non-empty sections are returned, in display order — so the
SPA renders one independently-paginated block per section (issue #224) and knows each's size
without fetching its cards. The four sections are the membership buckets with the `booster`
pool split by `exclusive`; their combined `total` is the whole card count.

`ProductComponent = { kind, name, quantity, product: Product | null, card: Card | null }`
(the `.../products/{id}/contents` endpoint) is one "what's in the box" line item: `kind` is
`sealed` (a nested pack/box), `deck` (a precon deck), `card` (a fixed promo), or `other` (a
physical extra — a die, storage box, land pack…); `name` is the display label (the linked
child's catalog name when one resolves, else MTGJSON's); `quantity` how many the product
holds. A `sealed` component links its sub-product via the embedded `product` (reusing the
`Product` shape, so the SPA renders a linked tile — "the products this box contains"); a
`card` component links its card via `card`; `deck`/`other` (and unresolved links) leave both
`null`. Sourced from MTGJSON's `contents` (nested `sealed` w/ counts, `deck`, `card`, `other`)
via `sealed_components`; `pack`/`variable` are left to the card-membership pass above. The
component list is **non-recursive** (a box's direct contents, not its packs' cards).

## Collection API contract

Per-user, **authenticated** (`Authorization: Bearer <access_token>`, via `AuthUser`),
game-namespaced under `/api/collection/{game}`. Every route is in the router's
`private` group, so responses are `Cache-Control: no-store` (per-user data is never
shared-cached). Ownership is always scoped to the token's user id, so one user can
neither read nor mutate another's collection. Card ids in the path are the **external**
card id (the same id the public catalog exposes); the handler resolves it to the
internal `cards.id` before storage (so a holding survives a catalog re-import). A
missing token is `401`; an unknown game/card is `404`. These endpoints are **per-user
rate limited** (issue #168, `ratelimit::user_rate_limit`, keyed by the token's user
id): a generous `general` quota covers reads/edits/batch lookups, and a tighter
`import` quota covers the expensive import/sync/CSV endpoints; over-limit is `429` +
`Retry-After` (and, being per-user, `no-store`).

A "holding" is `(user, game, card) → { quantity, foil_quantity }`; there is no row for
a card you don't own (setting both counts to zero deletes the row), so the table holds
only owned cards. Model: `entities/collection_item.rs` (`collection_items`, unique on
`(user_id, game, card_id)`, `user_id` FK → `users` `ON DELETE CASCADE`).

| Method & path | Body | Returns |
|---------------|------|---------|
| `GET /api/collection/{game}?…&set&include_related` | — | page of `CollectionEntry`, most-recently-updated first (`?page`/`?page_size`, default 60 / max 200) — `{ data, page, page_size, total, has_more }`. Optional `?set=<code>` scopes to one set (ANDed with `q`) — the per-set collection view; with `?include_related=true` the scope spans the set's whole **group** (root + related sub-sets), the collection mirror of the catalog's `include_related` (resolved via the same `group_set_codes`) |
| `GET /api/collection/{game}/summary?set&include_related` | — | `CollectionSummary` `{ unique_cards, total_cards, total_value_usd, bulk_value_usd }` (see below). Optional `?set=<code>` scopes the stats to one set; `?include_related=true` (with a set) spans the set's whole **group** (root + related sub-sets, same `group_set_codes` as the list) so the value matches the include-related browse view. Backs the scoped collection value shown next to the browse count (issue #119) |
| `GET /api/collection/{game}/sets` | — | `{ data: CollectionSet[] }`, newest set first — the sets the user owns cards in, each the catalog `Set` shape plus owned aggregates (see `CollectionSet` below). Powers the collection's per-set landing (mirrors the catalog's game → sets view) |
| `GET /api/collection/{game}/sets/{code}/drops?q&page&page_size` | — | the signed-in user's **owned** cards in a drop-grouped set (e.g. Secret Lair), grouped by **Secret Lair drop** and **paginated by drop** — `{ data: CollectionDropGroup[], page, page_size, total, has_more }` where `CollectionDropGroup = { slug, title, card_count, cards: CollectionEntry[] }` and `total` counts drops. The collection mirror of the catalog's set-drops endpoint (owned cards only, each carrying its owned counts); a drop the user owns nothing in is absent, cards not in the snapshot fall into a trailing `"Other"` group. `404` if the set isn't drop-grouped (use `has_drops`); optional `q` filters, dropping now-empty drops |
| `GET /api/collection/{game}/sets/{code}/subtypes?q&page&page_size` | — | the signed-in user's **owned** cards in a set, grouped by **sub-type** (card treatment) and **paginated by sub-type** — `{ data: CollectionSubtypeGroup[], page, page_size, total, has_more }`, `CollectionSubtypeGroup = { slug, title, card_count, cards: CollectionEntry[] }`, `total` counts sub-types. The collection mirror of the catalog's `/subtypes` endpoint (owned cards only, each carrying its owned counts); a sub-type the user owns nothing in is absent. Any set works (no drop-table gate; the SPA gates on `has_subtypes`); optional `q` filters, dropping now-empty sub-types |
| `GET /api/collection/{game}/cards/{id}` | — | `{ quantity, foil_quantity }` — the owned counts for one card (zeros if not owned) |
| `PUT /api/collection/{game}/cards/{id}` | `{ quantity, foil_quantity }` | `{ quantity, foil_quantity }` — sets the **absolute** counts (not a delta); both zero removes the card; a negative or oversized (`> 1_000_000`) count is `422`. Upserts on the unique key (a concurrent first-add that loses the race falls back to an update) |
| `POST /api/collection/{game}/owned` | `{ ids: string[] }` | `{ data: { [externalId]: { quantity, foil_quantity } } }` — batch owned counts for the given cards, **owned cards only** (unowned ids are absent, so nothing owned → `{ "data": {} }`). Blank/duplicate ids are trimmed away; **> 500 ids** is `422`. A `POST` (not a `GET` query) so a big browse page's id list can't blow the request-line length behind a proxy. Powers the owned-count badges overlaid on the public browse grids |
| `POST /api/collection/{game}/import` | `{ provider, source, mode }` | **`202`** `ImportJob` `{ job_id, status: "queued" }` — enqueues a one-off import (runs async; poll the job below). Validated synchronously: `422` for an unknown provider / unparseable source; `503` if too many imports are queued. `provider` is `"archidekt"` or `"moxfield"`; `source` is a collection URL or bare id; `mode` ∈ `overwrite`/`replace`/`merge`/`smart` (see below). Does not save a link |
| `POST /api/collection/{game}/import/csv?mode=` | raw CSV body (`text/csv`) | **`200`** `ImportSummary` — import an uploaded CSV export, sniffing the shape from the header row: **Archidekt** (a Scryfall ID column, plus Finish + Quantity) or **Moxfield** (no card id — Count + Edition + Collector Number + Foil, resolved against the catalog by set + collector number; `Proxy=True` rows are skipped). Runs **synchronously** (no upstream fetch → no job/rate-limiter): parses the CSV, reconciles per `?mode` (`overwrite`/`replace`/`merge`), returns the summary directly (its `provider` reflects the detected shape). A CSV is inherently one-off (no link is saved). Body is bounded by a route body limit (`MAX_CSV_UPLOAD_BYTES`, 16 MB) → `413` if larger; `422` for a bad mode / unreadable CSV / one missing a required column / a header matching neither shape / an empty upload |
| `GET /api/collection/{game}/import/jobs/{job_id}` | — | `ImportJob` `{ job_id, status, progress?, summary?, error? }` — poll an import/sync job. `status` ∈ `queued`/`running`/`complete`/`error`; `progress` (`ImportProgress = { fetched, total? }` — provider rows fetched so far + the provider-reported total when known; `total` absent for a smart sync, which has no meaningful total) present only while `running`; `summary` (an `ImportSummary`) present on `complete`, `error` message on `error`. `404` for an unknown job or another user's |
| `GET /api/collection/{game}/source` | — | `CollectionSource` or `null` — the saved collection link for this game |
| `PUT /api/collection/{game}/source` | `{ provider, source, smart? }` | `CollectionSource` — save/upsert the link (one per user+game; validates the source resolves; does not sync). `smart` (default `false`) records whether re-syncs use smart (incremental) sync vs. a full mirror |
| `DELETE /api/collection/{game}/source` | — | `204` — forget the saved link (idempotent) |
| `POST /api/collection/{game}/sync` | — | **`202`** `ImportJob` — enqueues a re-sync from the saved link (the worker stamps `last_synced_at` on success). Uses **smart** sync when the saved link opted in, otherwise **mirror/replace**. `404` if no link is saved |
| `GET /api/collection/{game}/export?format=` | — | **`text/csv`** download (`Content-Disposition: attachment; filename="tcglense-{game}-collection-{format}.csv"`) of the whole collection in a provider shape — `?format=archidekt` (default) or `moxfield`. Unpaginated; one row per non-empty finish bucket (a card owned in both finishes yields a Normal/regular row **and** a Foil row), name-sorted. The inverse of the CSV upload, and a re-importable round trip (see **Export** below). `422` for an unknown `format` |

`CollectionEntry = { card: Card, quantity: number, foil_quantity: number }` — `card` is
the full catalog `Card` shape (reusing the shared `CardResponse`).

`CollectionSummary = { unique_cards, total_cards, total_value_usd, bulk_value_usd }`
(`api/src/handlers/shared/holdings.rs`): distinct cards owned, total copies (regular +
foil), an estimated USD value (regular copies at `usd`, foil at `usd_foil`, a 2-dp string,
`null` if nothing owned is priced), and a **bulk slice** — `bulk_value_usd` is the value
of just the finishes priced **under $1 each** (the low-value commons/uncommons), a 2-dp
string; `"0.00"` when something is priced but none of it is bulk, `null` when nothing owned
is priced.

`CollectionSet` is the catalog `Set` shape (`code`, `name`, `set_type`, `released_at`,
`card_count`, `icon_svg_uri`, `parent_set_code`, `has_drops`, `has_subtypes` — the latter
derived from the user's *owned* cards in the set) plus owned aggregates:
`owned_cards` (distinct owned), `owned_copies` (regular + foil), `owned_value_usd`
(estimated USD value, same semantics as the summary's `total_value_usd`, scoped to the one
set, `null` if none priced), and `owned_bulk_value_usd` (the set-scoped bulk slice — value
of just the finishes priced under $1 each; `"0.00"` when priced-but-none-bulk, `null` when
nothing owned in the set is priced). Built by `build_collection_sets`
(`handlers/shared/holdings.rs`, generic over `HoldingCounts` and shared with the wish
list), which aggregates owned holdings per `set_code` — dressing each with its `card_sets` metadata
(falling back to the card's own `set_name` when the set row is missing) — and orders newest
set first.

`CollectionDropGroup = { slug, title, card_count, cards: CollectionEntry[] }` is the
collection mirror of the catalog's `DropGroupResponse` (owned cards only, each carrying its
counts); the `.../sets/{code}/drops` handler reuses the shared generic `group_into_drops`
(generic over the grouped item, so it folds `(collection_item, card)` pairs) and paginates
by drop in memory. `CollectionSubtypeGroup` is the same shape for the `.../sets/{code}/subtypes`
handler, which reuses the sibling `group_into_subtypes` over the same pairs. The batch/import `POST`s, the `PUT`s, and the saved-source `DELETE` need
CORS `POST`/`PUT`/`DELETE` (all in the allow-list alongside `GET`); in dev/prod the SPA is
same-origin so CORS isn't exercised, but a direct cross-origin call needs it.

The **owned-count badges** (issue #85) reuse the same stacked-cards / sparkles chip the
collection grid shows (`components/cards/OwnedCountBadge.vue`): while signed in, the
catalog browse grids (all-cards, a set — flat or by-drop — and a card's other printings)
overlay each owned card with its total + foil counts. The web side looks up the visible
page's ids via `useOwnedCounts` (gated on auth, empty while signed out) — splitting them
into batches of ≤ 400 under the server cap (so even a big drop-grouped "Other" page never
trips the 422) and merging the results — and `CardGrid` renders the badge for any card
present in the map.

### Import / sync

`handlers::collection` + the `collection_import` module pull a collection from an external
provider **server-side** (via the shared `AppState.http` client) and reconcile it into
`collection_items`. Because the provider enforces a strict request cap (Archidekt ≈20
req/min), an import of a large collection takes minutes, so it runs **asynchronously**: the
handler validates synchronously, enqueues a background job (`collection_import::jobs`), and
returns `202` + a `job_id`; the SPA polls the job-status route until `complete`/`error`.
Imports run **one at a time** (a job waiting for the slot reports `queued`), and a
**per-provider** `RateLimiter` (`collection_import::rate_limit`'s `ProviderLimiters`, one
limiter per provider — Archidekt + Moxfield both 20/min ⇒ one request every 3s for now,
tunable independently) throttles **every** request to that provider across all imports (so
one provider's spacing/back-off never stalls another's). If the provider still returns
**`429`**, the fetch **backs off** that provider's limiter by at least a minute (honoring a
larger `Retry-After`, capped at 5 min) so all imports for *that provider* pause, then
retries the same page — giving up (`503`) after a few attempts.

Providers are dispatched by a `Provider` enum (Archidekt + Moxfield, one module per
service), each fetching + parsing to normalized `(external_card_id, foil, quantity)`
holdings; the provider-independent engine aggregates by card (`(uid, foil)` — the same
printing can span several provider rows), resolves each `external_card_id` to
`cards.external_id` (for both providers that's the Scryfall id: Archidekt's `card.uid`,
Moxfield's `card.scryfall_id`) in chunked `IN` lookups, skips unmatched cards, then applies
the chosen `ReconcileMode` in one transaction (atomic `ON CONFLICT` upserts + keyed
deletes).

The **CSV upload** path (`collection_import::csv_import` + `execute_csv_import`) is a second
*source* of the very same holdings: it sniffs the export shape from the header row and
parses an **Archidekt** export (only the Scryfall ID / Finish / Quantity columns) or a
**Moxfield** export (no card id — Count / Edition / Collector Number / Foil, plus optional
Name for unmatched labels and Proxy to skip proxies; rows pre-resolve to external ids by
`(set_code, collector_number)`, per-set chunked lookups, with unmatched rows keeping a
readable `"Name (set #num)"` placeholder that surfaces in the summary sample). Both paths
are defensive (the `csv` crate handles quoting/escaping, a leading BOM is stripped, a
non-UTF-8 body is rejected, rows are capped at `MAX_IMPORT_ROWS`, per-field length bounds,
and the finish is keyed off the shared foil rules) and yield `Vec<FetchedHolding>`, then
run the exact same aggregate/resolve/reconcile/apply engine — but with no upstream fetch, so
no rate limiter or job, reconciling inline in the request (the handler bounds the body with
a route-scoped `DefaultBodyLimit`).

`ImportSummary = { provider, mode, total_rows, distinct_cards, matched_cards,
unmatched_cards, unmatched_sample, regular_copies, foil_copies, removed_cards,
stopped_early }`. Import jobs live in-memory in `AppState.imports` (lost on restart; the
client just re-imports). A saved link is `entities/collection_source.rs`
(`collection_sources`, unique on `(user_id, game)`, `user_id` FK → `users` `ON DELETE
CASCADE`, stores `provider` + `external_id` + `last_synced_at` + `smart`). Both providers
are MTG-only. **Moxfield's live URL import is currently disabled**
(`Provider::network_import_enabled()` returns `false` pending an approved
`MOXFIELD_USER_AGENT`): the handlers reject a Moxfield URL import, saved-link save, or
re-sync with a `422` pointing at the CSV upload, and the web import dialog greys
Moxfield out in the link picker — see `docs/tradeoffs.md`. Archidekt is fetched at `https://archidekt.com/api/collection/{id}/?page={n}`
(25 rows/page, capped at `MAX_IMPORT_ROWS`); the id is validated all-digits. Moxfield is
fetched at `https://api2.moxfield.com/v1/collections/search/{id}?pageNumber={n}&pageSize=100`
(paged on the envelope's `totalPages`, same row cap; `isProxy` rows skipped); the id is
validated against the base64url charset, and it sends the approved `MOXFIELD_USER_AGENT`
when configured (Moxfield's bot wall only serves approved clients — a `403` maps to a clear
"needs an approved User-Agent" error). Either way the URL is built host-side from the
validated id, so there's no SSRF surface.

The **smart** mode (`ReconcileMode::Smart`, issue #101) is an *incremental* mirror for
re-syncing a mostly-unchanged collection cheaply under the rate limit. It fetches the
provider collection **most-recently-updated first** (Archidekt `?orderBy=-updatedAt`;
Moxfield `sortType=lastUpdated&sortDirection=descending` — an edit-aware order: a card
whose count changed bubbles to the top even though its row-visible created-at is old) and
**stops paging once a whole page already matches what we hold** (`fetch_holdings_smart` +
the pure `smart_absorb_page`, judged per page after the whole page is folded into the
running aggregate so a card owning a regular + foil finish isn't seen mid-aggregate). It
then **overwrites each fetched card's observed finishes** but **preserves any finish it
never fetched** (its rows sit in the unscanned tail) and **never deletes**
(`reconcile_smart`), so an early stop can't zero a foil we simply didn't page to. The
trade-off: because it never fetches the whole collection, smart only touches
recently-changed cards — it will **not** remove cards deleted upstream (a full `Replace`
does). `stopped_early` reports whether the fetch stopped at the already-synced tail vs.
scanned everything. Smart is offered in the import dialog as a mode, and on a saved link via
its stored `smart` flag (the saved re-sync then runs smart instead of mirror/replace).

### Export

`GET /api/collection/{game}/export` (`handlers::collection::export`) is the inverse of the
CSV upload: it streams the signed-in user's whole collection as a CSV shaped like a genuine
**Archidekt** or **Moxfield** export (`?format=`, default Archidekt), so it round-trips —
re-uploading an exported file reproduces the same holdings. It reuses the collection list's
`owned_with_cards` base query (unpaginated — an export is the entire collection; a holding
whose catalog card row is gone is skipped, as every other read does), name-sorts, and writes
one row **per non-empty finish bucket** (regular `quantity` + foil `foil_quantity` become up
to two rows per card). The `csv` crate handles quoting/escaping (Moxfield quotes every
field, Archidekt only when needed) with RFC 4180 CRLF terminators, matching the real files.

Because a holding only stores two counts, the export fills the provider columns we don't
track with the neutral values a fresh export uses — Condition `NM`/`Near Mint`, Language
`EN`/`English`, blank Purchase Price/Tags, `Alter`/`Proxy` `False` — and the Archidekt-only
`Multiverse Id`/`MTGO ID` columns (which the catalog never ingests) as `0`, exactly as
Archidekt does for a card it can't map. Card metadata comes from the joined `cards` row: the
foil finish is `Foil`/`foil` (regular is `Normal`/blank, both of which the importer reads
back as non-foil), the round-trip key is the `Scryfall ID` column (Archidekt) or the
`Edition` (set code) + `Collector Number` pair (Moxfield), and the derived Archidekt columns
map our stored data — colour letters → full names (`W` → `White`), `cmc` → an integer-when-
whole Mana Value, and the `type_line` split into `Types`/`Sub-types`/`Super-types` on the em
dash. See `docs/tradeoffs.md` for the field-omission rationale.

## Wish list API contract

Per-user **wish list** (issue #167) — the cards a user wants to buy. Same auth, id
resolution, and holding semantics as the collection (bearer `AuthUser`, private
`no-store` group, external card ids resolved to internal, `(user, game, card) →
{ quantity, foil_quantity }`, both-zero deletes the row) over its **own, fully
independent table**: `entities/wishlist_item.rs` (`wishlist_items`, unique on
`(user_id, game, card_id)`, `user_id` FK → `users` `ON DELETE CASCADE`). There is
**no import/sync layer** (scoped out by the issue), and the wire shapes are the
**same DTOs the collection returns** (`CollectionEntry`/`CollectionSummary`/
`CollectionSet`/`CollectionDropGroup`/quantities — hoisted into
`handlers/shared/holdings.rs`), so the `owned_*` field names read as "wanted" here.
(The summary/set DTOs still carry the `bulk_value_usd` / `owned_bulk_value_usd` fields,
but the wish-list UI shows only the **total** value — a wish list is a shopping list, so
only what it costs matters — and ignores the bulk slice.) Each route mirrors its
collection twin exactly (params, ordering, errors, caps):

| Method & path | Mirrors |
|---------------|---------|
| `GET /api/wishlist/{game}?q&sort&dir&set&include_related&page&page_size` | the collection list (most-recently-updated first, Scryfall `q`, set/group scope) |
| `GET /api/wishlist/{game}/summary?set&include_related` | the collection summary (unique / copies / value of what's wanted) |
| `GET /api/wishlist/{game}/sets` | the collection per-set aggregates (sets holding wishlisted cards, newest first, counts + value) |
| `GET /api/wishlist/{game}/sets/{code}/drops?q&page&page_size` | the collection by-drop view (`404` if the set isn't drop-grouped) |
| `GET /api/wishlist/{game}/sets/{code}/subtypes?q&page&page_size` | the collection by-sub-type view (any set; the SPA gates on `has_subtypes`) |
| `POST /api/wishlist/{game}/counts` `{ ids }` | `POST .../owned` (batch counts, listed cards only, > 500 ids `422`) — named `/counts` because a wish list doesn't track ownership |
| `GET /api/wishlist/{game}/cards/{id}` | the single-card counts read (zeros if absent) |
| `PUT /api/wishlist/{game}/cards/{id}` `{ quantity, foil_quantity }` | the absolute-count upsert (both-zero deletes, negative/oversized `422`) |

The wish list and the collection never read or write each other's rows (pinned by a
security test); browse-grid badges/ghosts on wish-list pages come from `/counts` the
same way the catalog's come from `/owned`.

## Dataset mirror

Optional public endpoints (`handlers::mirror`), wired **only when `MIRROR_ENABLED=true`**
(off by default — an enabled mirror is a public read proxy to the upstream dataset
hosts; rationale + trade-offs in `docs/tradeoffs.md`). Each dataset route streams the file
from its fixed upstream host on demand — no disk persistence, the same fetch-and-serve
model as `CDN_MODE` — with CDN-cacheable headers via the shared `public_cache_layer`. The
`kind`/path inputs are sanitised (host-locked, no traversal). The MTGJSON route forwards
`If-None-Match` and relays the upstream `304`, so an unchanged file stays a cheap
conditional.

| Method & path | Returns |
|---------------|---------|
| `GET /api/mirror/scryfall/bulk-data` | Scryfall's bulk-data catalog JSON |
| `GET /api/mirror/scryfall/sets` | Scryfall's sets listing |
| `GET /api/mirror/scryfall/file/{kind}` | the named Scryfall bulk file (`kind` validated) |
| `GET /api/mirror/mtgjson/AllPrintings.json.gz` | MTGJSON's `AllPrintings` gzip (ETag-conditional) |
| `GET /api/mirror/tcgcsv/{*path}` | the TCGCSV path proxied through (catalog / prices / daily archives) |
| `GET /api/mirror/fingerprints/{game}` | the visual-scanner match index for `game` as a compact binary payload (`application/octet-stream`), so other instances **import** it instead of hashing card images |

The **fingerprint** route is not an upstream proxy: it serializes this origin's own
in-memory index (built by the operator's `FINGERPRINT_BUILD_ENABLED` instance) into a
self-describing little-endian payload — `magic "TCGLFP01"` · `algo_version i32` ·
`count u32` · then `count` records of `id_len u16` · `external_id` · `face_index i32` ·
`32-byte pHash` (see `catalog::fingerprint_sync`). It carries a strong content `ETag`, so
a consumer with the current index gets a bodyless `304`. A self-host with
`FINGERPRINT_IMPORT_ENABLED=true` (default) pulls this on the sync interval, version-gates
it against its own `FINGERPRINT_ALGO_VERSION`, and replaces its local `card_fingerprint`
rows in one transaction — the ~3–4 MB MTG index is fetched **once per change**, so every
ordinary self-host runs the scanner while fetching **zero** card images.
