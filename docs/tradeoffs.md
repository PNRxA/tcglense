# Known trade-offs & future work

This is the on-demand detail companion to `CLAUDE.md`. `CLAUDE.md` is the always-loaded
core (kept deliberately slim); this file is the full rationale reference for every deliberate
trade-off, residual edge case, and "why it's built this way" essay that used to live in
`CLAUDE.md`'s "Known trade-offs / future work" section. Read it when you need the *why*
behind a design decision, the exact failure modes a feature tolerates, or the caveats a
production deploy must account for. Grouped for navigability; the content is otherwise
carried over near-verbatim from the audited source, with the sealed-product provider
(TCGCSV) trade-offs added.

**Not yet built:** set-completion progress (per-set completion tracking against the
catalog) is planned but not implemented.

---

## Auth & sessions

- **Token storage:** the refresh token is an `HttpOnly` cookie (not readable by JS)
  and the access token is held in memory only, so an XSS can't exfiltrate the
  long-lived credential. In production set `COOKIE_SECURE=true` (HTTPS) and serve
  web + API same-origin (or configure cross-origin CORS credentials).
- **Atomic rotation:** `/refresh` serializes one user's session mutations, then claims
  the presented token, checks reuse/session generation, creates and links the successor,
  and (when needed) revokes the login family in one transaction. A disconnect or DB
  failure rolls the whole rotation back instead of stranding a revoked predecessor.
  The remaining transport edge case is a fully committed rotation whose `Set-Cookie`
  never reaches the browser; the client reduces that window with `keepalive: true` on
  the refresh POST. The server cannot safely "heal" that state because it is
  indistinguishable from normal predecessor replay while a live successor exists.
- **Concurrent refresh:** reuse detection is lineage-based (a token records its
  successor and login family) so a benign concurrent double-submit does not burn the
  family — only replay of a token whose successor was itself consumed does. Reuse
  revokes that login/device family, never a separately-created session. The client
  single-flights within a tab and uses the Web Locks API where available to serialize
  refresh-cookie mutation across tabs; unsupported browsers retain the server's safe
  concurrent-submit behavior but may need to restore again.
- **Password-reset invalidation:** access and refresh credentials capture a per-user
  session generation. Reset increments it transactionally, invalidates sibling reset
  links, and revokes refresh rows under the same per-user serialization, so an in-flight
  rotation cannot survive the sweep and already-minted access JWTs stop working without
  waiting for their 15-minute expiry. The same transaction also soft-revokes **every
  programmatic `tcgl_` API key** the user holds (`api_key::revoke_all_for_user`): a key
  carries no session generation — it resolves purely on `revoked_at`/`expires_at` — so
  without this, a key an attacker minted while holding a compromised session (key
  creation only needs a live `SessionUser`) would outlive the victim's recovery. Account
  recovery therefore invalidates *all* credential classes together, not just the browser
  session.
- **Refresh-token pruning:** a background task deletes rows past `expires_at` every
  6h so the table can't grow unbounded; revoked-but-unexpired rows are retained so
  reuse detection still works.
- **Gotcha — `jsonwebtoken`:** v10 needs a crypto provider feature or it panics at
  runtime; this crate pins `default-features = false, features = ["aws_lc_rs"]`,
  sharing rustls's provider and avoiding unused RSA dependencies. Don't enable a
  second provider or drop this one when bumping it.

## Public API keys (issue #284)

- **Same routes, not a new surface.** API keys authenticate the *existing*
  collection/wish-list endpoints rather than a duplicated `/api/v1/...` tree. The
  whole design is: teach the one `AuthUser` extractor to also resolve a `tcgl_`-labelled
  bearer credential to a user, and every per-user endpoint accepts a key for free.
  The alternative (a parallel public router) would double the wire surface and drift.
- **`Bearer tcgl_…`, not `X-Api-Key`.** Reusing the `Authorization: Bearer` header
  means zero CORS changes (`AUTHORIZATION` is already allow-listed) and one credential
  path. A JWT is three base64url segments and can't start with `tcgl_`, so the two
  never collide — the extractor branches on the label.
- **SHA-256, not argon2.** A key is 32 CSPRNG bytes — already uniformly random — so a
  fast hash is correct (argon2 is for low-entropy passwords) and, crucially, lets the
  auth path resolve a presented key with a single indexed lookup on `token_hash`. The
  plaintext is returned once and only the hash is stored (mirrors refresh/email tokens).
- **Scope enforced by extractor, not by method.** `read` vs `read_write` can't be a
  pure HTTP-method check because `POST /owned` and `POST /counts` are *reads*. So
  reads take `AuthUser` (session or any key), writes take `WritableUser` (session or a
  `read_write` key; a read-only key → **403**), and this is explicit per handler. A
  bad/expired/revoked key is **401** (invalid credential); a valid read-only key on a
  write is **403** (valid but unauthorized) — the two are deliberately distinct.
- **Keys can't manage keys.** Management (`/api/auth/api-keys`) uses `SessionUser`
  (JWT only) so a leaked key can neither mint more nor revoke its siblings — a
  compromised key is contained to *using* the API, and the real session can revoke it.
- **Rate-limit parity.** The per-user limiter had to be taught to resolve a `tcgl_`
  token to its user id (one extra indexed lookup, only for key traffic — a JWT still
  takes the pure-crypto fast path), or keyed requests would silently bypass the
  per-user quota. Extracting the key token *before* the `await` keeps that middleware
  future `Send` (a `&Request` held across the await isn't, since axum's `Body: !Sync`).
- **Soft revoke + optional expiry + a per-user cap (25).** Revocation is a `revoked_at`
  stamp (audit trail; an in-flight request sees it), expiry is optional per key, and
  dead (expired/revoked) rows are pruned by the same 6h maintenance loop as the other
  token tables. `last_used_at` is a best-effort, ≤once/60s throttled write so a busy
  key doesn't pay a DB write per request. The cap is **best-effort**: creation counts
  live keys, inserts, then re-counts and rolls back (soft-revokes the just-minted key,
  before its plaintext is returned) if the insert raced past the cap — so a concurrent
  burst converges to 25 rather than being hard-blocked by a transaction/row-lock on the
  hot path. A password reset revokes every one of the user's keys (see
  §Password-reset invalidation).

## Rate limiting & anti-abuse

- **Auth anti-abuse (CAPTCHA + rate limiting):** the auth mutation endpoints are
  guarded by Cloudflare Turnstile (`captcha.rs`) and per-IP rate limiting
  (`ratelimit/per_ip.rs`, `governor`). The rate limiters are **in-memory by default**; set
  `REDIS_URL` to back them with Redis so the keyspace is **shared across instances** (a
  multi-instance deploy then enforces one global quota). On a Redis outage the limiter
  **fails open**, degrading to in-memory for that check (rate limiting is abuse
  protection, not authn — CAPTCHA still fails closed). The client-IP
  resolution trusts proxy headers only when `TRUST_PROXY_HEADERS=true`, so behind
  a proxy that env **must** be set or every client keys as the proxy's IP. When it
  IS set, the left-most `X-Forwarded-For` is used, which is only safe if the proxy
  **replaces/strips** any inbound header (an appending proxy lets a client spoof
  the left-most entry); a multi-hop CDN chain would need a trusted-proxy list
  (future work). IPv6 clients are keyed by their **/64** (not /128) so a client
  can't rotate source addresses within its block to dodge the limit.
  CAPTCHA is disabled by default (no `TURNSTILE_SECRET_KEY`); rate limiting is enabled
  but fails open when the client IP is unresolvable (for example in-process tests).
  New signups are closed by default, and an internet-facing server refuses to open
  them without both Turnstile keys, verified-domain email configuration, HTTPS, and
  secure cookies. A production proxy must also set `TRUST_PROXY_HEADERS` safely.
  Login is still not
  account-locked (per-IP only), so distributed credential-stuffing from many IPs
  isn't fully mitigated; the token cooldown (`issue_with_cooldown`, an atomic
  conditional insert) remains the per-user email-issue brake underneath the per-IP
  limit.
- **Per-user rate limiting (issue #168):** the *authenticated* API surface (the
  `/api/collection/*` + `/api/wishlist/*` endpoints + `GET /api/auth/me`) also carries a per-user limit
  (`ratelimit/per_user.rs`'s `UserRateLimiters` + the `user_rate_limit` middleware), keyed by
  the access-token user id rather than the IP, so it caps what a single account can
  do no matter how many IPs it comes from — the per-user complement to the per-IP
  auth limits above. Three classes (`ratelimit/per_user.rs`'s `UserRoute`): a generous
  `general` bucket (reads/edits/batch lookups, ~300/min); a middle `analytics` bucket
  (~30/min) for the whole-collection × full-price-history reads and the CSV export —
  `GET …/collection/{game}/value-history`, `…/movers`, `…/export` — which read
  whole-collection price data (up to O(cards × captured days) for a wide value-history
  window; the movers/cutoff anchors are per-item point seeks since the 2026-07 rewrite —
  §Price history) and are `no-store` (no CDN shields them), so one account
  can't sustain full-history revaluation scans against the weak prod Postgres, while a
  human flipping chart ranges in a burst still fits; and a tight `import` bucket (the
  expensive import/sync/CSV-upload endpoints, ~10/min). Over-limit is `429` +
  `Retry-After` + `no-store`. It gates on
  the same `RATE_LIMIT_ENABLED` switch and shares the per-IP limiter's caveats:
  in-memory by default, or Redis-backed via `REDIS_URL` (shared across instances, same
  fail-open posture), and it only engages for a request carrying a *valid* bearer token (an
  unauthenticated / bad-token request has no user to key on and is left to the
  handler's own `401`, so it is **not** an IP-level DoS guard for those routes —
  that would still need the per-IP/WAF layer). Since issue #413 the **per-IP**
  limiter also covers the DB-query public surfaces with generous quotas: the
  catalog reads (search/autocomplete/set/product pages, ~300/min) and the public
  sharing reads + the body-keyed `/owned` POST (~120/min — previously the one
  wholly-unthrottled, uncacheable DB endpoint). The image/icon proxies and the
  import-status poll are deliberately classified out (a legitimate grid page
  fires dozens of art requests; the SPA polls status several times a second), so
  the image routes keep the open posture noted under image caching.

## Browser security headers (CSP)

- **The shipped Content-Security-Policy is narrow — `base-uri 'self'; object-src 'none';
  frame-ancestors 'none'` — and deliberately has no `script-src`/`default-src` yet.** It
  is set both by the API's `security_headers_middleware` (`router.rs`) and repeated in
  every deploy Caddyfile, and it stops clickjacking, `<base>` hijacking, and plugin
  embeds, but it does **not** constrain script execution. The token-storage model already
  resists XSS exfiltration (the access token is memory-only; the refresh cookie is
  `HttpOnly` + `SameSite=Lax`), and the SPA is Vue with default output escaping and no
  known injection sink — so a full `script-src` is **defence-in-depth, not a fix for a
  live bug**. It is intentionally deferred rather than shipped blind because getting it
  wrong breaks the app for everyone, and one interaction can't be verified without a
  live prod deploy: **Cloudflare Turnstile** on the auth forms loads
  `challenges.cloudflare.com` script + iframe, so the enforced policy must allow-list
  exactly those origins or *signups themselves break*. The rollout, post-launch: relocate
  the inline theme `<script>` in `web/index.html` to a static file (so no per-build hash
  is needed), then serve — on the SPA's HTML responses only, not the API JSON — a full
  policy (`default-src 'self'; script-src 'self' https://challenges.cloudflare.com;
  frame-src https://challenges.cloudflare.com; connect-src 'self'; img-src 'self' data:;
  style-src 'self' 'unsafe-inline'; base-uri 'self'; object-src 'none';
  frame-ancestors 'none'`), first as `Content-Security-Policy-Report-Only` (which never
  blocks) with a report sink, then flip to enforcing once the report stream is clean.
  The card scanner needs no third-party or `blob:` allowance in that policy: its
  tesseract.js worker, wasm cores, and traineddata are self-hosted and the worker loads
  as a plain same-origin script (issue #451, §Visual card scanner) — so `'self'` covers
  every scanner URL; do **not** re-add `cdn.jsdelivr.net` or `blob:` for it. The policy
  as quoted does still need one addition for the scanner: `'wasm-unsafe-eval'` in
  `script-src`, because OpenCV.js compiles wasm from embedded bytes on the **main
  thread** (under the document's policy). The tesseract cores compile embedded wasm the
  same way *inside the worker* — fine if the CSP header stays scoped to HTML responses
  (a network-URL worker takes its policy from its **own** response, and inherits nothing
  from the document), but the blanket Caddy `header` blocks stamp every response today,
  so either scope them or keep `'wasm-unsafe-eval'` load-bearing for the worker too.
  Apply it at the edge (the four Caddyfiles) and in `spa_headers_middleware`; keep the
  API's narrow policy for JSON. **Not** applied to the Vite dev server (its HMR needs
  `'unsafe-inline'`/`'unsafe-eval'`), so this is a production-HTML concern only.

## Postgres dual-backend

- **Optional Postgres (dual-backend).** `DATABASE_URL`'s scheme picks SQLite (default)
  or Postgres at runtime — both drivers compile in, no cargo feature. SQLite behaviour is
  unchanged. The whole non-search backend is SeaORM query-builder (portable); the search
  compiler, `auth::email_token::issue_with_cooldown` (raw SQL — placeholder renumbering
  plus a PG `pg_advisory_xact_lock` to reproduce SQLite's single-writer atomicity), and
  two migrations (m001's case-insensitive email index, m017's nullable `password_hash`)
  were made backend-aware via a `db::Dialect` seam. Postgres pool sizing
  is `DB_*` env vars (SQLite stays sea-orm's forced single connection). The default test
  suite + CI stay on in-memory SQLite; Postgres/Redis correctness is covered by opt-in
  `#[ignore]` integration tests (`src/integration_pg.rs` + the Redis tests in
  `ratelimit/backend.rs`, gated on `TCGLENSE_TEST_POSTGRES_URL` / `TCGLENSE_TEST_REDIS_URL`) run
  locally against docker (`deploy/docker-compose.yml`) and in the `postgres-redis-tests`
  CI job. Three deliberate cross-backend divergences, all a consequence of keeping SQLite
  byte-identical rather than forcing a shared behaviour: (1) text **sort order**
  (`order:name`/`set`/`artist`, the sealed-**products** name sort, and the `card-names`
  alphabetical tiebreak) follows each backend's default collation — SQLite `BINARY` (byte order) vs Postgres's DB locale — a
  cosmetic display-order difference only, no pagination impact; (2) the `/regex/` filter
  runs the Rust-regex `REGEXP` UDF on SQLite vs POSIX ARE `~*` on Postgres, which parse
  exotic constructs (`\d`, `\b`, lookaround, lazy quantifiers) differently, so a given
  regex can match different rows per backend (a Rust-valid pattern POSIX rejects is a 422,
  same as any bad pattern — via the SQLSTATE `2201B` mapping in `error.rs`); and (3)
  case-insensitive matching folds the column with the backend's `LOWER()` while the
  pattern is only ASCII-lowercased, so Postgres's locale-aware fold matches non-ASCII case
  (é↔É) that SQLite's ASCII-only fold does not. (2) and (3) are narrow result-set
  differences confined to exotic-regex and non-ASCII inputs; ordinary ASCII searches are
  identical across backends.

## Transactional email (Resend / Cloudflare)

- **Providers are interchangeable, pick one:** the `Emailer` enum sends through either
  [Resend](https://resend.com) (`RESEND_API_KEY`) or [Cloudflare Email Service](https://developers.cloudflare.com/email-service/)
  (`CLOUDFLARE_EMAIL_API_TOKEN` + `CLOUDFLARE_ACCOUNT_ID`, a both-or-neither pair). Both
  are single-POST HTTPS JSON APIs on the shared client, both share `EMAIL_FROM`, and both
  paths render the identical messages — the only asymmetries are the URL/auth/`to` shape
  (Resend takes `to` as an array, Cloudflare as a string in `/accounts/{id}/email/sending/send`)
  and that Cloudflare's `client/v4` envelope is checked for `success:true` on top of the HTTP
  status. **Resend takes precedence when both are configured** (an existing Resend deploy is
  unaffected by the mere presence of Cloudflare variables); the choice is logged at boot. This
  is an enum, not a trait object — two providers don't yet clear the rule-of-three bar for a
  `Provider` trait, and the enum matches Turnstile/`collection_import`.
- **Transactional email:** sends ride the shared reqwest client with a
  10s per-request timeout; there is no retry/queue — every account-lookup endpoint
  (register / resend / forgot) **spawns** its send off the request path and swallows
  failures (a surfaced 502 would only fire for existing accounts), and a dropped
  registration-completion mail is recovered by re-POSTing register. **Disabled mode
  (no email provider) returns the registration-completion token in the register
  response instead of emailing it** (the SPA drives straight to the set-password step;
  login skips the unverified-`403` gate — see the auth contract) — the intended dev/CI
  posture, gated by `ALLOW_INSECURE_DEV_AUTH=true`; internet-facing registration
  refuses to start in that mode. The disabled emailer normally **logs the whole message
  body** so the otherwise-unrecoverable dev link can be read from stdout — but that body
  carries a live single-use reset/verification token, so `Emailer::from_config`
  **withholds the body on an internet-facing host** (`Config::looks_like_production()`):
  a public deploy that runs with signups closed but no email provider (existing users
  can still request a reset) then never writes a live token into aggregated logs. Configure
  an email provider so account mail is actually delivered. **Residual timing channel:**
  the send runs off the request path, but token *issuance* is an on-path DB write that
  happens only when the account exists, so register/forgot/resend carry a
  sub-millisecond exists-vs-not timing difference. It's left unmitigated (login is the
  one path given explicit dummy-hash equalization) because every probe costs a fresh
  Turnstile solve plus the per-IP budget, making statistical sampling of a few-ms delta
  impractical; the observable response semantics (identical status + empty body) stay
  generic. `onboarding@resend.dev`
  (the default From) only delivers to the Resend account owner's address; production
  needs a verified domain + `EMAIL_FROM`. The 60s registration/resend/forgot mail
  cooldown stays **DB-backed** (the atomic `issue_with_cooldown` insert — on Postgres a
  `pg_advisory_xact_lock` keeps it exactly-once under a pooled writer), not Redis, so it
  holds across instances regardless of the rate-limiter backend.

## Collection import & sync

- **Collection import (Archidekt / Moxfield):** the full mechanics (async `202` + job
  polling, the single-slot queue, per-provider 20/min rate limiters + `429` back-off,
  provider dispatch, the reconcile engine) are contract-documented in
  `docs/api-contracts.md`. The trade-offs: both providers' APIs are unofficial and
  undocumented (may break on their side); a private/missing collection is a `404`, an
  empty one a `422`, and a mirror/replace that matches **zero** catalog cards is refused
  (so it can't wipe a collection against a misresolved/unsynced source). A saved re-sync
  mirrors (replace) — or, when the link opted into **smart** sync, runs the incremental
  smart path — and stamps `last_synced_at`, but there's **no automatic background
  sync** — re-sync is user-triggered. Cards not in our catalog are skipped (surfaced in
  the summary's `unmatched_*`). The layer is provider-generic: another service is a new
  `Provider` variant + module. The import **job queue + per-provider rate limiters
  remain per-process** even with `REDIS_URL` set (Redis backs only the auth/user rate
  limiters): a multi-instance deploy's job polling can 404 when
  `GET …/import/jobs/{id}` lands on a different instance than ran the job (jobs live in
  `AppState.imports`, lost on restart) — fixing that would need a shared job store + a
  distributed rate limiter (or a dedicated worker). This is intentionally out of
  scope — imports are rare, single-slot, and the client just re-imports.
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
- **Collection CSV upload:** the mechanics and the defence-in-depth bounds (16 MB body
  limit, row cap, UTF-8-only, BOM strip, per-field bounds) are in
  `docs/api-contracts.md`. The trade-offs: the shape sniff must check **Archidekt
  first** (an id column), because Archidekt's quantity column also accepts a "Count"
  spelling — only then does Count + Edition + Collector Number mean **Moxfield**, whose
  rows carry no card id and pre-resolve by `(set_code, collector_number)` (exact match
  on the trimmed number, set code lowercased; validated 1058/1058 against a real
  export). The same zero-match `Replace` guard applies, so an empty/garbage upload can't
  wipe a collection. The 16 MB cap is generous for either export (Moxfield's full export
  is ~100 KB per 1000 rows) but can reject a huge *all-columns* Archidekt export — its
  user is told to export only the three needed columns.
- **Collection CSV export (issue #232):** the mechanics are in `docs/api-contracts.md`.
  The trade-off worth calling out: a holding stores only two counts (regular + foil), so
  the export **cannot** reproduce the per-copy metadata a real Archidekt/Moxfield export
  carries — condition, language, tags, purchase price, alter/proxy flags — and fills those
  columns with the neutral defaults a fresh export uses (`NM`/`Near Mint`, `EN`/`English`,
  blank, `False`) rather than inventing data. The Archidekt-only `Multiverse Id`/`MTGO ID`
  columns are emitted as `0` because the Scryfall ingest never captures them, and
  `Types`/`Sub-types`/`Super-types` are **derived** by splitting the stored `type_line` on
  the em dash (front face only for double-faced cards) since we don't store them apart.
  None of this affects the round trip: re-importing keys off the `Scryfall ID` column
  (Archidekt) or `Edition` + `Collector Number` (Moxfield), both of which are faithful. We
  emit the full provider header (all 23 / 13 columns) so the file re-imports cleanly into
  the real services, not just our own uploader. Export is offered for the collection only,
  not the wish list (the wish list has no export-shaped provider format to target).
- **Sealed products use independent holding tables (issues #364/#435):** collection and
  wish list each track sealed products through matching route families and a shared lower
  engine (`handlers/shared/product_holdings.rs`, `lib/api/product-holdings.ts`, and
  `composables/productHoldingQueries.ts`). The tables remain independent —
  `collection_product_items` and `wishlist_product_items` — instead of putting a nullable
  `product_id` on either card-holdings table: a NULL product id would poison the NOT-NULL
  `(user, game, product)` upsert key, and NULLs in unique indexes behave differently across
  SQLite/Postgres. The shared wire shape is `{ quantity, foil_quantity }`, but the UI exposes
  a single **Quantity** and preserves the foil count on edits: a foil sealed variant is a
  separate TCGplayer SKU, not a finish of the same holding. Product lists are deliberately
  **fixed-sort (recency) and unfiltered** (no `q`/`sort`/facets) because personal sealed lists
  are expected to stay small. Collection import/sync/export and public-sharing remain
  card-only, so adding sealed products does not silently change provider round trips. The
  collection value-history chart does include sealed holdings as a separate line, and the
  movers panel switches between independent Singles and Sealed rankings; both reuse the same
  current-count and captured-price assumptions as the original card analytics. Value history
  deliberately ignores holding add dates, revaluing the whole current basket at every historic
  price point; movers likewise rank the current basket rather than reconstructing ownership.
  Their 1d lists independently fall back one available snapshot when the newest comparison has
  no non-zero movers, while longer windows stay anchored to the newest snapshot; `day_as_of`
  reports the fallback date only when that retry actually found movement. The retry's baseline
  normally costs **no extra query**: the anchor query already selects each item's snapshot at
  or before `latest - 2`, which *is* that baseline whenever captures are daily. The extra
  column is worth it because the trigger is a property of the feed, not the user — a flat
  capture day empties every collection's 1d window at once, so a second scan there would double
  the dominant I/O of an uncached, per-user route for every request, all day, against the weak
  prod Postgres. Only a genuine capture gap (nothing captured yesterday) lands on a baseline no
  anchor selected and re-reads the price index. The fallback is deliberately single-step — a
  feed that stalls for several captures shows an empty 1d rather than walking back through the
  history.
- **Foil-variant consolidation (issue #209):** some sets (Secret Lair especially) print
  the **foil** of a card as a *separate* Scryfall object whose collector number is the
  nonfoil's plus a star — `sld` `741` (nonfoil) and `741★` (foil). Left alone, importing
  the `741★` printing lands as its own owned card *beside* the `741` you already track —
  two rows for one card. Our model tracks regular **and** foil per card, so
  `collection_import::consolidate` folds a foil-★ holding onto its base as a **foil copy**.
  Two folds run before every reconcile (full network import, CSV upload, and smart
  re-sync — smart remaps per page so its early-stop still fires): the **incoming** import is
  remapped onto the base (`apply_foil_remap`/`consolidate_local`), and any star row the user
  **already holds** — a legacy pre-#209 import, or a manual add of the `…★` catalog card
  (which is still a distinct, addable card) — is folded onto its base and deleted first
  (`fold_existing_star_holdings`), so a star holding never coexists with the base and
  double-counts. The rule is deliberately **conservative**: only a `finishes="foil"` star
  with a `finishes="nonfoil"` sibling (same set, oracle id, and collector number sans the
  star) is folded — the case where the base genuinely can't be foil on its own. An
  **ambiguous** star (base itself `nonfoil,foil`, ~8 old Invasion-block/STX cases), an
  **etched** star, or a star with **no base** (a standalone promo) keeps its own holding —
  those are genuinely distinct printings. **Pricing:** Scryfall keeps the foil price only on
  the `…★` object, so `scryfall::enrich_foil_variant_prices` copies it onto the nonfoil base
  each sync tick (before the price snapshot, so it's captured into history too); without
  this a folded foil would value at $0 against the base's empty foil price (~94% of pairs).
  The base card then carries **both** prices, so the public catalog shows a foil price on it
  as well. When the periodic sync is off (`SYNC_ON_STARTUP=false`), `tasks::start` still runs
  the enrichment once at boot, so a holding folded by the migration doesn't sit at $0. **Perf
  (weak, cold prod Postgres):** the enrichment is **star-driven** — an `UPDATE…FROM` that starts
  from the ~1,851 `…★` stars via the partial index `m…044_add_cards_foil_variant_star_index` and
  strips the trailing star (`substr(…, 1, length(…) - 1)`) to point-seek each base — replacing an
  earlier base-driven correlated form that full-scanned the wide `cards` heap twice (once for the
  ~40k nonfoil bases, once to hash the ~12k foil rows) to find ~1,627 pairs, ~8 s on the cold prod
  box. It scans only the tiny star set and point-seeks each base, so it is **robust to
  visibility-map state** (unlike a covering index — see the snapshot-read note under §Price
  history): ~3.9× fewer buffers whether or not the preceding sync just churned the heap. One
  consequence of folding to a single dual-finish card: an **Overwrite** import that lists the
  base's nonfoil but not the foil-★ sets the base's *absolute* counts (foil → 0) — the same
  authoritative behavior Overwrite already applies to any card owned in both finishes (the
  provider models the foil separately, so a full import that omits the `…★` means "no foil");
  Merge adds and Smart preserves the unobserved foil, so a user tracking a foil their import
  omits should use those modes. Legacy duplicate rows that predate the fix are also folded
  once by the `m…023_consolidate_foil_star_holdings` **migration** (the same rule in
  cross-backend SQL, wrapped in a transaction so a crash-interrupted boot re-runs cleanly;
  irreversible, so `down` is a no-op) — belt-and-braces for a user who never re-imports; a
  re-import folds them anyway via `fold_existing_star_holdings`. The star (`…★`) card stays
  a browsable catalog entry — only the *holding* is consolidated, so a set's owned-card
  badges show the count on the base card, not the star.

## Decks (issue #363)

- **A new *container* surface, not a third holdings twin.** The collection and wish list are
  twin singletons (one implicit list per `(user, game)`) that share one engine
  (`handlers/shared/holdings.rs`, `makeHoldingApi`, `makeHoldingQueries`). A deck is a
  *first-class, one-of-many* container, so it can't ride that engine (there is nowhere for a
  `deck_id` axis in a flat `/api/{base}/{game}/…` URL or a `[prefix, game]` query key). Decks
  therefore live **beside** the flat card-holdings factory — with
  their own tables (`decks` / `deck_sections` / `deck_cards` / `deck_folders`), their own
  `handlers::decks` module, and their own web api/composable modules keyed under
  `deck`/`decks`. But a deck *card* is shape-identical to a holding, so `deck_card::Model`
  implements `HoldingCounts` and the deck reads reuse `summarize_holdings` and the `Card` DTO
  for free (the deck detail sorts its cards by name directly rather than through
  `apply_card_sort`); the only duplicated backend query is the one `deck_id`-scoped base select. This is the CLAUDE.md anti-fork rule applied correctly — reuse the *lower*
  seams, don't force the singleton engine.
- **Sections are Archidekt-style categories, not fixed boards.** A `deck_sections` table
  (per-deck, `position`-ordered, unique name) with `deck_cards.section_id` gives custom,
  renamable, reorderable buckets a card can be moved between — the model the issue asked for,
  and a superset of a fixed mainboard/sideboard. A new deck is **seeded** with a curated
  default set (`DEFAULT_SECTIONS`: the common type buckets, then functional categories — Ramp,
  Removal, Tutor, … — then Maybeboard) so it has a ready structure; empty sections are hidden
  in the UI behind a toggle. The unique key is `(deck_id, card_id, section_id)`, so a card may
  sit in several sections; deleting a section **reassigns** its cards to the deck's first
  remaining section (never orphaning them), and the deck's *last* section can't be deleted
  (a deck must always have somewhere to file a card).
- **Automatic filing is deliberately type-only.** A normal add targets "Automatic" by default
  and uses the card's front-face type to choose Lands / Creatures / Planeswalkers / Instants /
  Sorceries / Enchantments / Artifacts (in that priority order); an explicit section selection
  always wins. A missing/unknown preset uses an explicit Mainboard or Other catch-all when one
  exists; otherwise the add UI requires a section choice instead of silently filing into the
  first section (often Commander or Sideboard). Imports apply the same rule only to generic
  Mainboard rows and expose an
  `auto_categorize` switch (default true); provider categories such as Commander, Ramp, or
  Sideboard remain authoritative. Functional buckets cannot be inferred safely from oracle text
  (a treasure maker may or may not be "Ramp"), so they stay manual.
- **Per-deck sharing is a boolean column, not a visibility table.** Collections needed the
  separate `collection_visibility` table because a bag of holdings has no owning row to hang a
  flag on. A deck *is* the shareable unit (one row, 1:1), so `is_public` lives on the deck row
  and the public read (`sharing::decks::public_deck`) is a plain `is_public`-filtered load
  (no helper). Everything else mirrors #361 verbatim —
  `resolve_public_user`, the username-first `409`, the single `404` for every miss (no
  existence oracle), and the CDN-cacheable `public_holdings` group. Deck ids are globally
  unique, so the public URL is game-agnostic (`/api/u/{handle}/decks/{id}`), a static sibling
  of `/api/u/{handle}/{game}` that axum's static-wins routing keeps unambiguous.
- **The ownership check is the one genuinely new invariant.** `deck_cards`/`deck_sections`
  carry no `user_id` (they hang off `deck_id`), so unlike every collection query — which
  filters `user_id` directly — a deck route must first `load_deck` to prove the parent is the
  caller's, returning **404** (not 403) for someone else's deck so a deck id can't be probed.
  A security test pins this.
- **The web editor is extended, not forked.** The debounced/serialized/dirty-guarded
  absolute-count editor (`useOwnedCountEditor`) gained an optional `saveFn` injection: decks
  pass a writer that PUTs a `(deck, section, card)` row while reusing all the tricky flush
  machinery, and the existing collection/wish-list callers are untouched (they leave it unset
  and keep the internal mutation path). Before a card changes printing or section, the UI
  explicitly flushes that debounce so a trailing absolute-count write cannot recreate the old
  row. All three backend card mutations also serialize through a transaction-held update of the
  parent deck row, preventing a Postgres read/merge/write race. The deck view derives colour identity, card-type,
  nonland mana-curve, average-mana-value, and hypergeometric draw-odds analytics directly from
  its already-bounded `DeckDetail.cards` payload, so stats add no API request. Draw odds start
  with Commander / Companion / Sideboard / Maybeboard / Signature Spells excluded and expose
  section checkboxes so custom deck structures remain explicit. Printing changes
  use one atomic deck endpoint: the replacement must share the current card's `oracle_id`
  (same-name fallback only when both ids are absent), existing target counts merge, and the
  picker pages through the full exact-name result set (including 800+ basic-land printings).
- **Printing discovery is shared; mutations are not (issue #430).** Collection/wish-list
  quick add, deck quick add, and deck replacement all consume `usePrintingPicker` plus
  `PrintingPickerGrid` / `PrintingTile`. The query is one infinite family keyed exactly as
  `['card-printings', game, name]`, accumulating the all-cards endpoint's 200-row pages so
  every printing remains reachable; callers no longer mint incompatible paged/unpaged cache
  entries. Filtering stays client-side over the **loaded pages only** — fetching every page
  before the first render would add several requests for basic lands — and the shared grid
  explicitly says so whenever more pages remain, with Load more still available even on a
  zero-match filter. The tile owns artwork + printing metadata + display-currency price and
  common current/loading/disabled states, while slots/thin wrappers keep absolute holding
  editors, additive deck writes, and atomic replacement out of a domain-mode switch. The
  scanner keeps its compact Select but shares the same query + metadata label; when OCR gives
  a set code it loads all pages before auto-picking, avoiding a partial-list fuzzy match that
  could hide the exact older printing beyond row 200.
- **Format legality rides the shared `Card` DTO; deck breach is client-derived (issue
  #557).** `cards.legalities` (the verbatim Scryfall object, already stored for `f:` search)
  is parsed into an optional `legalities` map on the shared `CardResponse` — so it appears on
  *every* card payload, lists included. The alternative (a rulings-style
  `/cards/{id}/legalities` endpoint) was rejected: the deck page needs legalities for **all**
  of a deck's cards to show breaches, and per-card fetches or a bespoke bulk endpoint cost
  more than the field, which is highly repetitive JSON that gzip collapses to a few KB per
  60-card page while the DB already selects the column. Deck "in breach" is then evaluated
  client-side (`web/src/lib/legality.ts`) from the `DeckDetail.cards` payload the page
  already holds — same pattern as the deck analytics above, and it keeps owner/public views
  in lockstep for free. Semantics are deliberately conservative: `deck.format` stays free
  text on the wire and is normalized (aliases: EDH, PDH, Comp. Brawl, …) to a legality key —
  a custom/unknown format means *no evaluation*, never "everything is illegal"; a card with
  missing/malformed legality data is counted as unknown, never flagged; `banned`/`not_legal`
  always breach; `restricted` breaches only past 1 total copy per name (Vintage's rule).
  Every section counts (there is no board-role enum to tell a maybeboard apart — see the
  sections trade-off above). The malformed-JSON tolerance lives in one place
  (`parse_legalities` in `handlers/shared/dto.rs`): a bad row degrades to `null`, it never
  fails the request.
- **Deck import/export is a sibling pipeline, not collection reconcile (issue #389).** The
  provider *deck* endpoints and uploaded deck-list rows carry a category/board that the flat
  collection intermediate cannot represent. `deck_import` therefore produces `DeckCardRow`
  values and writes the new deck, its imported sections, and its aggregated cards in one
  transaction. Explicit provider categories stay exact; when `auto_categorize` is enabled
  (the default), only generic Mainboard rows are filed into the preset type buckets above. It
  deliberately reuses the lower collection seams — foil classification,
  external-id and Moxfield `(set, collector_number)` resolution, provider throttling/back-off,
  and `ImportError` mapping — but never calls collection reconcile/consolidate/smart. Empty and
  zero-match imports create nothing. Archidekt permits multiple categories per card while our
  schema files one row in one section, so its first category is the primary section; this avoids
  multiplying the card count. Moxfield's boards map directly, including signature spells,
  with each nested board independently falling back to the older top-level shape. The inverse
  exports preserve sections and regular/foil buckets in provider-shaped CSV or Moxfield-style
  text.
- **Live deck fetches stay small and policy-gated.** A deck is one provider object, so the route
  runs inline rather than entering the long-lived paginated collection queue, while sharing the
  same global provider limiters. Archidekt live URLs are enabled. Moxfield live URLs remain
  behind `Provider::network_import_enabled()` and the approved `MOXFIELD_USER_AGENT`, exactly
  like collection import; CSV/plain-text upload remains available without upstream access. A
  deck-specific 2000-row cap is enforced by every parser and again at the database boundary,
  and the synchronous response returns only the lightweight deck header; the full card DTOs are
  loaded through the normal deck-detail read after navigation.

## Data ingest & datasets

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
- **Tagger art tags (issue #140):** the `art:`/`arttag:`/`atag:` search filters are backed
  by Scryfall's `art_tags` bulk file (~40 MB, refreshed daily; official, so no scraping and
  no mirror-origin special-casing — consumers pull it through the generic
  `/api/mirror/scryfall/file/art_tags` route like any bulk file). Design choices, in order
  of consequence:
  - **The tag hierarchy is expanded at ingest, not at query time.** The bulk file carries
    only *direct* taggings — a parent tag like `animal` has none of its own — but Scryfall's
    own `art:animal` matches every descendant's artwork. We materialise that: each tagging
    also writes a row per ancestor tag, so the search compiles to a single indexed `EXISTS`
    probe on `card_art_tags (game, tag_slug, illustration_id)` — no recursive SQL, nothing
    dialect-sensitive, cheap on the weak prod Postgres. Cost: ~2× rows (~1M total after
    scoping to stored artworks — trivial next to price history) rebuilt wholesale on each
    changed tick inside one transaction (same atomic-swap + zero-row wipe-guard contract as
    rulings; `card_art_tags.id` is i64 because the daily rebuild would exhaust an i32
    Postgres sequence in a few years). The version gate folds the **card dataset's**
    imported version in beside the tag file's `updated_at` — the mapping derives from both
    inputs, so a card re-import (new sets → new artworks) rebuilds it; and an empty card
    catalog (fresh DB, failed card import) **defers** the import without stamping the
    version, so it can never latch empty tables as "complete".
  - **Tag slugs are denormalized onto the mapping rows** (no join to `art_tags` on the hot
    path). Scryfall warns slugs can drift — the durable id is stored in `art_tags` — but the
    wholesale daily rebuild self-corrects any drift, so v1 trades referential purity for the
    flat probe. The `art_tags` metadata table exists for the SPA's autocomplete/tag-browser
    (`GET /api/games/{game}/art-tags`); tags whose expanded artwork set is empty (digital-only
    art, empty branches) are dropped entirely so the browser never offers a zero-result tag.
  - **Not stored (deliberately):** tagging `weight`s/annotations (Scryfall search ignores
    them for plain `art:`, and an ancestor-expanded row has no single meaningful weight) and
    tag `aliases` (1.3k across 11.4k tags; revisit if users ask). Oracle tags (`otag:`) stay
    422 — same infra would work but they key on `oracle_id` and ship in a separate bulk file.
  - **Known limitation — multi-face artworks:** `cards.illustration_id` is the catalog's
    flattened artwork id (first face carrying one), so a tagging that applies only to a
    non-first face (a transform card's back-face painting) is dropped at ingest and `art:`
    won't match that card, where Scryfall would. Fixing it needs per-face artwork identity
    on `cards` — it equally affects `unique:art` grouping — and is deferred with that work.
- **Sealed products (TCGCSV):** sealed-product price tracking is **built** — a TCGCSV
  provider (`crate::tcgcsv`) feeds the `products` + `product_price_history` tables and the
  public `/api/games/{game}/products*` endpoints (`handlers::catalog::products`). [TCGCSV](https://tcgcsv.com)
  is a free, keyless daily mirror of TCGplayer's catalog + prices; TCGplayer category id
  `1` is MTG, the only game synced. Two features live in the module:
  - **Daily sealed-product sweep** (`tcgcsv::ingest` + `tcgcsv::price_history`): fetch
    every group (`/tcgplayer/1/groups`), then per group its products + prices, keep only
    the **sealed** products, classify each into a coarse `product_type`, and upsert them
    with current market prices; a companion daily `snapshot_prices` captures one
    `product_price_history` row per `(game, product, day)` from the already-committed
    `products` rows (so the series stays continuous even on a tick where the version-gated
    sweep is skipped — the same rationale as the Scryfall card snapshot). The whole sweep
    is **version-gated on TCGCSV's `last-updated.txt`** (an `ingest_state` row keyed
    `(mtg, tcgcsv_products)`), so an unchanged day costs one request. Requests are paced
    ~100 ms apart — a full sweep is ~900 requests, well under TCGCSV's ~10k req/day budget.
    Wired into `catalog::refresh_all` / `catalog::snapshot_all` alongside the card sync.
  - **One-time historic price backfill** (`tcgcsv::backfill`, opt-in via
    `PRICE_BACKFILL_ENABLED`): walks TCGCSV's daily price *archives* (one solid-PPMd `7z`
    per day since **2024-02-08**) and fills `card_price_history` **and**
    `product_price_history` for the days before the app began capturing its own snapshots.
    It runs **once**, gated on an `ingest_state` row keyed `(mtg, tcgcsv_price_backfill)`
    (distinct from the Scryfall `default_cards` dataset the status route reports), and is
    **resumable per date** — the last completed archive date is recorded after every day
    (even a 404 day), so a crash resumes at the next day rather than restarting. It
    **never overwrites** an existing `(game, card/product, date)` row (`ON CONFLICT DO
    NOTHING`), so a real daily snapshot always wins over a historic fill. `PRICE_BACKFILL_DAYS`
    (`0` = every archive day, `N` = only the most recent N) bounds a first run. Off by
    default because the walk is slow and hits an external service.
  - Trade-offs: (1) **Classification is derived, not fed** (`tcgcsv::classify`, pure +
    unit-tested) — neither "sealed vs. single card" nor product *type* is a structured
    field in TCGCSV. Sealed-vs-card follows TCGCSV's own guidance: a product with a
    `Rarity` or `Number` entry in its `extendedData` is a card, sealed products have
    neither (a `UPC` entry corroborates but isn't required). Product `type` is derived by
    **ordered keyword matching over the product name** (most-specific prefix first, so
    "Collector Booster Pack" never falls through to plain "Booster Pack"), into a small
    fixed vocabulary (`collector_display`/`collector_pack`/`play_*`/`set_*`/`draft_*`/
    `prerelease`/`commander_deck`/`secret_lair`/`bundle`/`case`/`starter`/`display`/`pack`),
    with `"other"` as the catch-all — kept small + cheap so `product_type` powers a plain
    equality filter. A renamed/novel product shape can misclassify or land in `"other"`.
    (2) The archive decode is **CPU-bound and buffered**: each day's `7z` is a *solid* PPMd
    block, decompressed on a `spawn_blocking` thread and read **in entry order** (every
    entry's stream fully consumed even when skipped, to keep the decoder aligned) —
    single-day, bounded, but not streamed across days. A single malformed prices file is
    logged + skipped rather than aborting the day. (3) TCGCSV blocks generic User-Agents,
    so the backfill needs a descriptive `TCGCSV_USER_AGENT` (defaults to the Scryfall UA
    fallback). Prices are **USD only** (TCGCSV carries no eur/tix). **MSRP** (issue #296)
    is a different provenance again: no feed carries sealed-product MSRP (neither TCGCSV nor
    MTGJSON), so the `products.msrp` retail list price comes from a **committed, curated
    map** (`tcgcsv::msrp`'s `msrp.json`, keyed by TCGplayer product id — the same
    embedded-JSON-where-upstream-is-missing pattern as `mtgjson::fallback` below), applied
    in the product upsert and `null` for anything not listed. Its content hash folds into
    the products sync's version gate, so editing `msrp.json` re-applies on the next sync
    even when TCGCSV itself is unchanged. Product images and the
    price-history `?range` downsampling reuse the same image-proxy + `pricing` helpers as
    the card endpoints; the products list uses a plain case-insensitive name substring for
    `q` (not the Scryfall search compiler — products aren't cards), and set names resolve
    against `card_sets`, degrading to `None` when a product's group has no matching set.
- **Dataset mirror (issue #192):** by default a self-host pulls the three big dataset
  files (Scryfall bulk cards + sets, MTGJSON `AllPrintings`, TCGCSV catalog/prices/
  archives) from a **TCGLense mirror** (`DATASET_MIRROR_URL`, default
  `https://tcglense.com`) rather than from the upstream services — it offloads those
  providers, rides the mirror's CDN, and needs none of the bot-walled providers'
  User-Agents. The `datasets::SyncSource` seam is the single place that decides
  upstream-vs-mirror; set `SYNC_FROM_UPSTREAM=true` to fetch from the real services (the
  mirror host's posture). Serving the mirror is separate: `MIRROR_ENABLED=true` exposes
  `/api/mirror/*` (`handlers::mirror`), which **streams each file from upstream on
  demand** (no disk persistence — the same fetch-and-serve model as `CDN_MODE`) and sets
  CDN-cacheable headers so a fronting CDN absorbs the repeats — but only once a Cloudflare
  **Cache Rule** marks `/api/mirror/*` edge-eligible; Cloudflare bypasses `/api/…` by
  default, so the deploy guides fold the mirror into the *honor-origin* catalog rule
  ([`self-hosting.md`](./self-hosting.md#behind-a-cdn-cloudflare) *Behind a CDN*), and without that rule every consumer pull re-streams from
  upstream. It's off by default so an
  ordinary self-host doesn't become an open proxy to the upstreams; the public mirror
  runs `MIRROR_ENABLED=true` + `SYNC_FROM_UPSTREAM=true`. Trade-offs: (1) the mirror
  fetches on the shared gzip-decoding client, so the Scryfall/TCGCSV JSON is re-served
  **decompressed** (a CDN re-compresses egress; MTGJSON's `.gz` + TCGCSV's `.7z` pass
  through as opaque bytes) — bounded-memory streamed, but the origin→CDN fill transfers
  the uncompressed size once per cache period, and because that decompressed Scryfall bulk
  file is large and steadily growing, watch it against Cloudflare's max cacheable object
  size (512 MB on the Free/Pro/Business plans), above which the edge silently bypasses
  that one route and re-streams it on every pull; (2) the bulk-**file** cache TTL is
  deliberately **shorter** than the bulk-data **catalog** TTL, so a consumer can never
  read a fresh `updated_at` while the CDN still holds a stale file (which it would then
  stamp as current and never re-pull) — a guarantee that holds only while the CDN honors
  each route's `s-maxage`, which is why the deploy rule keeps the mirror under
  *honor-origin* and never a fixed Edge TTL (one fixed TTL would collapse the file and
  catalog windows into one and could invert the guarantee); (3) TCGCSV's dated price
  **archives** are the one mirror route cached **`immutable`** (a year), because
  `archive/tcgplayer/prices-{date}.ppmd.7z` is fixed once published — unlike the live JSON
  beside it, which re-describes a moving catalog. The short meta TTL was actively wrong
  there: the one-time historic price backfill (`tcgcsv::backfill`) walks ~900 archive days
  and **each day is a distinct URL fetched exactly once** per consumer, so no shared cache
  ever serves a repeat *within* one walk, and a one-hour TTL has long expired before the
  next self-host walks — so the mirror re-fetched every archive from TCGCSV for every
  consumer that backfilled, spending TCGCSV's ~10k/day courtesy budget under the *mirror's*
  User-Agent and IP (`tcgcsv_proxy` sends its own UA, not the consumer's). Pinning a
  *missing* day is not a risk: a day TCGCSV hasn't published is a `404`, which
  `public_cache_layer` marks `no-store`; (4) MTGJSON stays ETag-gated end-to-end — the
  mirror forwards `If-None-Match` and relays the upstream `304` (its cache is
  `must-revalidate`), so an unchanged file is still a cheap conditional; (5) the mirror
  is a public read proxy to fixed upstream hosts with `kind`/path inputs sanitised
  (host-locked, no traversal), but — like the image proxy — has no per-IP rate limit or
  bandwidth budget yet, so it carries the same open-proxy abuse posture noted under
  "Image caching".
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
  collector-number groupings carry them. Each card is matched to its drop by
  `(game, set_code, collector_number)` from a **swappable runtime table** (seeded by the
  committed `scryfall/sld_drops.json`, refreshed daily via the mirror scrape/import, and
  persisted + reseeded at boot — see the **Secret Lair drop snapshot — a runtime overlay**
  trade-off below for the full mechanism). The loaded table **goes stale** between
  refreshes: a drop released after it has no title, so its cards surface under a trailing
  `"Other"` group (graceful). Per-card drop membership is **derived live** — the `drop_*`
  fields and `/drops` endpoint read it from the loaded table, not stored as rows (the
  snapshot JSON itself is now persisted in `sld_drop_snapshot` and reseeded at boot). A unit
  test guards that the shipped seed JSON parses and covers `sld`.
- **Sealed-product contents (MTGJSON):** the card-detail "which sealed products is this
  card in?" feature (`crate::mtgjson` → `sealed_contents`) is fed from MTGJSON's
  `AllPrintings.json` (MIT-licensed), the only open source that carries sealed-product
  contents *and* the join keys we need (`sealedProduct.identifiers.tcgplayerProductId` →
  `products.external_id`; card `identifiers.scryfallId` → `cards.external_id`). It runs
  in `catalog::refresh_all` after the Scryfall + TCGCSV syncs (so both tables exist to
  resolve against) and is **version-gated on the file's HTTP `ETag`** (`Meta.json` bumps
  daily from price rebuilds, so it's useless as a gate).

  **Derived booster pull-pools + bundle exclusivity (issue #290).** A sealed product's
  "Can be pulled from boosters" / "{family} exclusives" split needs a product to carry
  `booster`-membership rows for the boosters it *offers*. MTGJSON's native `sealed` recursion
  gives most bundles their pool (a gift box that lists its play + collector boosters expands
  both), but two gaps remain, filled by **ingest-time synthesis** (`mtgjson::ingest`,
  `merge_contained_booster_pools` + `merge_sibling_booster_pools`), each gated on "the product
  has **no** booster rows of its own" so MTGJSON / the fallback stay authoritative: (a) a
  bundle whose `contents: null` was composed only by the fallback (Avatar's Commander's Bundle)
  inherits the pools of the boosters its **`sealed_components`** link; (b) a booster *variant*
  with no pool — a "Sleeved Play Booster Pack", a language edition — inherits its **canonical
  same-set / same-family sibling's** pool (the fullest one in the group). The sibling rule can
  over-share a genuinely-distinct-but-empty variant (a "Minimal"/"Omega" pack) the standard
  family pool, but it only ever touches products that would otherwise render *nothing*, and a
  variant carrying its own curated pool (a "Sample" pack) is non-empty so it's left alone.
  Because this is **pure code** (no data file), its version is a bumped constant
  (`DERIVATION_VERSION`) folded into the gate, so changing the logic forces one rebuild.
  The **exclusive split** for a bundle is judged at read time (`booster_exclusive_card_ids`):
  a non-booster product's premium family comes from its contained boosters — Collector if it
  wraps a collector booster, else a generic special booster (the Chocobo case) — and the split
  reuses the same "cards no other family in the set can pull" comparison as a standalone booster.

  Trade-offs: (1) `AllPrintings`
  is one ~600 MB document; we fetch the ~160 MB gzip and parse only trimmed structs, but
  a rebuild still transiently uses a few hundred MB (the fetch buffers the compressed
  body and a normal set's booster sheets span ~its whole card list) — acceptable for a
  daily, ETag-gated background task; per-set streaming to bound that is future work.
  (2) The `booster` bucket ("can be pulled from") is inherently the whole set, so a
  common card lists ~its set's booster products — honest but low-signal; the UI groups it
  as a distinct "Can be pulled from" section so it doesn't masquerade as a guaranteed
  "Found in". (3) Contents reference cards by MTGJSON `uuid` / `(set, number)`; the ingest
  resolves them via the same file's `cards[]`, so a **cross-set** reference whose card
  isn't in the ref's own set (a rare bonus-sheet / The-List case) can go unresolved and be
  skipped. (4) The table is **rebuilt wholesale** each run (delete-then-insert in one txn),
  so a product whose contents changed never leaves a stale row. The offline dummy seed
  fabricates a handful of memberships so the feature renders without a live pull.
  (5) MTGJSON's contents are hand-curated upstream and **lag** — some products ship with
  `contents: null` (e.g. Avatar's "Commander's Bundle", whose borderless "Eternal"
  Commander-staple reprints — Sol Ring, Deflecting Swat, … — would otherwise show no
  sealed product at all). `mtgjson::fallback` (`fallback_sealed.json`, a committed snapshot
  like `scryfall/sld_drops.json`, keyed by `(set, number)` + membership) fills those gaps,
  merged **per-product only when MTGJSON emitted zero rows for that product** — so MTGJSON
  stays authoritative and a fallback entry self-retires the moment upstream authors the
  product's contents (no code change). The escape hatch is an entry flagged
  `supplement: true` (issue #352): when upstream *does* describe a product but is missing an
  axis — the Commander's Bundle's contents became a `deck` reference that originally
  resolved to no deck data (its guaranteed staples and 2-of-10 pool vanished while the
  product still counted as "described"), and its land packs are textual-only — the entry's
  memberships merge **alongside** upstream's rows instead of stepping aside. Supplements
  are add-only by default: a row upstream later authors dedups away (per-card self-retirement,
  the same pattern as the SLD bonus-pool pass). A narrowly-scoped
  `override_memberships: true` handles the next upstream state: the bundle deck now resolves,
  but MTGJSON's deck shape marks all 13 staples as guaranteed and cannot express that 10 are
  a two-of-ten random pool. For only the curated product/card pairs, contradictory upstream
  memberships are removed before the fallback rows are inserted; unrelated upstream rows
  remain authoritative. In both modes the entry's `components` keep the usual
  only-when-upstream-empty gate — so upstream's composition (e.g. its "1× Commander's Bundle"
  deck line) renders as-is. The file's content hash is folded into the version
  gate alongside the ETag (`ingest::compose_version`), so editing the fallback data rebuilds
  on the next sync even when `AllPrintings.json` is unchanged (which forces one full re-fetch
  to rebuild the merged table — rare, only on a fallback edit). The correct long-term fix for
  a gap is still to author the contents upstream; the fallback is the stopgap until it flows in.
  The authoritative upstream that `AllPrintings.json` is built from is the community repo
  **`mtgjson/mtg-sealed-content`** (raw `.../main/data/{contents,products}/<SET>.yaml`): per set,
  `contents/<SET>.yaml` maps a product name → its `sealed` (child products, `{count, name, set}`)
  / `card` / `deck` / `other` breakdown (an **empty `[]` = unauthored upstream**), and
  `products/<SET>.yaml` maps that name → `identifiers.tcgplayerProductId` — the join key onto our
  `products.external_id`. When hand-authoring a fallback entry, **pull from there rather than
  hand-researching**: our synced `AllPrintings.json` can *lag* this repo, so a `contents: null`
  in our DB may already be authored upstream; if it's empty there too, the gap is genuinely
  unauthored (the Secret Lair superdrop bundles below were, as of #279). (taw's
  `magic-sealed-data` is a *different* repo — booster-pack collation only, no Secret Lair.)
- **Secret Lair drop contents — derived, not hand-authored (`mtgjson::sld`):** MTGJSON ships
  many `SLD` (Secret Lair Drop) products with `contents: null`, but unlike ordinary packaging
  a drop's meaningful contents is the specific *list of cards* in that drop — and the app
  already tracks that (`scryfall::drops` / `sld_drops.json`, a title + collector numbers per
  drop). So instead of hand-typing card lists (which would rot as new drops sync), the ingest
  **derives** them: a null-contents `SLD` product's name (`Secret Lair Drop: <drop> - <Foil|
  Non-Foil> Edition`) is matched to its drop by normalised title, and the drop's cards become
  the product's `contains` contents (foil per the edition). Choices: (1) It's a **third source**
  after MTGJSON and the fallback, under the **same per-product gate** (applied only to products
  with no membership row yet) — so upstream stays authoritative and a derived entry self-retires
  when MTGJSON authors the product. (2) Matching is **exact normalised-title only** (case/
  punctuation-insensitive), which is *safe* (validated at ~99.8% on the drop products MTGJSON
  *does* describe — an unmatched product simply shows nothing, never a *wrong* drop) at the cost
  of recall; a tiny curated `PRODUCT_DROP_OVERRIDES` (TCGplayer id → drop slug) covers the few
  localised drops named differently on the storefront vs Scryfall's gallery. (3) The derivation's
  inputs (the *live* drop snapshot + overrides) are hashed into the ingest **version gate**
  alongside the ETag + fallback hash — computed live, not memoised — so a refreshed drop snapshot
  (the mirror's daily scrape / a consumer's daily import, or regenerating `sld_drops.json`) re-runs
  it on the next sync. See the **Secret Lair drop snapshot** trade-off below for how that snapshot
  is now refreshed at runtime. (4)
  Secret Lair **superdrop bundles** (a bundle *of drops*) are the inverse case: their composition
  isn't derivable from the drop snapshot and MTGJSON leaves it `null`, so it's hand-authored in
  `fallback_sealed.json` as linked `sealed` components (the drops the bundle contains) — the
  usual fallback stopgap, self-retiring when `mtgjson/mtg-sealed-content` authors it upstream.
  (5) **Random bonus cards (`merge_sld_bonus_cards` in `mtgjson::ingest`).** Some Secret Lair
  superdrops ship **one unpredictable bonus card** with every drop, drawn *at random* from a pool,
  *in addition to* the drop's own cards. They're recorded `variable` ("may be included") from a
  curated `RANDOM_BONUS_POOLS` table keyed by `sld_drops.json` slug, with the pool shared across a
  superdrop's drops (e.g. each Avatar drop draws one of Command Tower / Fellwar Stone — *one*, not
  both, so `variable` not `contains`). The key design point: this pass is **not gated on `covered`**
  the way `merge_sld_derived` (the drop's *own* cards) is — the bonus pool is a separate axis, so it
  must attach even once MTGJSON authors the drop's deck and flips the product to "covered"
  (otherwise the bonus would *vanish* exactly when upstream fills in the deck). It stays **add-only
  and self-retires _per card_**: `rows` is a set, so a `(product, card, variable, foil)` MTGJSON
  already authored is deduplicated away — upstream wins for every card it names, while a pool card it
  *omits* still surfaces. (A per-**product** self-retire was rejected: MTGJSON authors only each
  FINAL FANTASY drop's *own* per-drop card `7001`/`7002`/`7003` as `variable`, so skipping the whole
  product would suppress the genuinely-additive shared Evoke pool `7004`–`7008` — the valuable
  chase.)
  **The table is deliberately tiny.** The bulk of SLD bonus pools *are* enumerated by MTGJSON's
  `AllPrintings` `variable` contents (e.g. A Box of Rocks, Brain Dead, the Marvel singles); the
  ingest already surfaces those directly, so duplicating them here would only be maintenance
  surface that self-retires anyway. Only the superdrops MTGJSON does **not** deliver are curated:
  **Avatar** (`7062`/`7063`, marked `other: "Bonus card unknown"`), **Marvel's Spider-Man**
  (`7013`–`7021`, axis omitted), **FINAL FANTASY** (`7004`–`7008`, only the per-drop card authored),
  and **TMNT** (`7077`/`7078`/`7083` Slime Against Humanity, `other: "Bonus card unknown"`). Every
  pool was verified card-for-card against Scryfall's `sld` set (`promo_types` includes `sldbonus`)
  and each drop's published bonus mechanics; a number already in **every** listed drop's own gallery
  is excluded so it can't be shadowed, and a drop absent from the table simply shows no bonus pool —
  never a wrong one. The table is hashed into the derivation version alongside the drop snapshot, so
  editing it re-runs the pass on the next sync.
- **Secret Lair drop snapshot — a runtime overlay, seeded by a committed file (`scryfall::drops`).**
  The curated drop titles aren't in the bulk card API; they live only on Scryfall's `/sets/sld`
  gallery page. Originally we scraped that page **offline** (`api/scripts/gen-sld-drops.mjs`) and
  committed the result (`scryfall/sld_drops.json`), so the API had no runtime scrape dependency —
  but new drops then needed a human to re-run the script and redeploy. Now the committed file only
  **seeds** the drop store at boot (and is the offline / first-boot fallback), and the store is
  *swapped at runtime*: the **mirror origin** (`MIRROR_ENABLED`) re-scrapes the gallery daily
  (`scryfall::sld_scrape`, the JS scraper ported to Rust) and installs the fresh snapshot, serving
  it at `GET /api/mirror/scryfall/sld-drops`; **every other instance** imports that snapshot from
  the mirror daily (`scryfall::sld_sync`, `SLD_DROPS_IMPORT_ENABLED` on by default) — the same
  "contact one origin, not the upstream" posture as the dataset mirror and the fingerprint index,
  and it honours the self-host-never-scrapes rule (only the origin talks to Scryfall). Choices:
  (1) The store is a **process-global `RwLock<Arc<Tables>>`** (not per-`AppState`), because the read
  path reaches it from a bare `From<card::Model>` conversion with no state in hand; the swap is the
  same brief-lock/clone-an-`Arc` pattern the fingerprint index uses, and `drops::table()` now hands
  out an owned `Arc<DropTable>` so a reader is stable across a concurrent swap. (2) `install_snapshot`
  **validates before swapping** — a snapshot missing the `mtg/sld` set (a markup change that yields
  zero drops) is *rejected*, so a broken scrape/import never wipes the good table; the origin keeps
  serving its last-good (ultimately the committed) snapshot. (3) **The snapshot blob IS persisted** —
  in a dedicated single-row `sld_drop_snapshot` table (`scryfall::sld_persist`; migration m000054),
  and the store is **reseeded from it at boot** before the first-refresh deferral. Without
  persistence the in-memory store reseeds from the committed file each boot, so a restart soon after
  a refresh would serve that stale seed for up to a full interval — the `initial_delay` deferral
  honours the last-run *timestamp*, but the in-memory data was thrown away on restart. Reseeding from
  the last-good persisted snapshot makes that deferral correct: it serves fresh drops, the mirror
  origin serves the same `ETag` it served before the restart, and a consumer's conditional import
  `304`s onto the persisted snapshot rather than the seed. It's its **own** table, not a column on
  `ingest_state`, because the ~58KB blob would otherwise be dragged into every unfiltered
  `ingest_state` read (e.g. the sitemap's global last-modified query), and `StateFields` (no
  `Default`, built at ~9 unrelated call sites) would carry an SLD-only field everywhere. The
  `(mtg, sld_drops)` `ingest_state` row still records the last-run time + import `ETag`, so a restart
  within the interval **defers** the first scrape/import and after a long downtime runs immediately
  (`sld_tasks::initial_delay`); the committed file remains the offline / first-boot fallback (no
  persisted row yet). The stored `content_version` + `updated_at` also make "which drop snapshot is
  loaded, and when did it last refresh" queryable.
  (4) The content version hashes
  the drop **data** (each set's ordered drops), *not* the JSON bytes — so the pretty-printed
  committed seed and the mirror's compact scrape of the *same* drops share a version. It feeds both
  the mirror `ETag` (a `304` when unchanged) and the sealed-contents derivation version
  (`sld::derivation_version`, computed live). Hashing the raw bytes instead would make every reboot
  (which reseeds from the persisted snapshot, or the committed file on first boot) look like a change versus the last-imported compact
  snapshot and trip a needless full `AllPrintings` rebuild; hashing the data means only a *real* drop
  change re-derives SLD product contents (even when `AllPrintings.json` is byte-identical). The
  mirror endpoint also reads the body + version from a single store snapshot, so a concurrent daily
  swap can't pair a stale `ETag` with a fresh body.
- **Sealed-product composition / "what's in the box" (MTGJSON):** the same
  `AllPrintings.json` `sealedProduct.contents` that feeds `sealed_contents` also carries the
  product's *packaging* — `sealed` (nested packs/boxes, **with a `count`** and a `uuid` that
  resolves to the sub-product's `tcgplayerProductId`), `deck`, `card` (fixed promos), and
  `other` (physical extras: dice, storage boxes, land packs). The membership pass flattens
  all of this down to the individual *cards*, discarding the structure — so a second pass
  (`model::build_compositions`) keeps it, in a sibling table `sealed_components`, and the
  `/products/{id}/contents` endpoint links the sub-products a box contains. Design choices:
  (1) A **separate table**, not a column on `products`: the composition is a variable-length
  ordered list with per-item links, and the structural data lives *only* in `AllPrintings`
  (which we don't retain), so it must be persisted at ingest like `sealed_contents`.
  (2) The list is **non-recursive** — a product's *direct* components (a bundle's "9× Play
  Booster", a box's "36× pack"), not the packs' cards (those are already the cards section).
  (3) `pack` (a booster *sheet config*, not a purchasable sub-product — and at the top level
  usually a pack describing *itself*) and `variable` are **excluded** from the composition;
  they're left to the card-membership pass, so a bare booster pack shows *no* "what's in the
  box" rather than a redundant "1× itself". (4) Both passes run in one `spawn_blocking` over
  the parsed doc, each building its own cheap `Indexes` (the maps are a few ms over the parse
  tree — the ~600 MB cost is the parse, shared); both tables are wiped+rebuilt in the same
  txn. (5) `mtgjson::fallback` gained a `components` list (same per-product gate: applied only
  when MTGJSON gave that product *no* composition) so the `contents: null` products get a
  hand-authored box — e.g. the Avatar Commander's Bundle's "9× Play Booster + 1× Collector
  Booster + lands + life counter + storage box", the retail manifest MTGJSON hasn't authored.
  (6) The `/products/{id}/containers` endpoint reads the same table in reverse by
  `child_product_id`, letting a booster pack show the boxes and bundles that directly contain
  it (issue #415). This remains non-recursive and data-backed — it does not guess relationships
  from names — and repeated child lines in one parent are collapsed with summed quantities.
  A sibling `(game, child_product_id)` index covers this reverse read; the original
  `(game, product_id, position)` unique index continues to cover the forward composition.
- **Gotcha — `reqwest`:** pinned `default-features = false, features = ["rustls",
  "gzip", "stream", "json"]` to use rustls (matching SeaORM's `runtime-tokio-rustls`)
  and to stream + auto-decompress the gzip bulk file. No overall request timeout on
  the client (the bulk download streams for a while); a `read_timeout` guards stalls.

## Price history & the historic-price chart

- **No Postgres timeseries extension — plain relational tables, by design (issue #297).**
  The card- and sealed-product price charts (`GET …/cards/{id}/prices`,
  `…/products/{id}/prices`) are served from two ordinary tables — `card_price_history`
  and `product_price_history` — **not** from any PostgreSQL timeseries extension. There is
  no TimescaleDB, hypertable, `time_bucket`, or continuous aggregate anywhere (a keyword
  sweep across `api/`, `deploy/`, and CI is clean; the *only* Postgres extension the app
  uses is `pg_trgm` for card-name search — m027 — which is not a timeseries one). The
  "timeseries" behaviour lives in a schema shape plus a little app code instead:
  - **Shape.** One row per `(game, entity, day)` of decimal-string prices (kept as the
    provider's strings, never `f64`); `as_of_date` is a zero-padded `"YYYY-MM-DD"` **text**
    column, not a native `DATE` (mirroring `cards.released_at`). The single index on each
    table is the **unique composite** `(game, {card,product}_id, as_of_date)`
    (`idx_card_price_history_game_card_date` / `idx_product_price_history_game_product_date`),
    which doubles as the daily snapshot's upsert key — so it costs nothing extra on write.
  - **The read is a single index range scan, not a scan + sort.** Each endpoint
    (`handlers::catalog::prices::card_prices`, `products::product_prices`) filters
    `game = ? AND id = ?` plus an optional `as_of_date >= cutoff`, ordered by
    `as_of_date ASC`. That composite index is a leading-prefix match: the two equality
    columns seek to one entity's rows, the date cutoff is a range bound on the trailing
    column, and the index's own order **satisfies the `ORDER BY` with no separate sort**
    (lexical order over the zero-padded date string equals chronological order). Confirmed
    on Postgres against a 200k-row stand-in — both the full-history and the windowed query
    plan to `Index Scan … Index Cond: (game, id[, as_of_date >= …])` with **no Sort node**
    and a few hundred buffer hits, sub-millisecond — exactly the shape the weak prod
    Postgres wants, and the same access pattern on SQLite.
  - **Downsampling is app-side, not a DB rollup.** Longer `?range` windows are thinned to
    one representative row per bucket by `handlers::shared::pricing::downsample_rows`
    (keep the *last* real row per `bucket_days`-wide bucket — never averaged, so every
    returned point is a genuine snapshot), so the wire payload and plotted line stay light
    however much history accrues. The DB just returns the windowed, ordered run; the result
    set per request is bounded by one entity's days of captured history (≈ one row/day since
    the 2024-02-08 backfill floor — a few hundred rows), small enough that an in-process
    rollup beats the round-trips a DB-side `GROUP BY` / `time_bucket` would add.
  - **Why not an extension.** The blocker is the runtime dual-backend (SQLite by default,
    Postgres by `DATABASE_URL` scheme, both drivers compiled in; the whole non-search layer
    is portable SeaORM query-builder kept byte-identical on SQLite, and the default tests +
    CI run on SQLite — see "Postgres dual-backend"). A Postgres-only hypertable /
    `time_bucket` / continuous aggregate has no SQLite equivalent, so it can't be *shared*:
    adopting one means either dropping SQLite or carrying a second, divergent Postgres-only
    storage/query path behind the `db::Dialect` seam — not worth it for a workload this
    small and point-shaped that is already an O(log n) index range scan. If a single series
    ever did grow huge, the move is Postgres-only *physical* tuning behind that same seam
    (a native `DATE` column + BRIN, or partitioning), never an extension the SQLite default
    can't load.
- **The movers reference-date lookup gets its own descending-latest index (`m…050`).** Both
  price-history twins carry a *second* purpose-built index — `(game, as_of_date, {card,product}_id)`,
  the mirror of `m…031`'s covering index with the key columns reordered. The collection *movers*
  endpoint (`handlers::collection::price_movements`) anchors every window to `SELECT MAX(as_of_date)
  WHERE game = ? AND {card,product}_id IN (…owned…)`; against the `(game, id, as_of_date)` key that
  `MAX` cannot stop early — it reads *every* owned item's whole history (on a 17.8M-row Postgres 16
  repro: ~846k entries, planned as a ~176k-cold-heap-read bitmap scan — the shape that logged ~5 s on
  prod — or, VM-warm, a full index-only scan). Leading with `as_of_date` turns it into a **backward
  index-only scan that stops at the first owned row** (measured: 30 entries, 6 buffers, ~2 ms), with
  the trailing id making the `IN (…)` test index-only. Crucially it stays robust to the churned
  visibility map the snapshot leaves behind — the tail seek pays ~30 heap-visibility fetches, not
  176k — so it is the *tiny-point-seek* shape the covering-index audit endorsed, **not** the
  full-table index-only scan it rejected below. It rides the same accepted trade-off as `m…031`
  (another never-pruned index) with negligible write cost: `as_of_date` increases monotonically, so
  each day's rows append at the B-tree's right edge. Of the other three logged statements, the
  windowed value-history fetch is a heap-free index-only scan on `m…031`'s covering index whose
  cold cost is inherent `O(cards × days-in-range)` volume — **but only while the visibility map is
  warm and the planner's stats are fresh**; on the *un-vacuumed, never-`ANALYZE`d* prod table it
  degrades badly (a plan flip and heap-fetch cliff, both fixed by the per-tick maintenance two
  bullets down — read that before trusting "index-only" here). These routes also sit behind the
  analytics response-cache and the `analytics` per-user rate-limit bucket. The remaining two
  (movers' per-item anchor read and value-history's pre-cutoff seed) stopped being whole-history
  scans entirely — next bullet.
- **The per-item anchor reads are correlated `LIMIT 1` point seeks, not grouped aggregates
  (2026-07 slow-log follow-up to `m…050`).** The movers anchor read and value-history's
  pre-cutoff seed used to be grouped aggregates: ten
  `MAX/MIN(CASE WHEN … THEN date||'|'||price… END) … GROUP BY id` string-concat aggregates
  evaluated over *every* captured row of *every* held item (~846k rows × 10 expressions for the
  951-card prod collection). That shape is index-only on `m…031`, but its cost is the volume
  itself — it logged 14.7–19 s cold (~1.2 s warm, CPU-bound in the per-row concat aggregates)
  on prod. `SnapshotSeek` (`handlers::collection::price_movements`) replaces it: the query
  drives from the holdings table (`collection_items` / `collection_product_items` filtered by
  user + game, so the giant `IN (…)` id lists are gone too) and each anchor is a correlated
  scalar sub-select — `game = ? AND id = <holding>.id [AND as_of_date <= target] ORDER BY
  as_of_date DESC LIMIT 1` — one tiny descent of `m…031` per item per anchor, the same
  endorsed tiny-point-seek shape as `m…050`, robust to the churned post-capture visibility
  map. Measured on a faithful 2.7M-row repro (951 held cards × ~900 days), Postgres 16: movers
  anchors 1268 ms → 130 ms warm, ~6.3 s → 0.13 s cold; value-history's cutoff seed
  363 ms → 13 ms; SQLite 1243 ms → 39 ms — byte-identical results, zero heap fetches, no new
  index. Honest caveats: the two `first_priced` anchors filter on a non-key column, so a
  never-priced finish walks that one item's run finding nothing (per-item and aggregate-free,
  but not a true point seek); and product seeks ride `m…020`'s non-covering unique index (one
  heap fetch per returned row — fine at sealed-product counts). Movers additionally accepts an
  optional `?window=<day|week|month|year|two_year|three_year|all_time>` (the biggest-movers
  query-efficiency follow-up): the SPA passes the active window so a visitor who never leaves
  the default computes ~two anchors per held item instead of ten and never pays the all-time
  `first_priced` walk unless "All" is opened — the unrequested anchors are selected as SQL
  `NULL` (`AnchorNeeds`, so their sub-selects are never evaluated) and the ranker skips those
  windows. Both holding kinds still ship for the requested window (so the Singles/Sealed switch
  stays a client-side toggle), the reference dates are computed regardless, and omitting the
  param preserves the original all-windows response for API-key consumers; each window caches
  independently under the same version keys. The windowed raw fetch is deliberately unchanged. The analytics response-cache is also **single-flight** now
  (`AnalyticsCache::get_or_compute`): concurrent misses for one body key compute once — prod
  logged the same movers computation running twice concurrently after a capture orphaned every
  key. The in-flight map is process-local and cancellation-safe (an RAII guard removes the
  entry when a disconnected leader's request future is dropped, waking followers to retry
  leadership), and the leader publishes the body to followers over the watch channel so they
  get it even when the Redis write fails.
- **The daily snapshot's cards read is left as a sequential scan — a covering index was measured
  and rejected.** Each tick, `scryfall::price_history::load_price_columns` reads *every* card's
  five price columns (`SELECT id, price_usd, price_usd_foil, price_eur, price_tix FROM cards WHERE
  game = ?`, ~106k rows) to snapshot them into history. It shows up as a ~3.7 s slow-query on the
  weak, cold prod Postgres because `game = ?` matches the whole (single-game) table so the planner
  sequential-scans the wide ~140 MB `cards` heap for five tiny columns. The obvious fix — a
  covering index `(game) INCLUDE (id, price_usd, price_usd_foil, price_eur, price_tix)` for a
  heap-free index-only scan (the `m…031` `card_price_history` pattern) — was built and benchmarked
  and **deliberately not shipped**: it repeats the `m…033 → m…034` mistake. The snapshot runs in
  the **same sync tick, immediately after** `catalog::refresh_all`'s `flush_cards` rewrites every
  card whose price moved, and `cards` is **never `VACUUM`ed** (m…034's stated invariant). On a real
  import day **~34 %** of the ~106k cards change price (measured day-over-day from
  `card_price_history`), which — at ~6 rows/heap-page — clears the all-visible bit on ~87 % of
  pages. The index-only scan then either degrades to a per-tuple heap fetch (measured on a faithful
  106,520-row Postgres 17 repro: ~69k heap fetches + random I/O + hint-bit write-back, **slower in
  wall-time than the plain seq scan** despite fewer raw buffers) or the planner reverts to the seq
  scan for no gain — while the `INCLUDE` payload still adds unconditional non-HOT write
  amplification on every price move. The index-only win (~29× fewer buffers) only materialises
  against a well-vacuumed visibility map, precisely the state that does *not* hold when the
  snapshot runs. So the seq scan stays: the read touches the whole partition regardless, and no
  visibility-map-robust index beats it here. (Contrast the foil-enrichment fix in the same audit —
  `m…044`, §Collection import & sync — which *is* shipped because it leans on a tiny partial index
  + point seeks, not a full-table index-only scan, so it is robust to the churned VM.)
- **The value-history read's real cold cost was the un-maintained table, not a missing index — the
  fix is per-tick `VACUUM (ANALYZE)`, plus a wide-range skip-seek (2026-07 slow-log follow-up).**
  The bullets above call the windowed value-history fetch a "heap-free index-only scan whose cost
  is inherent volume." Re-profiling on a faithful 18M-row Postgres 16 repro (20k cards × ~900 days,
  a 956-card collection, cold cache) showed that is true *only when the table is well-maintained* —
  and prod's price-history tables are neither `VACUUM`ed nor `ANALYZE`d, because the daily capture
  inserts one row per entity into a multi-million-row table and Postgres's default *scale-factor*
  autovacuum triggers (`analyze_scale_factor` 0.1, `vacuum_insert_scale_factor` 0.2) only fire after
  ~0.1–0.2·N changes — **months** on a table that large. Two degradations follow, both matching the
  logged ~6 s:
  - **Stale stats flip the plan.** With stats frozen from when the table was far smaller, the
    planner mis-costs `card_id IN (…owned…)` and demotes it from a per-card index seek to an
    in-memory **Filter**, scanning the *whole game's* date window: the 30-day read scanned 620k
    index entries (`Rows Removed by Filter: 590364`, 137,979 buffers) to return 30k rows — **6.7 s**,
    versus **0.16 s** the moment `ANALYZE` ran.
  - **A stale visibility map** turns the covering index's index-only scan into per-row
    heap-visibility fetches: the same read paid ~30k–59k heap fetches (~15× the buffer touches) on a
    churned tail until a `VACUUM` reset the all-visible bits.
  Both are fixed at the source by `catalog::maintain_price_history` — a `VACUUM (ANALYZE)` of both
  price-history tables **right after each capture** (`snapshot_all`), Postgres-only, under the sync
  leader lock, `SHARE UPDATE EXCLUSIVE` so it never blocks traffic, and scanning only the freshly
  appended tail (`as_of_date` is monotonic). `m…053` is the standing backstop: it switches the two
  tables to *absolute* autovacuum thresholds (scale-factor 0) so stats/VM stay fresh even if the
  explicit pass is disabled (`SYNC_INTERVAL_HOURS=0`) or fails. This is deliberately **not** another
  index — the read was already index-only; the table was just never maintained. (`cards` stays the
  documented exception — the bullet above measured a covering index there as a net loss, and `cards`
  is not on this per-capture `VACUUM` path.)
- **Wide ranges (`1y`/`2y`/`3y`/`all`) fetch one row per bucket, not every daily row
  (`value_history`).** The daily fetch pulls `O(cards × days-in-range)` rows and thins them in Rust
  (`downsample_rows`); for the wide, downsampled ranges that is up to ~1M rows fetched to plot a few
  dozen points. Naively pushing the thinning into the DB (`DISTINCT ON`/`GROUP BY`) is a **trap** —
  it still scans every daily entry *and* adds a full-set disk sort (measured **slower**: 2.76 s vs
  1.50 s on the all-history read). Instead, for `bucket_days > 1` the fetch runs one correlated
  `LIMIT 1` seek per `(item, downsample bucket)` on the covering index — the [`SnapshotSeek`] shape,
  but a *range* per bucket — so it returns the already-thinned set (all-history: **15k rows vs 861k**,
  at *fewer* cold reads: 8.3k vs 8.6k), which the existing fold + final `downsample_rows` reduce to a
  byte-identical series (a SQLite equivalence test pins this across every wide range). The buckets
  align to `downsample_rows`' `days-since-CE / bucket_days` grid; the pre-cutoff seed still carries a
  real value into the window's first day; `all` reads its start from a cheap `MIN(as_of_date)` on
  `m…050`. The daily ranges (`7d`/`30d`/full) keep the chunked bulk fetch — at daily resolution
  there is nothing to thin, and the covering index already serves them index-only once the table is
  maintained (previous bullet). Cross-backend raw SQL through the `db::Dialect` placeholder seam
  (portable `SELECT … UNION ALL` bucket table; `||`/`COALESCE` like `encoded_snapshot`).

## Images & assets

- **Image caching:** the proxy mechanics (lazy first-view download to
  `<DATA_DIR>/images`, the `scryfall.io` allow-list, redirects disabled, concurrency
  cap 8, `CDN_MODE`) are in `docs/api-contracts.md`. The posture: a bulk image download
  is deliberately avoided (hundreds of GB) — images cache lazily, per view. The image
  route is public and card ids are enumerable, so
  it's open to scripted disk-fill / bandwidth-amplification abuse — there's no per-IP
  rate limit, cache budget, or eviction yet (the same open posture as the dataset
  mirror; the per-IP limiter deliberately classifies the image/icon routes out of
  the public-catalog quota, since one grid page load fires dozens of art
  requests). Set icons go through the
  same cache (`.../sets/{code}/icon`, `image/svg+xml`), so the provider is hit only
  once per asset rather than hotlinked on every view. `CDN_MODE=true` (fetch-and-serve
  only, no persistence) only makes sense behind a caching CDN — without one every view
  re-fetches upstream, so it's off by default.
- **Negative cache (issue #214):** a positive cache-miss persists to disk, but a
  *failure* has nowhere to live — so a URL the provider `403`s (the TCGplayer product CDN
  does this for products with no art; `has_image` is derived from the provider metadata
  and over-reports) was re-fetched, re-logged at ERROR, and returned as a **500** on every
  single page view. The proxy now treats a definitive upstream `4xx` (all but `429`/`408`,
  which are transient) as [`ImageError::Unavailable`] → a **404**, and remembers it
  in-process for 6h so the dead URL isn't asked again within the window (`5xx`/`429`/timeout
  stay a retryable **502**, uncached). The cache is a plain `Mutex<HashMap>` keyed by the
  asset's cache path, TTL-checked on read and lazily pruned at a soft cap of 8192 entries —
  deliberately not per-deployment-tunable (a constant, not an env var) and not persisted
  (a restart re-probes each dead URL once, which is cheap). The TTL is a compromise: too
  long strands an image the provider adds later, too short brings the log spam back.
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

## Visual card scanner

- **Approach — perceptual hash, not a learned embedding.** A card is identified by a
  256-bit DCT perceptual hash (pHash) of its image, matched by Hamming distance. The
  decisive reason is **cross-implementation parity**: the reference index is built in
  Rust (`api/src/phash`) and the query is hashed in the browser (`web/src/lib/scan/
  phash.ts`), and the two must agree closely or the distance is meaningless. Both sides
  run the *same* integer grayscale + box downsample + DCT against a **shared basis
  table** (`scripts/gen_phash_basis.py` generates identical f64 constants for both
  languages), so parity is guaranteed by construction and pinned by golden fixtures in
  both test suites. A learned embedding would be more robust but would require shipping
  and running one identical ONNX model on both sides (float-nondeterminism caveats) plus
  a heavy client bundle — deferred behind the `algo_version` upgrade seam. The trade-off
  is honesty about accuracy: pHash identifies a card's *art* well but **cannot** separate
  printings that share art (reprints, borderless, alt-art) — which is exactly why the
  scanner is **hybrid** (below), not a pHash-only replacement.
- **Hybrid, not a replacement.** The visual match resolves *identity* (which card); the
  existing OCR of the collector-number + set line (`lib/scan/match.ts::matchPrinting`)
  resolves the *exact printing*. Full-card pHash mathematically discards the tiny
  set-symbol / collector signal that distinguishes reprints, so replacing OCR with pHash
  would regress printing accuracy — the two are kept complementary.
- **Storage / NN search — opaque BLOB, no vector extension.** Fingerprints live in a
  narrow `card_fingerprint` table (never a column on the wide `cards` row) as a 32-byte
  BLOB/BYTEA, **never queried by content in SQL**. Nearest-neighbour is a brute-force
  Hamming scan in memory (`FingerprintIndex` in `AppState`, rebuilt after each build/sync)
  — a few ms over ~90k entries — so there is no pgvector / sqlite-vec, no dialect-specific
  popcount, and behaviour is identical on SQLite and Postgres (the DB only ever does
  insert/upsert/select-all). Query-time cost to the (weak) prod Postgres is therefore
  **zero**; only the off-request-path enumerate-pending walk touches it, as an
  index-bounded `cards.id` range scan.
- **Sourcing — the one sanctioned bulk image fetch.** Building the index needs every
  card image once, which collides with the "never bulk-download" posture. Resolved by
  making the build **opt-in and off by default** (`FINGERPRINT_BUILD_ENABLED`): only the
  operator's index-building instance runs it, at the smallest useful size (`small`,
  146×204), through the same downloader (`ImageCache::fetch_bytes`, the shared 8-way cap
  + host allow-list), **hashing and discarding** each image (nothing written to
  `<DATA_DIR>/images`). The derived index (~3 MB for ~90k) is distributed via the
  existing dataset mirror, so every ordinary self-host imports it and fetches **zero**
  images — the guarantee stays literally true for self-hosts. This is within Scryfall's
  guidelines: their documented rate limit governs the **API** (`api.scryfall.com`); the
  **image CDN** (`cards.scryfall.io`) is **not** rate-limited, and Scryfall supports
  resolving many images from bulk-data URIs. So the build applies no artificial
  per-request delay — the shared 8-way concurrency cap is its only politeness bound.
- **Privacy.** Matching is server-side but the photo never leaves the device: the browser
  computes the 32-byte fingerprint locally and uploads only that (a small, non-reversible
  vector) to `POST /api/games/{game}/scan`. The endpoint is auth-gated (scanning builds a
  signed-in user's collection) and `no-store`.
- **Crop correctness wins at the frame edge.** A clipped card cannot produce the tight
  full-card crop that pHash and printing OCR need, so both browser detectors reject contours
  and corners inside a small scale-aware frame inset rather than allowing a convex hull to
  invent a boundary corner. The outline tracker also treats unsupported corner movement as
  a miss instead of blending it toward the edge. Capture re-detects at its higher resolution,
  requires the first quad to agree tightly with the green lock, and only pools later burst
  frames whose geometry and pHash still belong to that card; a failed first-frame check asks
  the user to move the whole card inside the frame. This
  deliberately trades a little edge-of-view recall for reliable crops while retaining the
  glare/finger hull recovery and the live loop's bounded mobile cost. The same posture
  extends to region-of-interest crops: a contour touching an *artificial* crop edge is
  skipped outright (neither candidate nor clipped-card blocker) — only crop sides that
  coincide with the real camera frame keep clipped-card semantics — so a windowed search
  can never invent a corner the full frame would not have produced.
- **Busy-background lock-on: escalate evidence, never loosen gates.** In the scanner's
  guided modes the OpenCV detector is an escalation ladder over one shared grayscale+blur
  (`opencvDetect.ts` orchestrates; candidate gating lives in `opencvContours.ts`,
  mode-aware selection in `quadSelect.ts`, window geometry in `guidedDetect.ts`):
  median-luma Canny (the historical pass) → a fixed low band (a superset edge map for
  soft or luma-mismatched edges the median thresholds sit above — a bright region drags
  the luma median far past a weak card edge; the card typically surfaces as the hole its
  border moat leaves in the closed texture blob) → the Otsu retry. The plain
  (default) mode is deliberately NOT the ladder — it stays the historical median-band +
  Otsu pipeline, behaviour-compatible, so non-scanner callers can never see a
  card-identity change. Each guided band tries its morphologically closed map then the
  raw one (the 5×5 close both bridges glare breaks *and* glues the outline to clutter —
  the raw map preserves topology when gluing is what defeated the closed pass), and
  clipped-card blockers are per-mask, so one band's merged blob can't veto another's
  clean candidate. A pass's winner returns early only when two orthogonal evidence
  checks agree — perimeter support (sides trace real edges; inverts on grid textures,
  where a texture-aligned phantom out-scores a true quad whose sides bow a pixel) and
  interior-ring emptiness (the card's border moat; the one signal busy texture cannot
  fake) — otherwise the ladder continues and the best support+interior-weighted winner
  is returned at the end; an ambiguous pass (two near-tied spatially distinct
  candidates, after same-card-tolerance dedup) refuses the whole frame rather than
  letting an earlier pass's banked winner shadow it. Selection is spatially aware
  without weakening geometry: acquisition gates hard on distance to the on-screen guide
  (up-weighting near-guide candidates), a first detection stays *tentative* (no green)
  until a consistent second confirms it (`cardLock.ts` wraps `quadTracker`, and resets
  it when a track is lost so the tracker's retained memory of the old card can't veto a
  legitimate reacquisition), and once locked, ticks search only a padded ROI around the
  prior in tracking mode — clutter outside the ROI simply vanishes — with a
  prior-associated, fallback-free full-frame retry on the second consecutive miss, so
  the lock can never jump to an unrelated object. ROI crop sides within the camera
  frame's safety inset keep physical (clipped-card) semantics; truly artificial crop
  sides make a touching contour neither candidate nor blocker. The capture path threads
  the tracked quad as its prior (ROI-first, capture-strict association, then
  full-frame; the historical tight-agreement gate stays as defense in depth) so
  whatever pass produced a busy-background live lock is reproducible at capture
  resolution — previously an unconditioned full-frame re-detect could fail at commit
  time even with a correct green lock. Costs stay bounded: fallback bands run on a
  1-in-3 miss cadence full-frame, always on (cheap) ROI crops. Deliberately *excluded*
  for now — Hough-line quad assembly (no reproducible failing scene needs it; highest
  false-positive and perf risk), adaptive thresholding, `minAreaRect`, and any learned
  segmentation. Known pre-existing edge case (unchanged by this work): a card whose
  bounds fill >95% of the frame registers no clipped-card blocker (the near-full-frame
  guard reads it as the viewport), so a crisp printed inner frame there can win — the
  guard cannot be removed without breaking the hole-contour mechanism on portrait
  phones, where the closed texture blob legitimately fills the frame.
- **Detector readiness is explicit.** The npm-bundled OpenCV runtime is a multi-megabyte
  payload that compiles embedded WASM on the main thread. The dedicated scanner route starts
  warming it as soon as the page mounts, overlapping that cost with reading and camera
  permission rather than waiting for the live preview. The ordinary warm-up window does not
  run the lightweight detector: letting that weaker algorithm establish a lock and then
  switching geometry underneath it made cold production loads behave differently from warm
  local ones. The fallback remains available only after a real load failure, is visibly
  labelled in the camera overlay, logs the underlying error, and retries when the camera is
  started again. Do not move the import into global app startup or route-chunk prefetching —
  opening the mobile navigation prefetches `/scan`, and must not make every user download
  OpenCV.
- **OpenCV loads through a one-line ESM wrapper — a production-only interop trap, not a
  network failure.** `@techstark/opencv-js` is a UMD/CJS module whose **`default` export is a
  real `Promise`** that resolves to the initialised `cv`. A *direct* dynamic
  `import('@techstark/opencv-js')` makes the production Rolldown build insert its CJS→ESM
  interop (`__toESM`) into the import's own `.then`, which re-wraps that promise in an object
  built with `Object.create(Object.getPrototypeOf(promise))` — it inherits `Promise.prototype`
  (so `instanceof Promise` is true) but holds no internal promise state. Returning that
  fake-thenable through a `.then` chain makes the runtime call `Promise.prototype.then` on an
  incompatible receiver and **throw** (`Method Promise.prototype.then called on incompatible
  receiver`), so `loadOpenCv` rejected and the scanner silently ran the basic detector — **in
  the production build only** (the Vite dev optimizer hands the module over in a shape that
  doesn't trip the interop, which is why it "worked locally"). The fix is `opencvRuntime.ts`, a
  local ES module that *statically* `import cv from '@techstark/opencv-js'` and re-exports it;
  `loadOpenCv` dynamically imports **that wrapper** instead. Because the wrapper is a genuine ES
  module, dynamically importing it yields a normal namespace whose `default` is the real
  promise, and the interop stays local and correct. OpenCV still ends up in its own ~15 MB lazy
  chunk (the wrapper is reached only via dynamic import from `ScanView`). The same thenable
  mis-assimilation is why the Node integration spec `require()`s the package directly rather
  than going through `loadOpenCv`. `opencvRuntime.spec.ts` pins the wrapper indirection; don't
  collapse it back to a direct `@techstark/opencv-js` dynamic import (verify a real `vite build`,
  not just dev — this bug is invisible in dev and in `cargo test`/vitest).
- **OCR runtime is self-hosted — no third-party code at runtime (issue #451).** The
  set/collector-number OCR half of the hybrid scanner uses `tesseract.js`, whose browser
  *default* downloads its worker, wasm core, and `eng.traineddata` from
  `cdn.jsdelivr.net` at runtime and runs them with the app origin's privileges (the
  worker's `fetch` carries the session cookie) — a polyfill.io-class exposure the
  pre-launch audit flagged, and a contradiction of the otherwise self-contained posture
  (the sibling OpenCV runtime is npm-bundled). So the same lockfile-pinned files are
  served same-origin instead: the `tesseractAssets()` plugin in `web/vite.config.ts`
  publishes `worker.min.js`, the LSTM `tesseract-core*.wasm.js` builds, and
  `eng.traineddata.gz` under `/tesseract/` (emitted into the build; streamed from
  `node_modules` in dev), and `useCardScanner.ts` passes explicit
  `{ workerPath, corePath, langPath }` plus `workerBlobURL: false` — the worker loads as
  a plain same-origin script, so a future `worker-src 'self'` CSP can hold (see
  §Browser security headers). Shape constraints worth knowing before "improving" it:
  `corePath`/`langPath` are **directories** and the worker picks a core by SIMD
  feature-detection against canonical filenames, so the files keep unhashed names and
  can't ride Vite's hashed `?url` assets; unhashed names need revalidation, so they're
  served `public, no-cache` — by the API's SPA fallback, and set explicitly for
  `/tesseract/*` in the two Caddy `file_server` topologies (`deploy/Caddyfile`,
  `deploy/web.Caddyfile`), whose `file_server` otherwise sends no `Cache-Control` and
  would let browsers cache heuristically. That keeps the worker/core JS in step with
  each deploy; the **traineddata is the exception** — tesseract.js consults its
  IndexedDB cache (a versionless key) before ever re-fetching, so a data-package bump
  only reaches browsers that haven't scanned before or whose cached model fails to
  initialise. Acceptable for a model file that essentially never changes; a forced
  refresh would need a versioned `langPath` or `cacheMethod: 'refresh'`. Only the
  `-lstm` cores and the `best_int` traineddata ship, because the scanner pins
  `OEM.LSTM_ONLY` and the legacy engine's data isn't published — requesting a legacy
  OEM finds no legacy core (the SPA fallback answers the core URL with `index.html`,
  which `nosniff` stops `importScripts` from running): it fails by design, not by
  accident. A unit spec (`useCardScanner.spec.ts`) locks the
  explicit-paths contract so a refactor can't silently fall back to the CDN. Bonus: the
  scanner now works where jsdelivr is blocked and leaks no usage/IP to a CDN.
