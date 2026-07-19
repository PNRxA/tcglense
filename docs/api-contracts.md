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

`User` shape: `{ id: number, email: string, created_at: string (RFC3339 UTC), username:
string | null, discriminator: number | null, handle: string | null, currency: string }`.
`currency` is the account's ISO 4217 display preference (`USD` by default; one of
`USD`/`AUD`/`CAD`/`EUR`/`GBP`/`JPY`/`NZD`). Catalog prices and valuation fields remain
canonical USD on the API.

**Two-token model:** a short-lived **access token** (JWT, 15 min, returned as
`access_token`, kept in memory on the client) plus a long-lived **refresh token**
(opaque, 30 days, delivered only as the `tcglense_refresh` httpOnly cookie, stored
server-side as a SHA-256 hash).

**Email-first registration (issue #176):** register takes `{ email, redirect? }` and always
answers a generic `200 { completion_token: null }` — a new, a pending, and an
already-registered address are indistinguishable (no enumeration oracle; the pre-#176
duplicate-email `409` is gone). A new address creates a **pending** account
(`users.password_hash` is now nullable — `NULL` = pending, cannot sign in) and emails a
**completion link** (`{PUBLIC_SITE_URL}/complete-registration?token=…`, a
`complete_registration` email-token, 24h, single-use, issued through the 60s cooldown,
sent fire-and-forget off the request path); re-POSTing register re-sends it for a
pending address (the cooldown collapses bursts) and sends nothing for an activated one.
`POST /api/auth/complete-registration` then consumes the token, sets the first password
(+ optional public username), stamps the email verified (using the link proves mailbox
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
posture and requires the explicit local-only `ALLOW_INSECURE_DEV_AUTH=true` opt-in (the
e2e suite runs this way); the test suites use a mail *sink* (which counts as enabled) so
`completion_token` stays null and they exercise the real email-first flow end to end.
Internet-facing configurations reject that flag and refuse to enable signups while the
email bypass is active.

| Method & path | Body | Success | Notes |
|---------------|------|---------|-------|
| `POST /api/auth/register` | `{ email, redirect? }` | `200 { completion_token }` — `completion_token` is **always `null`** (the link is emailed) unless email is disabled (dev bypass: the token is returned so the SPA can drive the set-password step) | generic (a new, pending, or already-registered address are identical — no `409`) · a safe same-origin `redirect` is carried through the completion link; external/malformed values are ignored · `422` invalid email · `403` when `SIGNUPS_ENABLED=false` (message from `SIGNUPS_DISABLED_MESSAGE` or generic; checked **before** the CAPTCHA — see `GET /api/config`) |
| `POST /api/auth/complete-registration` | `{ token, password, username? }` | `200 { access_token, user }` + refresh cookie — finishes registration: sets the first password, verifies the email, and signs in | `401 "invalid or expired token"` (also once the account already has a password) · `422` weak password (checked **before** the token is spent) · `403` when `SIGNUPS_ENABLED=false` (so a link minted before signups closed can't finalise a new account) |
| `POST /api/auth/login` | `{ email, password }` | `200 { access_token, user }` + refresh cookie | `401 "invalid email or password"` (generic — incl. a pending password-less account, same dummy-hash timing) · `403 "email not verified"` (skipped when email is disabled; now only reachable by grandfathered accounts) |
| `POST /api/auth/refresh` | — (refresh cookie) | `200 { access_token, user }` + **rotated** cookie; returning identity with the token prevents stale/cross-tab account state | `401` if missing/invalid/expired/reuse-burned (clears cookie); a benign concurrent superseded-token submit returns `401` **without** clearing a newer cookie |
| `POST /api/auth/logout` | — (refresh cookie) | `204` (revokes that login family + clears cookie) | idempotent |
| `GET /api/auth/me` | — (`Authorization: Bearer <access_token>`) | `200 { user }` | `401` if missing/invalid/expired |
| `PUT /api/auth/currency` | `{ currency }` (`Authorization: Bearer <access_token>`) | `200 User` — persists the account's preferred display currency | `422` unsupported currency · read-only API key `403` |
| `POST /api/auth/verify-email` | `{ token }` | `204` (stamps `users.email_verified_at`; no session) | `401 "invalid or expired token"` |
| `POST /api/auth/resend-verification` | `{ email }` | `204` **always** (anti-enumeration; async send, 60s cooldown; only re-sends for a grandfathered password-bearing unverified account — a pending registration re-sends via `register`) | `422` invalid email shape |
| `POST /api/auth/forgot-password` | `{ email }` | `204` **always** (anti-enumeration; async send, 60s cooldown) | `422` invalid email shape |
| `POST /api/auth/reset-password` | `{ token, password }` | `204` (re-hashes the password, invalidates **every access and refresh session, plus every programmatic `tcgl_` API key,** plus sibling reset links, verifies a still-unverified email — so forgot/reset also activates a pending password-less account) | `401` bad token · `422` weak password (checked **before** the token is spent) · `403` when `SIGNUPS_ENABLED=false` **and** the account is still pending (password-less) — this reset-activation is the same new-account creation the signup gate refuses; a genuine reset for an already-active account still works |
| `GET /api/health` | — | `200 { status: "ok" }` (including maintenance mode) | — |
| `GET /api/ready` | — | `200 { status: "ready" }` after a database round-trip | `503 { status: "unavailable" }` without internal details when storage is unavailable · `503 { status: "maintenance" }` when `MAINTENANCE_MODE=true` **or** while the boot migrations are still running (the listener binds first so `/api/health` stays 200; the site presents as maintenance until the schema is ready) |
| `GET /api/config` | — | `200 { maintenance_mode: bool, turnstile_site_key: string \| null, signups_enabled: bool, signups_disabled_message: string \| null }` — public runtime config the SPA reads on boot and before rendering the auth forms; remains available during maintenance so a cached SPA can switch to its maintenance screen (`maintenance_mode` is `true` when `MAINTENANCE_MODE=true` **or** while the boot migrations are still running); `signups_disabled_message` is non-null only when `signups_enabled` is `false`; `no-store` | — |
| `GET /api/currencies` | — | `200 { base: "USD", as_of: "YYYY-MM-DD", rates: Record<string, number> }` — daily display rates; `rates.USD` is `1`; after 12h the last-good snapshot is returned immediately while one background refresh runs, with a hard seven-day stale limit | `502` when no snapshot exists or the last-good snapshot is over seven days old and refresh is unavailable |

With `MAINTENANCE_MODE=true`, startup still applies database migrations but does not
start background jobs. Apart from the two probes and uncached runtime config above,
every API and combined-image SPA/static request returns a non-cacheable
`503 { error: "service is under maintenance", code: "maintenance" }`. The machine-readable
code lets an already-open SPA replace its normal shell after any application request.

The **boot-migration window presents identically**: the API binds its listener before
running the schema migrations (so `/api/health` answers the platform health check even
when a large migration takes minutes), and a startup gate serves that same maintenance
`503` — plus `maintenance_mode: true` from `/api/config` and `{ status: "maintenance" }`
from `/api/ready` — until the migrations finish, so no request touches a half-migrated
schema. A migration failure exits the process so the deploy rolls back.

**Anti-abuse (all seven auth mutation endpoints above):** each request body may carry
a `captcha_token` (Cloudflare Turnstile). When `TURNSTILE_SECRET_KEY` is set the
token is **required** — a missing/rejected one is `400 "captcha …"` (deliberately
not 401/403, so it never collides with login's `403`), verified **before** any
account lookup so it leaks nothing. A successful Siteverify response must also match
`PUBLIC_SITE_URL`'s hostname and the widget's `auth` action. When the key is unset,
CAPTCHA is disabled and
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
atomic conditional `UPDATE`, with the claim, session-generation check, successor
insert, lineage link, user load, and any reuse revocation in **one transaction**
serialized per user, so a dropped request or DB failure rolls the rotation back)
with **lineage-based reuse detection**. Each token records its successor and login
family: replaying a token whose successor was itself consumed burns that family only,
while a benign concurrent double-submit whose successor is still active is rejected
without touching the cookie. A revoked token is never exchanged for a new one, and
replaying an old token cannot revoke a separately-created login family. Access and
refresh tokens capture the user's session generation; password reset advances it so
already-minted access JWTs fail immediately. The cookie is `HttpOnly; SameSite=Lax; Path=/api/auth;
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
network (the security-test harness swaps in a capturing mailbox). The SPA captures
each query token into memory and immediately replaces the visible URL without it;
production responses also set `Referrer-Policy: no-referrer`.

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
| `GET /api/games/{game}/sets/{code}/drops?q&page&page_size` | a drop-grouped set's cards broken into **Secret Lair drops** (Scryfall's curated drop titles), **paginated by drop** — `{ data: DropGroup[], page, page_size, total, has_more }` where `DropGroup = { slug, title, card_count, cheapest_prints_usd, cards: Card[] }` and `total` counts drops. `cheapest_prints_usd` is the drop's "cheapest prints" total — for each **distinct** card in the drop (by gameplay identity/`oracle_id`, so a foil-variant printing isn't double-counted), the price of its cheapest available printing *anywhere in the catalog* (the lower of that printing's regular and foil price, so a card is floored at a cheap reprint rather than its Secret Lair printing), summed. A canonical USD decimal string (the SPA renders it in the display currency), or `null` when no card in the drop has a priced printing. Computed with one extra indexed `(game, oracle_id)` lookup scoped to the page's cards. Drops keep Scryfall's order; within a drop, cards are by collector number. Cards not in the snapshot fall into a trailing `"Other"` group (`slug: null`). `404` if the set isn't drop-grouped (use `has_drops`); optional `q` filters cards, dropping now-empty drops |
| `GET /api/games/{game}/sets/{code}/subtypes?q&page&page_size` | a set's cards grouped by **sub-type** (card treatment: Borderless, Showcase, Extended Art, Full Art, …), **paginated by sub-type** — `{ data: SubtypeGroup[], page, page_size, total, has_more }` where `SubtypeGroup = { slug, title, card_count, cards: Card[] }` and `total` counts sub-types. The sub-type is **derived** from the card's print attributes (see `crate::scryfall::subtypes`); every card classifies, so `Normal` heads the list, then treatments. Unlike `/drops` this never `404`s (any set groups — one `Normal` group if plain; the SPA gates the view on `has_subtypes`); optional `q` filters cards, dropping now-empty sub-types |
| `GET /api/games/{game}/cards?q&page&page_size&name` | page of `Card` (optional `q` Scryfall-style search; optional `name` = exact-name equality filter, the quick-add "printings of this name" step), by name |
| `GET /api/games/{game}/card-names?q&limit` | `{ data: string[] }` — up to `limit` (default 10, max 25) **distinct** card names containing `q` (case-insensitive; names *starting* with `q` first, then alphabetical). `[]` for a blank/absent `q`. Powers the collection/wish-list quick-add autocomplete |
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
`s`/`set`/`e`, `st`/`settype` (game-scoped subquery on `card_sets`), `cn`/`number`
(a bare integer with `:` — `cn:234` — matches that collector number **and** its
single-letter variants `234a`/`234b`/…, issue #479; `cn=234` and an already-suffixed
`cn:234a` stay exact, and `cn>`/`cn>=`/… compare the leading integer),
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
routes — and the dataset mirror's dated TCGCSV archives — set their own longer
`immutable` header, which the layer preserves.

**Cloudflare (issue #284 bullet 3).** These directives are all standard and
Cloudflare-honored: `public` makes a response edge-storable, `s-maxage` sets the edge
TTL (Cloudflare respects it over `max-age` at the edge), `stale-while-revalidate` is
supported, and the weak `ETag` + `If-None-Match → 304` revalidation (below) lets the
edge revalidate cheaply. Public reads set **no** `Set-Cookie` and **no** `Vary`, so
nothing defeats shared caching, and errors are forced to `no-store` so the edge never
pins a negative. The origin is therefore Cloudflare-ready as shipped. Operational
caveat: Cloudflare does **not** cache `/api/...` (JSON) paths by default — the edge only
stores a response once a **Cache Rule** marks its path eligible. Those rules already
exist and are documented in the [self-hosting guide](./self-hosting.md#behind-a-cdn-cloudflare) (*Behind a CDN (Cloudflare)*): its catalog rule
(extended to also match `/api/openapi.json`, and — on a mirror host — the dataset mirror
`/api/mirror/*`, issue #192) makes the public reads edge-cacheable, and its bypass rule
keeps the per-user `/api/auth/*` (incl. the API-key management routes),
`/api/collection/*`, and `/api/wishlist/*` responses off the edge.
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
**skips** `immutable` responses (the image/icon proxy and the mirror's dated TCGCSV
archives — never revalidated within `max-age`, and hashing a large binary would be
wasteful) and `no-store` responses
(errors / per-user), and only runs for `GET` (axum serves `HEAD` off the same handler
but strips the body, so a `HEAD` carries no validator). Buffering the body to hash it is
bounded by `MAX_ETAG_BODY_BYTES` (a body of unknown or over-cap size is served
un-`ETag`ged).

### Sitemaps (crawlers)

A DB-backed XML sitemap advertises the public catalog (`handlers::sitemap`).
`GET /sitemap.xml` is a **sitemap index** pointing at child sitemaps:
`/sitemaps/pages.xml` (static + per-game routes, the sealed hubs, each game's flat
sealed-product browse, and the legal pages), `/sitemaps/sets.xml` (every card set,
plus every sealed-catalog set that actually holds products), `/sitemaps/cards-{n}.xml`
(cards), and `/sitemaps/products-{n}.xml` (sealed products). Cards and products are
chunked at 5 000 URLs/file — well under the protocol's 50 000 cap, because Google
timed out fetching the full-size chunks (issues #294, #318). The `<loc>`s are the
SPA's own routes (e.g. `/cards/mtg/sets/blb`, `/sealed/mtg/sets/blb`,
`/sealed/mtg/{id}`), built against `PUBLIC_SITE_URL` — not the API's `/api/...` URLs —
with a `<lastmod>` from the set/card/product `released_at` (a sealed-catalog set page
uses its matching card-set's release date, when one resolves) or the latest sync.
Served at the **site root** so the sitemap-protocol
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
| `GET /api/games/{game}/products/facets` | `{ data: ProductFacets }` — the distinct filter values that actually occur among the game's products, so the SPA builds dropdowns without hardcoding. `ProductFacets = { types: string[], sets: ProductSetRef[] }`; `ProductSetRef = { code, name: string | null, product_count: number }`. `types` alphabetical; `sets` are the sets that have products, in resolved-name-then-code order (a blank set code is excluded), each carrying how many products it has (grouped count, for the sealed-catalog set-landing tiles) |
| `GET /api/games/{game}/products/{id}` | one `Product` |
| `GET /api/games/{game}/products/{id}/image?size` | the product image bytes, proxied + cached from the TCGplayer CDN (`tcgplayer-cdn.tcgplayer.com`, host allow-listed). `size` ∈ `normal` (1000×1000, default) / `small` (200w); the on-disk cache + `Cache-Control: immutable` + `CDN_MODE` behave exactly like the card image proxy |
| `GET /api/games/{game}/products/{id}/prices?range` | `{ data: ProductPricePoint[] }` — the product's price history, **oldest first** (`[]` if none in range). Reuses the exact `?range` windowing/downsampling as the card price endpoint (`api/src/handlers/catalog/pricing.rs`): no `range` = the full daily series, an explicit `range` (`7d`/`30d`/`1y`/`2y`/`3y`/`all`) windows + downsamples it, unknown `range` = `422` |
| `GET /api/games/{game}/products/{id}/cards?page&page_size&section` | page of `ProductCardEntry` — the cards this product is found to contain / can be pulled from, the **reverse** of `.../cards/{id}/sealed` (issue #204). Ordered by membership (`contains` → `booster` → `variable`, so the guaranteed cards lead) and, within the booster pool, **family-exclusive cards first** (a collector booster's special printings no other booster in the set can pull — each flagged `exclusive`, PR #221), then set code + collector number; each card deduped to its **strongest** membership with a foil-only flag. Optional `?section` (`contains`/`exclusive`/`booster`/`variable`) pages just one display section so the SPA paginates each on its own (issue #224); omit it for the whole ordered list — `total`/`has_more` then describe the selected section. Empty page when the product has no ingested contents; `404` for an unknown game/product, `422` for an unknown section |
| `GET /api/games/{game}/products/{id}/cards/sections` | `{ data: ProductCardSection[] }` — the **non-empty** display sections of the cards above, in display order (`contains` → `exclusive` → `booster` → `variable`) with per-section counts, so the SPA knows which independently-paginated blocks to render (issue #224) before fetching any card. `[]` when the product has no ingested contents; `404` for an unknown game/product |
| `GET /api/games/{game}/products/{id}/contents` | `{ data: ProductComponent[] }` — the product's **structural composition** ("what's in the box"): the nested packs/boxes it bundles (each linked to its own product page), precon decks, fixed promo cards (linked to the card), and physical extras, in display order with quantities. Sourced from MTGJSON's sealed-product `contents` via `sealed_components` (with curated fallback). `[]` when the product has no ingested composition (a bare booster pack, or a product neither MTGJSON nor the fallback describes); `404` for an unknown game/product |
| `GET /api/games/{game}/products/{id}/containers` | `{ data: ProductContainer[] }` — the **reverse structural composition**: parent sealed products that directly contain this product, so an individual booster page can link to its booster boxes and bundles. Each entry embeds the parent `Product` plus the quantity of the viewed product it contains; duplicate component lines for one parent are summed. Ordered by parent name; `[]` when no ingested composition references the product; `404` for an unknown game/product |

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

`ProductContainer = { product: Product, quantity }` (the `.../products/{id}/containers`
endpoint) reverses linked `sealed` components: `product` is the direct parent and `quantity`
is how many copies of the viewed child it contains. It uses the same non-recursive
`sealed_components` data as `ProductComponent`; it does not infer case/box relationships
from product names.

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

The collection also holds **sealed products** (issue #435) in the independent
`collection_product_items` table. Product ids on the wire are external TCGplayer ids;
the handler resolves them to `products.id` for storage so rows survive catalog re-syncs.
The UI exposes one quantity while preserving `foil_quantity` (foil sealed variants are
separate TCGplayer SKUs), and both counts zero deletes the row. Collection product routes
share their wire shaping, validation, pagination, and valuation with the wish list through
`handlers/shared/product_holdings.rs`, while each surface keeps its own SeaORM queries and
rows:

| Method & path | Body | Returns |
|---------------|------|---------|
| `GET /api/collection/{game}/products?page&page_size&set` | — | `Page<ProductHoldingEntry>`, most-recently-updated first (fixed recency sort, no `q`/`sort`), default page size 60 / max 200. Optional `set` restricts to one set code (an unknown/unheld code → empty page, `total` 0) — the set-scoped drill-in for the tiles below |
| `GET /api/collection/{game}/products/sets` | — | `{ data: ProductHoldingSet[] }` — every set the user owns sealed products in, **unpaginated**, newest set first (the catalog set's `released_at`, or the newest `released_at` among the set's held products when it has no `card_sets` row; date-less last; ties by code asc). Each is an aggregate tile; drill into one via `?set=<code>` on the flat list above |
| `GET /api/collection/{game}/products/summary` | — | `ProductHoldingSummary { unique_products, total_products, total_value_usd }`; value = regular×`usd` + foil×`usd_foil`, `null` when nothing owned is priced |
| `POST /api/collection/{game}/products/owned` | `{ ids }` | `{ data: { <external id>: { quantity, foil_quantity } } }`; unowned ids absent, > 500 ids `422` |
| `GET /api/collection/{game}/products/{id}` | — | `{ quantity, foil_quantity }`, zeros if not owned; unknown game/product `404` |
| `PUT /api/collection/{game}/products/{id}` | `{ quantity, foil_quantity }` | absolute-count upsert; both-zero deletes, negative/oversized `422`, read-only key `403` |

`ProductHoldingEntry = { product: Product, quantity, foil_quantity }`;
`ProductHoldingSet = { code, name: string | null, unique_products, total_products,
total_value_usd: string | null }` (a held-product-set tile, its aggregates scoped to the one
set, `name` null for a set with no `card_sets` row; drill into a set with `?set=<code>` on the
flat products list). Collection import/sync/export remain card-only; value
history and movers include both card and sealed-product holdings. **Public sharing exposes these
sealed products** through read-only `/api/u/{handle}/{game}/products{,/summary,/sets}` mirrors of
the three authed reads above — handle-resolved + gated by the same per-game visibility flag as the
public card reads (a private/unknown handle → uniform 404), served from the CDN-cacheable
`public_holdings` group (ETag'd, `PUBLIC_HOLDINGS_CACHE`); the wish-list twin has no public
surface.

| Method & path | Body | Returns |
|---------------|------|---------|
| `GET /api/collection/{game}?…&set&include_related` | — | page of `CollectionEntry`, most-recently-updated first (`?page`/`?page_size`, default 60 / max 200) — `{ data, page, page_size, total, has_more }`. Optional `?set=<code>` scopes to one set (ANDed with `q`) — the per-set collection view; with `?include_related=true` the scope spans the set's whole **group** (root + related sub-sets), the collection mirror of the catalog's `include_related` (resolved via the same `group_set_codes`) |
| `GET /api/collection/{game}/summary?set&include_related` | — | `CollectionSummary` `{ unique_cards, total_cards, total_value_usd, bulk_value_usd }` (see below). Optional `?set=<code>` scopes the stats to one set; `?include_related=true` (with a set) spans the set's whole **group** (root + related sub-sets, same `group_set_codes` as the list) so the value matches the include-related browse view. Backs the scoped collection value shown next to the browse count (issue #119) |
| `GET /api/collection/{game}/value-history?range` | — | `{ data: CollectionValuePoint[] }`, oldest first, with separate card and sealed-product value lines (see below). No `range` = the full daily series; `7d`/`30d`/`1y`/`2y`/`3y`/`all` windows and downsamples like item price history; unknown range `422`. |
| `GET /api/collection/{game}/movers?window` | — | `CollectionMovers` keeps the card series and a parallel `sealed` series with the same windows — each contains its own five largest holding-value gainers/losers for 1d / 7d / 30d / 1y / 2y / 3y / all captured history (see below). An empty latest-day comparison retries from the previous available snapshot. No `window` = every window (the original response); an optional `window` (`day`/`week`/`month`/`year`/`two_year`/`three_year`/`all_time`) computes only that date range on demand — the requested window is populated for both the card and `sealed` series while the rest come back empty (the `as_of` reference dates are always returned); unknown `window` `422`. |
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

`CollectionValuePoint = { date, value_usd, sealed_value_usd }`: `value_usd` is the card
holding value and `sealed_value_usd` is the sealed-product holding value, each as a 2-dp USD
string or `null` until that holding kind has a captured price on/before the day. Both lines
share the union of their snapshot dates and independently carry their last captured prices
forward. Each revalues the user's **current** quantities across every captured historic date,
regardless of when the holding was added. Quantity changes before today are not stored, so the
graph intentionally answers what the current basket would have been worth at historic prices,
not what the user actually owned on each date.

`CollectionMovers = { as_of, day_as_of, day, week, month, year, two_year, three_year,
all_time, sealed }`:
`as_of` is the newest `YYYY-MM-DD` price snapshot across the user's priced card holdings, or
`null` when none has history. `day_as_of` is the date used by the 1d comparison: normally
`as_of`, but when that comparison has no non-zero movers it retries from the previous
available snapshot and, when the retry finds movement, reports that date instead (a retry
that also finds nothing keeps `day_as_of` at `as_of`). Every window is a required
`CollectionMoverList = { gainers, losers }`; both arrays are value-change ordered and capped
at five. A `CollectionMover` is
the backward-compatible card shape `{ card, quantity, foil_quantity, value_now, value_prev,
change_usd, change_pct }`. `sealed` is an independent `CollectionSealedMovers` with its own
`as_of`, `day_as_of`, and the same seven window keys; its lists contain
`CollectionSealedMover` rows with `product` in place of `card`. Singles and sealed products
do not compete in one ranking; the SPA switches between the two series. The three values are
2-dp USD strings and `change_pct` is null when the baseline value is zero.
The optional `?window=<day|week|month|year|two_year|three_year|all_time>` scopes the whole
computation to one date range: only that window's lists are ranked (for both series) and the
rest are returned empty, so a landing that only ever shows its default window seeks ~two
anchors per held item instead of ten and skips the all-time earliest-price scan. `as_of` /
`day_as_of` (and their `sealed` counterparts) are computed regardless of window, so the panel
still renders; omitting the param keeps the original all-windows response for API-key
consumers. Each window caches independently under the same holdings/price version keys. The SPA
passes the active window (the window selector then fetches on demand and caches per window,
while the Singles/Sealed switch stays a client-side toggle because both series ship together).
Movement is the per-finish unit-price change multiplied by the user's current regular/foil
counts. Fixed windows carry forward the latest snapshot at or before their calendar baseline
measured back from `as_of`; the 1d fallback alone measures from `day_as_of`. A finish
participates only when both endpoints have a price. `all_time` instead compares each finish
with its own earliest non-null captured price, so a newer catalog item is not excluded by an
older item's history. A holding kind with no captured history has null `as_of` / `day_as_of`
and fourteen empty arrays, independently of the other kind.

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
only what it costs matters — and ignores the bulk slice.) The **card** routes below each
mirror their collection twin exactly (params, ordering, errors, caps):

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

The wish list additionally holds **sealed products** (issue #364) —
`(user, game, product) → { quantity, foil_quantity }` over its own, fully independent
`entities/wishlist_product_item.rs` (`wishlist_product_items`, unique on
`(user_id, game, product_id)`, `user_id` FK → `users` `ON DELETE CASCADE`). This is
independent from the collection's matching product surface introduced by issue #435.
`{id}` is the external (TCGplayer) product id, resolved to the internal `products.id` on
write so a row survives catalog re-syncs (the daily TCGCSV sweep is upsert-only); both
counts zero deletes the row. The wire keeps the shared two-count
`{ quantity, foil_quantity }` shape (foil sealed variants are separate TCGplayer SKUs),
but the UI exposes a single **Quantity** and preserves an existing foil count. Both
surfaces use `ProductHoldingEntry` (`{ product: Product, quantity, foil_quantity }`),
`ProductHoldingSummary` (`{ unique_products, total_products, total_value_usd }`), and
`ProductHoldingSet` (the held-product-set tile) from `handlers/shared/product_holdings.rs`:

| Method & path | Returns |
|---------------|---------|
| `GET /api/wishlist/{game}/products?page&page_size&set` | `Page<ProductHoldingEntry>`, most-recently-updated first (fixed recency sort, no `q`/`sort`), `page`/`page_size` default 60 max 200. Optional `set` restricts to one set code (unknown/unheld code → empty page, `total` 0), the drill-in for the tiles below |
| `GET /api/wishlist/{game}/products/sets` | `{ data: ProductHoldingSet[] }` — every set the user wants sealed products in, **unpaginated**, same grouping/order as the collection `products/sets` above (newest set first; date-less last; ties by code asc); each an aggregate tile drilled into via `?set=<code>` on the flat list |
| `GET /api/wishlist/{game}/products/summary` | `ProductHoldingSummary { unique_products, total_products, total_value_usd }`; value = regular×`usd` + foil×`usd_foil` (market prices; msrp never used); `total_value_usd` null when nothing wanted is priced; no set scope, no bulk split |
| `POST /api/wishlist/{game}/products/counts` `{ ids }` | `{ data: { <external id>: { quantity, foil_quantity } } }` — batch wanted counts for the product-tile badges; un-wanted ids absent (never zero); > 500 ids = `422` |
| `GET /api/wishlist/{game}/products/{id}` | the single-product wanted counts (`CollectionQuantities`; zeros if absent, `404` unknown game/product) |
| `PUT /api/wishlist/{game}/products/{id}` `{ quantity, foil_quantity }` | the absolute-count upsert (both-zero removes, negative/oversized `422` before product resolution, read-only key → `403`) |

The wish list and collection never read or write each other's card or product rows
(pinned by security tests); browse-grid badges on each surface come from its own batch
counts route.

**Public wish-list sharing (issue #493).** A wish list can be made public independently of
the collection, through a separate per-`(user, game)` flag (`wishlist_is_public`, a new column
on the same `collection_visibility` row — the collection's `is_public` twin). The authed toggle
mirrors the collection's:

| Method & path | Body | Returns |
|---------------|------|---------|
| `GET /api/wishlist/{game}/visibility` | — | `WishlistVisibility { public, handle }` — whether this game's wish list is public + the caller's handle (null until a username is set). Defaults `public: false` when no row exists. `AuthUser` |
| `PUT /api/wishlist/{game}/visibility` | `{ public? }` | `WishlistVisibility` — flip the flag. Enabling public **requires a username first** (a public wish list is addressed by handle) — else **409** (the SPA prompts the username step). `WritableUser`, so a read-only key is 403. Touches only the wish-list flag, never the collection's sharing or landing display prefs on the same row |

When public, a read-only view of the owner's wanted cards **and** sealed products is served,
handle-resolved under a static `wishlist` segment (`/api/u/{handle}/wishlist/{game}...`, which
outranks the `{game}` capture in axum, like `decks`), gated on `wishlist_is_public` — a
private/unknown handle is a uniform **404** (no oracle). These mirror the authed reads above
(and the public collection reads' cache policy: CDN-cacheable + ETag'd `public_holdings`, except
the body-keyed `/owned` POST which is `no-store`):

| Method & path | Mirrors the authed |
|---------------|--------------------|
| `GET /api/u/{handle}/wishlist/{game}` | `GET /api/wishlist/{game}` |
| `GET /api/u/{handle}/wishlist/{game}/summary` | `.../summary` |
| `GET /api/u/{handle}/wishlist/{game}/sets` | `.../sets` |
| `GET /api/u/{handle}/wishlist/{game}/sets/{code}/{drops,subtypes}` | `.../sets/{code}/{drops,subtypes}` |
| `GET /api/u/{handle}/wishlist/{game}/products{,/summary,/sets}` | `.../products{,/summary,/sets}` |
| `POST /api/u/{handle}/wishlist/{game}/owned` `{ ids }` | `.../counts` (show-ghosts overlay; `no-store`) |

The public **profile** (`GET /api/u/{handle}`) gains a `wishlists: PublicGameSummary[]` array
alongside its `games` (public collections); a user who has shared **only** a wish list still
resolves (200), and the profile 404s only when nothing — no public collection, wish list, or
deck — is shared.

## Decks API contract

Per-user **decks** (issue #363) — build and organise decks of cards for a game.
Authenticated (`Authorization: Bearer <access_token>`, via `AuthUser` reads /
`WritableUser` writes), game-namespaced under `/api/decks/{game}`, in the router's
`private` group (so every response is `Cache-Control: no-store` and per-user rate
limited). Unlike the collection / wish list (one implicit list per `(user, game)`), a
user has **many** decks, so the routes nest a `{deck_id}` and add CRUD verbs. Card ids in
a path are the **external** card id, resolved to the internal `cards.id` on write (so a
deck card survives a catalog re-import — same as the collection).

A deck is three tables: `entities/deck.rs` (`decks` — the deck row, carrying `name`,
`description`, `format`, a nullable `folder_id`, and an `is_public` flag), `deck_section.rs`
(`deck_sections` — the Archidekt-style categories a deck's cards are filed into, one row per
`(deck, name)`, ordered by `position`), and `deck_card.rs` (`deck_cards` — one row per
`(deck, card, section)`, the same two-count `{ quantity, foil_quantity }` shape as a holding
so it reuses the shared valuation/summary machinery; both counts zero deletes the row). Decks
are grouped at the deck level into `deck_folder.rs` (`deck_folders`). All four cascade-delete
from `users`; deleting a deck cascades its sections + cards, and deleting a folder **ungroups**
its decks (`folder_id → NULL`) rather than deleting them.

**Ownership.** A `deck_card`/`deck_section` has no `user_id` — it hangs off `deck_id` — so
every deck-scoped route first proves the parent deck belongs to the token user; a deck (or
folder/section) that isn't the caller's is a **404** (never 403 — no existence oracle over
deck ids), matching the public-sharing surface.

| Method & path | Body | Returns |
|---------------|------|---------|
| `GET /api/decks/{game}` | — | `{ data: Deck[] }` — the user's decks, most-recently-updated first (not paginated; a user has few decks). Each `Deck = { id, game, name, description, format, folder_id, is_public, card_count, created_at, updated_at }` (`card_count` = total copies across all sections) |
| `POST /api/decks/{game}` | `{ name, description?, format?, folder_id? }` | `DeckDetail` — creates a deck **seeded with the default sections** and returns its full detail. `422` blank/oversized name or over the per-game cap (1000); `404` if `folder_id` isn't one of the caller's folders |
| `POST /api/decks/{game}/import` | `{ provider, source, contents, format, name, auto_categorize }` | `DeckImportResponse { deck: Deck, provider, total_rows, matched_cards, unmatched_cards, unmatched_sample }` — creates a new deck from exactly one source: a public deck URL/id (`source`; Archidekt live import) or uploaded file text (`contents`; Archidekt CSV or Moxfield CSV/plain text). `deck` is the lightweight list header; load `GET /api/decks/{game}/{deck_id}` for sections/cards. The unused source fields are `null`. Explicit provider categories/boards become deck sections. `auto_categorize` defaults to `true` when omitted and files generic Mainboard rows into the matching preset type section; set it to `false` to preserve Mainboard exactly. `422` for malformed/empty/zero-match sources or more than 2000 source rows; nothing is created on failure |
| `GET /api/decks/{game}/{deck_id}` | — | `DeckDetail` — the full deck: metadata, the owner handle, a value summary, every section in order, and every card (returned whole — a deck is bounded — so the SPA groups `cards` by `section_id`). `404` if not the caller's |
| `GET /api/decks/{game}/{deck_id}/export?format=archidekt\|moxfield\|moxfield-text` | — | Owner-scoped deck-list download. `archidekt` (default) and `moxfield` return CSV; `moxfield-text` returns a sectioned plain-text list. Regular and foil quantities are separate rows, and sections round-trip through each format — a text section header the plain-text grammar would misread (a leading quantity like `2 Drops`, a leading `#`, or edge `~ / : [ ]` characters) is emitted wrapped in one bracket pair (`[2 Drops]`), which the importer strips back off verbatim. Two text-format caveats: line breaks inside a section name flatten to spaces, and a name that reduces to a standard board alias (`Deck`, `Considering`, …) re-imports as that board. `404` if not the caller's |
| `PUT /api/decks/{game}/{deck_id}` | `{ name, description?, format? }` | `Deck` — replace the deck's editable metadata (folder + sharing are their own routes) |
| `DELETE /api/decks/{game}/{deck_id}` | — | `204` — delete the deck (sections + cards cascade) |
| `PUT /api/decks/{game}/{deck_id}/folder` | `{ folder_id }` | `Deck` — file the deck under a folder, or `null` to loosen it (`404` for a folder that isn't the caller's) |
| `PUT /api/decks/{game}/{deck_id}/visibility` | `{ public }` | `DeckVisibility { public, handle }` — enable/disable public sharing. Enabling **requires a username first** (a public deck is addressed by handle) — else **409** (the SPA prompts the username step). `WritableUser`, so a read-only key is 403 |
| `GET /api/decks/{game}/folders` | — | `{ data: DeckFolder[] }` — the user's folders (alphabetical), each `DeckFolder = { id, name, deck_count }` |
| `POST /api/decks/{game}/folders` | `{ name }` | `DeckFolder` — create a folder. `409` if the name already exists; `422` over the cap (500) |
| `PUT /api/decks/{game}/folders/{folder_id}` | `{ name }` | `DeckFolder` — rename |
| `DELETE /api/decks/{game}/folders/{folder_id}` | — | `204` — delete (its decks are ungrouped, not deleted) |
| `POST /api/decks/{game}/{deck_id}/sections` | `{ name }` | `DeckSection { id, name, position }` — add a custom section (appended). `409` duplicate name; `422` over the per-deck cap (200) |
| `PUT /api/decks/{game}/{deck_id}/sections/{section_id}` | `{ name?, position? }` | `DeckSection` — rename and/or reposition |
| `DELETE /api/decks/{game}/{deck_id}/sections/{section_id}` | — | `204` — delete a section, **moving its cards** to the deck's first remaining section (merging counts on a collision). `409` if it's the deck's only section (a deck must keep ≥ 1) |
| `PUT /api/decks/{game}/{deck_id}/sections/reorder` | `{ section_ids }` | `{ data: DeckSection[] }` — set the section order; `section_ids` must be exactly the deck's sections (`422` otherwise) |
| `PUT /api/decks/{game}/{deck_id}/cards/{id}` | `{ quantity, foil_quantity, section_id }` | `{ quantity, foil_quantity }` — set the absolute counts for a card in one section (both zero removes it there). `404` unknown deck/section/card, `422` negative/oversized |
| `PUT /api/decks/{game}/{deck_id}/cards/{id}/move` | `{ from_section_id, to_section_id }` | `{ quantity, foil_quantity }` — move a card between two of the deck's sections (merging counts on a collision) |
| `PUT /api/decks/{game}/{deck_id}/cards/{id}/printing` | `{ new_card_id, section_id }` | `{ quantity, foil_quantity }` — atomically replace a card with another printing of the same gameplay card in that section, preserving finish counts (or merging when the target printing is already present). Serialized with count-set and section-move writes through the parent deck row. `404` unknown/non-owned source row, `422` unrelated target card |

`DeckDetail = { id, game, name, description, format, folder_id, is_public, handle, summary,
sections, cards, created_at, updated_at }` — `summary` is the shared `CollectionSummary`
(reused for the value/copy aggregates; the deck UI ignores the bulk slice), `sections` are
`DeckSection[]` in display order, and `cards` are `DeckCardEntry[]` (`{ card, section_id,
quantity, foil_quantity }`, the full catalog `Card` plus which section it sits in — a
deck-specific DTO, since a `CollectionEntry` has no section). The default seeded sections are
Archidekt-flavoured (Commander, Creatures, …, the functional categories Ramp / Removal /
Tutor / …, and Maybeboard). The SPA's default add target uses the front-face card type to
pick the matching preset type bucket, while an explicit section choice always wins — see
`docs/tradeoffs.md`. Unknown types use a Mainboard/Other catch-all when present; otherwise
the SPA requires an explicit section instead of defaulting to the deck's first section.

Deck imports run inline because each provider deck endpoint is a single-object fetch, unlike
the paginated collection job. They still share the collection importer's per-provider rate
limiter, `429` back-off, foil detection, external-id resolution, and Moxfield
`(set, collector_number)` lookup. Archidekt uses the first category as the card's primary
section; Moxfield boards map to Mainboard / Sideboard / Commander / Maybeboard / Companion /
Signature Spells, with per-board fallback for older top-level payloads. With the default
`auto_categorize: true`, only generic Mainboard rows are re-filed into Creatures / Artifacts /
Enchantments / Instants / Sorceries / Planeswalkers / Lands; explicit categories remain exact.
Name-only plain text resolves to the newest exact-name printing; an explicit printing tuple
that is absent from the local catalog stays unmatched. The write is one transaction and goes
straight to `decks` / `deck_sections` / `deck_cards` — it never invokes collection reconcile.

Moxfield **live URL** import uses the same deployment gate as collection import and remains
disabled until the operator has an approved `MOXFIELD_USER_AGENT`; upload is the supported
Moxfield path. Upload bodies are capped at 16 MiB and deck lists at a deck-specific 2000-row
cap. The synchronous response stays bounded by returning a `Deck` header, never every card.
CSV parsers accept the provider's relevant columns plus extra export metadata. The app's own
minimal exports are deliberately re-importable: Archidekt uses Quantity / Name / Finish /
Scryfall ID / Categories, while Moxfield uses Count / Name / Edition / Foil / Collector
Number / Board.

**Public deck sharing** reuses the collection's handle namespace (issue #361's model, but per
deck): a deck's `is_public` column exposes a read-only view, and these token-less reads live in
the CDN-cacheable `public_holdings` router group (ETag'd, `PUBLIC_HOLDINGS_CACHE`). Deck ids are
globally unique, so no game segment is needed. Every miss — unknown handle, or a private/absent
deck — is the same **404** (no existence oracle), and the public `DeckDetail` carries only the
owner handle (no email/PII).

| Method & path | Returns |
|---------------|---------|
| `GET /api/u/{handle}/decks` | `{ data: Deck[] }` — the owner's public decks (across games), newest first. `404` when the handle is unknown **or** the user has no public deck (non-oracle, like the public profile) |
| `GET /api/u/{handle}/decks/{deck_id}` | `DeckDetail` — one public deck (the shareable view). `404` if private/absent |

## Dataset mirror

Optional public endpoints (`handlers::mirror`), wired **only when `MIRROR_ENABLED=true`**
(off by default — an enabled mirror is a public read proxy to the upstream dataset
hosts; rationale + trade-offs in `docs/tradeoffs.md`). Each dataset route streams the file
from its fixed upstream host on demand — no disk persistence, the same fetch-and-serve
model as `CDN_MODE` — stamping its own CDN-cacheable `Cache-Control` (preserved by the shared
`public_cache_layer`). A fronting CDN only *absorbs* these repeats once a **Cache Rule**
marks `/api/mirror/*` edge-eligible — the deploy guides fold it into the honor-origin
catalog rule ([self-hosting.md](./self-hosting.md#behind-a-cdn-cloudflare) *Behind a CDN (Cloudflare)*); without that rule Cloudflare bypasses
`/api/…` by default, so every consumer pull re-streams from upstream. The
`kind`/path inputs are sanitised (host-locked, no traversal). The MTGJSON route forwards
`If-None-Match` and relays the upstream `304`, so an unchanged file stays a cheap
conditional.

TCGCSV's dated price **archives** (`archive/…`) are the one mirror route cached
**`immutable`** (a year): `prices-{date}.ppmd.7z` is fixed once published, unlike the live
JSON beside it. The short meta TTL was actively wrong there — the one-time historic price
backfill walks ~900 archive days and **each day is a distinct URL fetched exactly once**
per consumer, so no shared cache ever serves a repeat *within* one walk, and a one-hour TTL
has long expired before the next self-host walks. Pinning a *missing* day is not a risk: an
unpublished day is a `404`, which `public_cache_layer` marks `no-store`.

| Method & path | Returns |
|---------------|---------|
| `GET /api/mirror/scryfall/bulk-data` | Scryfall's bulk-data catalog JSON |
| `GET /api/mirror/scryfall/sets` | Scryfall's sets listing |
| `GET /api/mirror/scryfall/file/{kind}` | the named Scryfall bulk file (`kind` validated) |
| `GET /api/mirror/scryfall/sld-drops` | the current Secret Lair drop snapshot (curated titles + collector numbers) as JSON, served from this origin's in-memory drop store (a strong content `ETag`, so an unchanged snapshot is a `304`) |
| `GET /api/mirror/mtgjson/AllPrintings.json.gz` | MTGJSON's `AllPrintings` gzip (ETag-conditional) |
| `GET /api/mirror/tcgcsv/{*path}` | the TCGCSV path proxied through (catalog / prices / daily archives; `archive/…` is cached a year `immutable`, everything else keeps the 1-hour meta TTL) |
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

The **sld-drops** route is likewise not an upstream proxy: it re-serves this origin's own
in-memory Secret Lair drop snapshot as JSON (the shape of `scryfall/sld_drops.json`). Those
curated drop titles aren't in the bulk card API, so the **mirror origin** (`MIRROR_ENABLED`)
scrapes Scryfall's gallery daily (`scryfall::sld_scrape`) and installs the fresh snapshot into
its drop store; every **other** instance imports it from this route daily
(`scryfall::sld_sync`, on by default via `SLD_DROPS_IMPORT_ENABLED`) rather than scraping
Scryfall itself. Both fall back to the committed `sld_drops.json` until their first fetch. A
scrape that yields no drops (a markup change) is rejected, so a broken scrape never wipes the
good table. Version-gated by a strong content `ETag` (a bodyless `304` when unchanged).
