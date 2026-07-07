# Known trade-offs & future work

This is the on-demand detail companion to `CLAUDE.md`. `CLAUDE.md` is the always-loaded
core (kept deliberately slim); this file is the full rationale reference for every deliberate
trade-off, residual edge case, and "why it's built this way" essay that used to live in
`CLAUDE.md`'s "Known trade-offs / future work" section. Read it when you need the *why*
behind a design decision, the exact failure modes a feature tolerates, or the caveats a
production deploy must account for. Grouped for navigability; the content is otherwise
carried over near-verbatim from the audited source, with the sealed-product provider
(TCGCSV) trade-offs added.

---

## Auth & sessions

- **Token storage:** the refresh token is an `HttpOnly` cookie (not readable by JS)
  and the access token is held in memory only, so an XSS can't exfiltrate the
  long-lived credential. In production set `COOKIE_SECURE=true` (HTTPS) and serve
  web + API same-origin (or configure cross-origin CORS credentials).
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

## Rate limiting & anti-abuse

- **Auth anti-abuse (CAPTCHA + rate limiting):** the auth mutation endpoints are
  guarded by Cloudflare Turnstile (`captcha.rs`) and per-IP rate limiting
  (`ratelimit.rs`, `governor`). The rate limiters are **in-memory by default**; set
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
  CAPTCHA and rate limiting are both **disabled by default** (no `TURNSTILE_SECRET_KEY`;
  though rate limiting is on, it fails open when the client IP is unresolvable, e.g.
  in-process tests) so dev/CI/e2e run without either — a production deploy must set
  the Turnstile keys and (behind a proxy) `TRUST_PROXY_HEADERS`. Login is still not
  account-locked (per-IP only), so distributed credential-stuffing from many IPs
  isn't fully mitigated; the token cooldown (`issue_with_cooldown`, an atomic
  conditional insert) remains the per-user email-issue brake underneath the per-IP
  limit.
- **Per-user rate limiting (issue #168):** the *authenticated* API surface (the
  `/api/collection/*` + `/api/wishlist/*` endpoints + `GET /api/auth/me`) also carries a per-user limit
  (`ratelimit.rs`'s `UserRateLimiters` + the `user_rate_limit` middleware), keyed by
  the access-token user id rather than the IP, so it caps what a single account can
  do no matter how many IPs it comes from — the per-user complement to the per-IP
  auth limits above. Two classes: a generous `general` bucket (reads/edits/batch
  lookups, ~300/min) and a tight `import` bucket (the expensive import/sync/CSV
  endpoints, ~10/min); over-limit is `429` + `Retry-After` + `no-store`. It gates on
  the same `RATE_LIMIT_ENABLED` switch and shares the per-IP limiter's caveats:
  in-memory by default, or Redis-backed via `REDIS_URL` (shared across instances, same
  fail-open posture), and it only engages for a request carrying a *valid* bearer token (an
  unauthenticated / bad-token request has no user to key on and is left to the
  handler's own `401`, so it is **not** an IP-level DoS guard for those routes —
  that would still need the per-IP/WAF layer). The public catalog + image routes
  remain unthrottled (same open posture noted under image caching).

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
  `ratelimit.rs`, gated on `TCGLENSE_TEST_POSTGRES_URL` / `TCGLENSE_TEST_REDIS_URL`) run
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

## Transactional email (Resend)

- **Transactional email (Resend):** sends ride the shared reqwest client with a
  10s per-request timeout; there is no retry/queue — every account-lookup endpoint
  (register / resend / forgot) **spawns** its send off the request path and swallows
  failures (a surfaced 502 would only fire for existing accounts), and a dropped
  registration-completion mail is recovered by re-POSTing register. **Disabled mode
  (no `RESEND_API_KEY`) returns the registration-completion token in the register
  response instead of emailing it** (the SPA drives straight to the set-password step;
  login skips the unverified-`403` gate — see the auth contract) — the intended dev/CI
  posture, so don't run prod without a key (there the token stays out of the response,
  so an unverified/unowned address can never complete registration). `onboarding@resend.dev`
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
  the enrichment once at boot, so a holding folded by the migration doesn't sit at $0. One
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
    fallback). Prices are **USD only** (TCGCSV carries no eur/tix). Product images and the
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
  CDN-cacheable headers so a fronting CDN absorbs the repeats. It's off by default so an
  ordinary self-host doesn't become an open proxy to the upstreams; the public mirror
  runs `MIRROR_ENABLED=true` + `SYNC_FROM_UPSTREAM=true`. Trade-offs: (1) the mirror
  fetches on the shared gzip-decoding client, so the Scryfall/TCGCSV JSON is re-served
  **decompressed** (a CDN re-compresses egress; MTGJSON's `.gz` + TCGCSV's `.7z` pass
  through as opaque bytes) — bounded-memory streamed, but the origin→CDN fill transfers
  the uncompressed size once per cache period; (2) the bulk-**file** cache TTL is
  deliberately **shorter** than the bulk-data **catalog** TTL, so a consumer can never
  read a fresh `updated_at` while the CDN still holds a stale file (which it would then
  stamp as current and never re-pull); (3) MTGJSON stays ETag-gated end-to-end — the
  mirror forwards `If-None-Match` and relays the upstream `304` (its cache is
  `must-revalidate`), so an unchanged file is still a cheap conditional; (4) the mirror
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
  collector-number groupings carry them. We capture them once into a committed
  snapshot (`scryfall/sld_drops.json`, regenerated by
  `api/scripts/gen-sld-drops.mjs`) and match each card to its drop by `(game, set_code,
  collector_number)` — no runtime scraping. The snapshot **goes stale**: a drop
  released after it was taken has no title, so its cards surface under a trailing
  `"Other"` group (graceful, but re-run the generator to pick up new drops). The
  snapshot is the only drop source; nothing in the DB stores drop membership (the
  `drop_*` fields and `/drops` endpoint derive it live from the embedded table), so
  no migration/re-import is needed to update it. A unit test guards that the shipped
  JSON parses and covers `sld`.
- **Sealed-product contents (MTGJSON):** the card-detail "which sealed products is this
  card in?" feature (`crate::mtgjson` → `sealed_contents`) is fed from MTGJSON's
  `AllPrintings.json` (MIT-licensed), the only open source that carries sealed-product
  contents *and* the join keys we need (`sealedProduct.identifiers.tcgplayerProductId` →
  `products.external_id`; card `identifiers.scryfallId` → `cards.external_id`). It runs
  in `catalog::refresh_all` after the Scryfall + TCGCSV syncs (so both tables exist to
  resolve against) and is **version-gated on the file's HTTP `ETag`** (`Meta.json` bumps
  daily from price rebuilds, so it's useless as a gate). Trade-offs: (1) `AllPrintings`
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
  product's contents (no code change). The file's content hash is folded into the version
  gate alongside the ETag (`ingest::compose_version`), so editing the fallback data rebuilds
  on the next sync even when `AllPrintings.json` is unchanged (which forces one full re-fetch
  to rebuild the merged table — rare, only on a fallback edit). The correct long-term fix for
  a gap is still to author the contents upstream (taw's `magic-sealed-data` /
  `mtgjson/mtg-sealed-content`); the fallback is the stopgap until it flows in.
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
- **Gotcha — `reqwest`:** pinned `default-features = false, features = ["rustls",
  "gzip", "stream", "json"]` to use rustls (matching SeaORM's `runtime-tokio-rustls`)
  and to stream + auto-decompress the gzip bulk file. No overall request timeout on
  the client (the bulk download streams for a while); a `read_timeout` guards stalls.

## Images & assets

- **Image caching:** the proxy mechanics (lazy first-view download to
  `<DATA_DIR>/images`, the `scryfall.io` allow-list, redirects disabled, concurrency
  cap 8, `CDN_MODE`) are in `docs/api-contracts.md`. The posture: a bulk image download
  is deliberately avoided (hundreds of GB, and against Scryfall's guidelines) — images
  cache lazily, per view. The image route is public and card ids are enumerable, so
  it's open to scripted disk-fill / bandwidth-amplification abuse — there's no per-IP
  rate limit, cache budget, or eviction yet (the same open posture as the dataset
  mirror; the per-IP limiter covers only the auth endpoints). Set icons go through the
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
