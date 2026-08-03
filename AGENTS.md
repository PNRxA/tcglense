# AGENTS.md

Guidance for working in this repository. This file is the always-loaded core; the
detail lives in `docs/` — read the relevant one before working in its area:

- [`docs/api-contracts.md`](./docs/api-contracts.md) — every HTTP endpoint, wire shape, the search syntax, caching/ETag/sitemaps, import/sync mechanics
- [`docs/architecture.md`](./docs/architecture.md) — the fully annotated file map for `api/src/` and `web/src/`, plus test organization
- [`docs/operations.md`](./docs/operations.md) — running, commands, CI, releases, Docker, and the full environment-variable reference (authoritative: `api/src/config.rs`, `api/.env.example`)
- [`docs/tradeoffs.md`](./docs/tradeoffs.md) — known trade-offs and design rationale; read it before "fixing" anything that looks odd
- Self-hosting / deploying: [`docs/self-hosting.md`](./docs/self-hosting.md) — the deploy hub (homelab, production split, bare metal, CDN cache rules), then the managed-cloud guides [`docs/deploy-digitalocean.md`](./docs/deploy-digitalocean.md) (Droplet, recommended) · [`docs/deploy-app-platform.md`](./docs/deploy-app-platform.md) (PaaS)

**TCGLense** tracks trading-card games: a card catalog (MTG first, via Scryfall),
singles + sealed-product price history (TCGCSV, MTGJSON), per-user collections and a
wish list (Archidekt/Moxfield/CSV import), email-first auth (Turnstile + rate
limiting), and a public API with scoped `tcgl_` API keys (OpenAPI at
`/api/openapi.json`, Scalar UI at the SPA's `/docs`).

| Dir    | App                     | Stack |
|--------|-------------------------|-------|
| `api/` | Backend (HTTP JSON API) | Rust 2024 · axum 0.8 · SeaORM 1.1 · SQLite by default, Postgres picked at runtime by the `DATABASE_URL` scheme · JWT (HS256) · Argon2 |
| `web/` | Frontend (SPA)          | Vue 3.5 · Vite 8 · Pinia · TanStack Query (vue-query) · vue-router · Tailwind 4 · shadcn-vue · TypeScript |

A `tcglense` **CLI + TUI** client (Rust · clap · reqwest · ratatui) for this API lives in
its own repository — [PNRxA/tcglense-cli](https://github.com/PNRxA/tcglense-cli) — and is
**not** in this tree.

**Search trap:** `.claude/` is gitignored but holds nested full-repo worktrees. Scope
repo-wide greps/finds to `api/` and `web/`, and never edit a file through a
`.claude/worktrees/…` path — that silently changes a different branch's checkout.

**No private memories:** don't stash project knowledge (gotchas, conventions, workflow
notes) in a per-user/agent memory store — put it in the repo (this file or `docs/`) via a
PR, so the whole team and every agent sees it. Learned something worth keeping? Add it here.

**Scoping a "review since `<tag>`":** pin the commit range and re-verify `HEAD` before you
finalize — `main` advances by squash-merge, so a `git diff v<x>..HEAD` scope can grow while
you work; confirm the final file list matches what you actually reviewed.

## Run & verify

```sh
cargo run        # api/ — :8080; migrations on boot; needs a real JWT_SECRET; first run: cp .env.example .env
npm run dev      # web/ — :5173, Vite proxies /api; first run: npm install (Node ^22.18 or ≥24.12)
```

`./scripts/dev.sh` runs both (and refuses already-taken ports — a stale server holds
the port). Default DB: `api/tcglense.db` (SQLite; WAL sidecars are normal).
Everything else (Postgres, command matrix, CI, releases): `docs/operations.md`.

**Before calling a change done:** `cargo check` + `cargo test` for `api/` work;
`npm run type-check && npm run lint && npm run test:unit -- --run` for `web/` work.
Also format: `cargo fmt --all` (in `api/`) and `npm run format` (in `web/`) — CI's
`format` job gates both with `cargo fmt --all -- --check` and `npm run format:check`
(oxfmt), so unformatted code fails the build. CI runs the test suites **plus a ts-rs
drift check** (generated types in `web/src/lib/api/generated/` must match the Rust
DTOs) and the `format` gate, but still does **not** run lint/clippy — the checklist
above is the only thing catching those.

**e2e gotcha:** Playwright starts only the *web* server. Start the API yourself with
`SEED_DUMMY_DATA=true` first — the specs probe `/api/ready` and **silently skip**
when it's unreachable, so the suite can "pass" without testing anything. Probe
**readiness, never `/api/health`**: liveness answers the moment the listener binds and
stays up through the boot-migration window, while the startup gate still answers every
other route with a maintenance `503` — gating on it makes the first specs fail on a 503
instead of skipping. The dummy seed (catalog, then the verified `e2e@tcglense.test`
account) runs *after* the gate opens, so "fully booted" is a successful seeded login —
that's what CI waits for.

## Where code lives

Skeleton only — the full annotated map is [`docs/architecture.md`](./docs/architecture.md).

- `api/src/`: `router.rs` (every route + middleware) · `handlers/` (incl. `tools/` — the
  play-aid namespace) · `entities/` +
  `migrator/` · `auth/` · `catalog/` (GAMES registry + per-game dispatch) · providers
  (`scryfall/`, `tcgcsv/`, `mtgjson/`) · `collection_import/` · `deck_import/` · `security_tests/`
  (HTTP-level suites driving the real router).
- `web/src/`: `views/` + `router/` · `components/` · `composables/` (query hooks) ·
  `stores/` (Pinia) · `lib/api/` (typed client; `generated/` = ts-rs wire types).

## Adding a backend feature

1. **Entity:** `entities/<name>.rs` (`DeriveEntityModel`); export from
   `entities/mod.rs` **and** `entities/prelude.rs`.
2. **Migration:** the date prefix is **frozen** — files are
   `m20240101_0000NN_<name>.rs`; increment only the counter (don't use today's
   date). Register in `migrator/mod.rs` in **two places**: a `mod` line + a
   `Box::new(...)` entry in `migrations()`.
3. **Handler + route:** module under `handlers/`, wired in `router.rs`. Return
   `AppError` — never `unwrap`/`expect`/`panic!` on a request path. SeaORM query API
   only (parameterized; anything raw goes through `db::Dialect` or it breaks one
   backend). Use `JsonBody<T>`, not raw `Json<T>`. Pick the right cache group in
   `handlers/cache.rs`; consider a `security_tests/` suite. **Document it in `/docs`:**
   any public/API-key JSON endpoint needs a `#[utoipa::path]` (+ `utoipa::ToSchema` on
   its DTOs, and a `__path_*` re-export from the group `mod.rs`) registered in
   `openapi.rs` — the `coverage_drift` test there fails until every `router.rs` route is
   either documented or in its `INTENTIONALLY_UNDOCUMENTED` allow-list (with a reason).
4. **Wire types:** derive `ts_rs::TS` on response DTOs
   (`#[cfg_attr(test, derive(ts_rs::TS))]`) — regeneration is in the frontend recipe.

Adding a TCG = a `Game` in `catalog::GAMES` + a provider module + one arm each in
`catalog::refresh_all` and `catalog::seed_all`.

## Adding a frontend feature

- **Wire types are generated:** derive `ts_rs::TS` on the Rust DTO, run `cargo test`
  from `api/` (only `cargo test` regenerates — not check/build; config in
  `api/.cargo/config.toml`). Never hand-edit
  `lib/api/generated/*.ts` — **except** `generated/index.ts`, a hand-maintained
  barrel: add an export line per new DTO (as is `lib/api/index.ts`).
- **Server state → vue-query** via `useAuthedQuery`/`useAuthedMutation` (public pages
  use plain `useQuery`; don't call `authFetch` directly for reads). Invalidate
  dependent queries after mutations; set a per-query `staleTime`. Footgun: reactive
  params go **inside** `queryKey` as refs/computed, never `.value`, or
  refetch-on-change breaks. **Client state → Pinia**; never duplicate a datum in
  both. Do **not** wrap `stores/auth.ts`'s refresh in vue-query — the single-flight
  rotation is hand-tuned.
- **Pages:** view under `views/` + route in `router/index.ts`; authed pages need
  `meta: { requiresAuth: true }`; per-view head tags via `usePageMeta()`.
- **UI primitives:** `npx shadcn-vue@latest add <name>`; hand-written ones copy the
  `components/ui/button/Button.vue` idiom. `@vueuse/core` is only a transitive dep —
  don't import it; use `defineModel` for v-model.

## Keep it maintainable

Hard-won from the 2026-07 refactor — these are the habits that previously grew
1,700-line files and six copy-pasted collection/wishlist twins:

- **Collection and wish list are twin surfaces — extend the shared engine, never
  copy-paste one to build the other.** The seams: `handlers/shared/holdings.rs`
  (DTOs + post-fetch shaping; only the per-entity SeaORM queries stay duplicated),
  `web/src/lib/api/holdings.ts` (`makeHoldingApi`), `composables/holdingQueries.ts`
  (`makeHoldingQueries`), and `useHoldingsLanding`/`useHoldingsBrowse` (view
  engines; templates stay per-view). A feature added to one surface lands in the
  seam, parameterized — asymmetries are flags/config there, not a forked file.
- **Rule of three:** before hand-rolling a helper, grep for an existing seam
  (`auth/secret.rs` for token generation/hashing, `catalog/ingest_state.rs` for
  provider sync bookkeeping); a third copy of anything means extract it.
- **One concern per file.** When a module mixes orchestration + pure algorithm +
  bookkeeping, split it (the `ratelimit/` and `mtgjson/ingest/` directory modules
  are the pattern: submodules `pub(super)`, public surface re-exported from
  `mod.rs` so external paths don't change). ~500 lines is the smell threshold —
  judge cohesion, not length; a long table of data constants is fine.
- **Views stay thin:** reactive engines belong in composables, repeated static
  markup in presentational components (`components/home/` is the pattern) — a
  `<script setup>` past ~150 lines is probably an unextracted engine.
- **Refactors are pure moves:** behavior-preserving, verified by the full suites
  and a zero-diff `web/src/lib/api/generated/` after `cargo test`.

## Don't break these

Rationale: `docs/tradeoffs.md` · full contracts: `docs/api-contracts.md`.

- Auth answers **generically** (register/resend/forgot reveal nothing; login's
  dummy-hash verify on unknown users is timing equalization, not dead code).
  Password rules are validated **before** an email token is consumed. A missing/bad
  CAPTCHA token is deliberately **400** (never 401/403).
- No email provider configured = the local email dev bypass (register returns the
  completion token; login skips the verified gate). A provider is either
  `RESEND_API_KEY` or the Cloudflare Email Service pair (`CLOUDFLARE_EMAIL_API_TOKEN`
  + `CLOUDFLARE_ACCOUNT_ID`); configure exactly one (Resend wins if both are set).
  Internet-facing configurations refuse to enable signups without a provider, a
  non-default `EMAIL_FROM`, and Turnstile; keep those bypasses local only.
- **API-key scope is enforced by extractor choice, not HTTP method:** reads take
  `AuthUser` (session JWT or any `tcgl_` key), writes take `WritableUser` (read-only
  key = **403**), key management (`/api/auth/api-keys`) takes `SessionUser` (JWT
  only — a key can't mint/list/revoke keys). A bad/expired/revoked key is **401**.
  Keep the per-user rate limiter key-aware (`ratelimit/per_user.rs` resolves `tcgl_` → user)
  or keyed traffic bypasses the quota. Store only the SHA-256 hash; the plaintext is
  shown **once**.
- Rate limiting **fails open** (Redis outage, unresolvable IP); CAPTCHA fails
  **closed**. `TRUST_PROXY_HEADERS=true` only behind a proxy that overwrites
  `X-Forwarded-For` (else clients spoof their IP).
- Collection and wish list are **independent tables** that share the ts-rs DTOs in
  `handlers/shared/holdings.rs` — editing a shared card shape changes both wire surfaces.
  Card holdings use **external** card ids; both counts zero deletes the row. Both surfaces
  also hold sealed products in independent `collection_product_items` /
  `wishlist_product_items` tables (`/api/{collection,wishlist}/{game}/products*`, external
  TCGplayer ids on the wire, same both-zero-deletes rule) through the lower shared seams:
  `handlers/shared/product_holdings.rs`, `lib/api/product-holdings.ts`, and
  `composables/productHoldingQueries.ts`. Collection import/sync/export
  remain card-only; collection value history and movers include both card and sealed-product
  holdings. **Public sharing exposes sealed products too:** the read-only
  `/api/u/{handle}/{game}/products{,/summary,/sets}` reads mirror the authed collection product
  endpoints (`collection::owned_product_{summary,sets}`/`owned_products_page` wrap the same
  `CollectionProductRepository`), gated by the identical per-game visibility flag; the public
  landing (`PublicCollectionView`) renders them through the shared `ProductHoldingSection`
  (public mode = a `handle` prop) and the read-only `PublicProductBrowseView` (a `readonly`
  `ProductGrid`, owner's counts as a static badge).
- **Decks** (`/api/decks/{game}*`, issue #363) are a **container** surface — many per user,
  in `decks`/`deck_sections`/`deck_cards`/`deck_folders` — **not** a collection/wishlist twin,
  so they don't ride `makeHoldingApi`/`makeHoldingQueries`; they live beside it and only reuse
  the *lower* seams (`deck_card::Model` impls `HoldingCounts`; the `Card` DTO). A `deck_card`
  has **no `user_id`** (it hangs off `deck_id`), so every deck route must `load_deck` to prove
  ownership first — a deck that isn't the caller's is **404, not 403**. Per-deck sharing is an
  `is_public` **column** on the deck row (not a `collection_visibility`-style table — a deck is
  1:1 with the shareable unit), public at `/api/u/{handle}/decks/{id}` (username-first `409`,
  reusing #361's `resolve_public_user`). A **maybeboard** is likewise a column
  (`deck_sections.is_maybeboard`, issue #570), never a name match: its cards are still stored,
  returned, and edited normally, but every "what is this deck" reader skips them — `summary`
  (vs its sibling `maybeboard_summary`), the list's `card_count`, `needed`, and, client-side,
  legality + analytics. A new such reader must split on the flag too, or the deck page's header
  and the deck list will disagree. The **name** (`is_maybeboard_section_name`) only *seeds* the
  flag where a section is born from untyped text — deck import, and migration 62's backfill of
  pre-flag decks — so a renamed maybeboard stays out and a section merely called "Considering"
  stays in. **Deck analysis is server-side** (issue #596): composition + draw odds
  (`/stats`), the legality verdict (`/legality`), and a seeded sample hand (`/goldfish`) all
  live in `handlers/decks/analysis/`, so a CLI gets what the deck page shows; each is
  mirrored under `/api/u/{handle}/decks/{id}/…` **through the same `analyse_*` core**, so a
  shared deck and its owner's copy can never disagree. All three are **`GET`s taking
  `AuthUser`** — they write nothing, so a read-only key must be able to call them.
  **Legality is two modules, not one:** `analysis::legality` judges each card against the
  format's Scryfall data, `analysis::rules` judges the deck (size, copy limit, command zone,
  colour identity) and the former composes the latter, so a new check belongs in the rules
  module, not a third one. Its zone split reads the section **name** (`Commander`,
  `Sideboard`, …) because a `deck_card` has no board role; keep those spellings in step with
  `deck_import::parser`'s. Every deck-wide rule is skipped rather than guessed when the format
  has no profile or the command zone is empty, and "not finished yet" is a `warning` severity —
  a half-built deck must never be reported as illegal. The format table + the breach-severity
  order are **mirrored** in `web/src/lib/legality.ts` (a dropdown must not wait on a request)
  with tests pinning both sides, like `lifeLayout.ts`; `GET /api/games/{game}/formats`
  publishes the server's copy. The default library the odds and the goldfish shuffle is derived
  from `rules::deck_zone`, **never a second list of names** — a section the stats called
  non-library while the rules called it the command zone would deal an Oathbreaker deck its own
  oathbreaker. The **goldfish is stateless**: the hand is a pure function of
  `(seed, mulligans, what was bottomed, how many drawn)`, all in the query string, so there is
  no session table and a hand is reproducible from a URL — which is why its shuffle is a
  hand-rolled SplitMix64 + Fisher–Yates rather than `rand`, whose generators don't promise a
  stable stream across versions. Two consequences of that statelessness are load-bearing:
  the seedless public mirror answers **`no-store`** (a random seed makes the response not a
  function of its URL, so a CDN would pin one visitor's roll for everyone), and the shuffle
  is **bounded** — it materialises one slot per *copy* and a deck row's counts are
  caller-controlled, so an oversized library is a `422`, never an allocation. For the same
  reason the command-zone check counts copies instead of expanding them; **nothing on these
  paths may go per-copy.** Deck writes must invalidate the analysis query family
  client-side (`invalidateDeckAnalysis`); it doesn't sit under the `['deck', …]` key.
  Deck **import/export** (issue #389) lives in the sibling
  `deck_import/` pipeline: categories/boards become exact sections and a new deck is written
  whole, never through the `collection_items` reconcile engine. It reuses the lower provider
  throttling, foil, and card-resolution seams; imports are capped at 2000 source rows and return
  a lightweight deck header; Moxfield live URLs keep the collection import gate.
- **Price alerts** (`/api/alerts*`, issue #525) are **session-only** (`SessionUser` — never an
  API key: the channel settings hold delivery credentials) and **allow-listed out of the
  OpenAPI doc** (an account/session-flow surface, like username/currency). The engine is
  `crate::alerts` (evaluation) + `crate::notifications` (Discord/Telegram dispatch); email
  reuses `email::Emailer`. It's **edge-triggered**: fire once on the rising edge, re-arm when
  the price crosses back — don't make it re-notify every tick. Evaluate against the **live
  price column** on `cards`/`products` (compare in cents via `valuation::price_cents`), not the
  history tables; targets are stored by internal id and are orphan-tolerant. A user-supplied
  Discord webhook URL is **host-allow-listed** (`notifications::validate_discord_webhook_url`)
  at save **and** send, and dispatched over `AppState::notify_http` (**redirects disabled** +
  timeout) — an SSRF guard; don't route it through the redirect-following shared `http`.
  The webhook URL + Telegram bot token are **credentials**: keep them redacted in `Debug`
  (`alert_channel::Model` hand-writes it). Email is **off by default** (`ALERTS_EMAIL_ENABLED`,
  costs money at scale) and additionally needs `RESEND_API_KEY`. The evaluator scales to
  millions of alerts by **keyset-paginating** the armed set (memory is O(batch), never "load
  every alert + target") and **narrowing** each pass by a `since` cursor to alerts whose own
  row or whose target's `updated_at` changed. That narrowing is correct **only** while every
  live price write bumps the target's `updated_at`: the catalog upsert's "changed guard" does
  (bumps on a real datum change), and `scryfall::enrich_foil_variant_prices` was fixed to stamp
  it too (★-variant foil prices arrive only through that path) — **any new writer of
  `cards`/`products.price_usd*` must bump `updated_at` or narrowing silently misses fires.**
  Two more couplings: an undelivered met alert is `touch`ed so the retry-next-pass contract
  survives narrowing, and the `since` cursor advances **only** when `evaluate_all` returns
  `true` (a mid-scan DB error must re-scan, not skip).
- **Release heads-ups** (`crate::release_alerts`, `RELEASE_ALERTS_ENABLED`) are two per-user
  opt-ins on the **same `alert_channels` row** (`sld_release_enabled` / `set_release_enabled`,
  both default **off** — subscriptions, unlike the channel on/off flags that default on) that
  fire a **day-before** heads-up over the shared channel fan-out
  (`notifications::deliver_to_user`, extracted so price alerts and release alerts deliver
  through one path — don't re-fork it). **Edge-triggered via the `release_notifications`
  ledger**, one row per `(user, kind, ref_key)` written **only on successful delivery** (an
  undeliverable heads-up retries next pass, same latch-on-delivery contract as price alerts).
  Dates are **derived, not newly ingested**: a Secret Lair drop's date is the earliest
  `released_at` among its cards grouped by the runtime drop table; a set's is
  `card_sets.released_at`. Regular sets are **one notification per theme** — top-level only
  (`parent_set_code IS NULL`), a curated set-type allow-list, non-digital, never `sld` (drops
  handle that per-drop). Session-only channel settings, like price alerts; the two flags ride
  the `AlertChannels` DTO, so they're already in the OpenAPI `INTENTIONALLY_UNDOCUMENTED` group.
- **Tools** (`/api/tools/{game}/...`) is a *namespace*, not a surface: play aids backed by the
  caller's own rows, grouped so a second tool adds a path segment rather than a new top-level
  route family (the API mirror of the SPA's `/tools` section, placed the way `/keywords` is).
  Today it holds the **life counter** — a container surface like decks (`life_sessions` /
  `life_session_players` / `life_events`), not a holdings twin, so it rides no `makeHoldingApi`.
  A seat and an event have **no `user_id`** (they hang off `session_id`), so every seat/event
  route must `load_session` first — a foreign id is **404, not 403**. Four invariants:
  **(1) a finished session is immutable** — every life/seat/undo write gates on
  `require_active` and answers **409**, because a recorded result already counts towards the
  per-deck record; start a rematch (`from_session_id`) instead. **(2) `life` is written in
  exactly two places** — a tap appends one event and moves the seat by its delta, and an undo
  re-folds the seat's whole chain through the pure `life/replay.rs` fold (which is why the fold
  honours `set` as an absolute and `adjust` as relative, and clamps rather than overflowing);
  nothing else may write it — and that survived the arrival of a **second counter axis** (#595)
  precisely because the other counters got **no column**. **(3) a seat names what was played in one of two mutually exclusive
  ways** — `deck_id` (one of *yours*, which is what builds a record) or `commander_card_id` (for the
  opponents whose deck you'll never have); both at once is a **422**, because a deck already knows
  its commander and the pair would surface as a wrong record rather than an error. **(4) both links
  are FK-less and orphan-tolerant** (the call `price_alerts.card_id` makes) — deleting a played deck,
  or a re-import dropping a card row, must neither fail nor delete history, so reads report the link
  absent and `life/stats.rs` inner-joins `decks` *scoped to the caller*. A **rematch** distinguishes
  a *copied* reference (dropped once it stops resolving, so an old pod stays re-playable) from an
  *explicit* one (still a `404`).
  **Counters beyond life** (#595 — poison/energy/experience, plus commander damage keyed by the
  *source* seat) ride `life_events.counter` + `source_player_id` rather than five more seat columns,
  so invariant 2 holds unchanged: they're folded out of the history by `replay_seat` (one chain per
  `(counter, source)`, each from its own start — `starting_life` for life, `0` for the rest — and
  within its own bounds, where only `life` may go negative), and a commander-damage tap deliberately
  does **not** move the target's life (the player reconciles it, as at a real table). Damage is per
  commander, so 7 from one and 6 from another is never 13 from either — that's what the 21 threshold
  is measured against. The source link is FK-less and orphan-tolerant like invariant 4's pair: a seat
  that leaves cascades away with *its own* events but not with the damage it dealt. Which counters a
  game tracks is `life_sessions.counters` (CSV, defaulted from `format`, `life` implicit and never
  listed); writing an untracked one is a **422**, switching one off keeps its recorded values (the
  SPA shows any counter still holding one), and a lethal threshold (21 damage, 10 poison) only ever
  *suggests* a result — a session must never finish itself, since a recorded result is immutable.
  The `layout` slug vocabulary + the per-count layout and per-seat rotation defaults are
  **mirrored** in `web/src/lib/lifeLayout.ts` with tests pinning both sides — as is the counter
  vocabulary in `web/src/lib/lifeCounters.ts`; a slug added on one
  side only is either rejected by the API or renders as something other than its name. The SPA
  batches taps into **one** committed change per run (`composables/useLifeTaps.ts`, keyed by the
  same `(seat, counter, source)` chain the server folds, so a 7-point commander hit is one row) and
  deliberately does **not** retry a failed commit — a request that failed in transit may still
  have applied, so re-sending could double the loss.
- **Every export is a file-download response through `handlers/shared/download.rs`**
  (`csv_download`/`text_download`) — don't re-roll the Content-Type + Content-Disposition
  pair. The **card-search `.txt` export** (`/api/games/{game}/cards/export` and its
  `.../sets/{code}/cards/export` sibling) is a *public catalog* read that must keep building
  its query from the listing's own builders (`catalog::cards::all_cards_query` /
  `catalog::sets::set_cards_query`) — a second query here means the file can silently
  disagree with the grid it was exported from. It is **uncapped and streamed**: rows drain
  through **one** SeaORM row stream into ~500-line chunks on a bounded channel, so peak
  memory is a chunk, not a result set — and it selects **only** the three columns a line
  needs, not all ~70 (that alone is 12x on a full-catalog drain). **Never hold a DB
  connection while awaiting the client:** SeaORM's row stream owns its `PoolConnection` for
  the stream's life and sea-orm pins SQLite (the default) to *one* connection, so streaming
  a single query to the client let one slow reader take the whole API down. Hence the
  two-phase drain — resolve ids in one query, then re-acquire per chunk — with the send
  happening outside any checked-out connection. Bounded channel *and* connection-free
  awaits; either alone is not enough. Don't give
  the response a size hint (that's what stops `conditional_request_layer` buffering the
  whole thing to compute an `ETag`), and don't turn a mid-stream failure into silence: it
  appends a `#`-comment marker **and** errors the transfer, so a short file is never
  mistaken for a whole one. Otherwise the body stays pure card lines so a paste is clean.
  That whole drain lives in **`handlers/shared/card_export.rs`**, shared with the
  **collection/wish-list card exports** (`/api/{collection,wishlist}/{game}/cards/export`,
  authed + no-store): those build from the twins' own listing builders through
  `resolve_holdings_list` + `narrow_export_statement`, and their `text` lines carry the
  **real held counts** — one line per non-empty finish, foil tagged ` *F*` (the grammar
  `collection_import::text_list` reads back) — where a catalog line is always `1 …`.
  A view served by a **different endpoint** than the one the export reuses must **hide**
  the button rather than hand back a file that isn't the rows on screen — that's why
  `SetView` gates on `!grouped`: `/drops` owns a `?drop=` filter the export can't express,
  and `/subtypes` parses the search's `unique:`/`order:` directives and then *discards*
  them, so a `q=unique:cards` grid shows every printing while the export would fold them.
  The holdings browse views gate the same way (`!grouped`), and additionally swap target
  per mode: held mode exports the holdings listing, show-ghosts mode *is* the catalog
  listing so it exports the public catalog search.
- A replace-mode import matching **zero** catalog cards is refused (wipe guard);
  **smart sync never deletes** upstream-removed cards — only a full replace does.
  Moxfield **URL** import is deliberately disabled
  (`Provider::network_import_enabled()` is the switch; CSV is the supported path) —
  a 422 there is not a regression.
- **Not every `Provider` fetches.** Mythic Tools (issue #572) has no public API, so it's
  file/paste-only: `network_import_enabled()` is `false` and every fetch/link path gates on
  that before dispatching. Its collections arrive through `execute_file_import`, which backs
  **both** `/import/csv` (upload) and `/import/text` (paste) and **sniffs** the format from
  the content — Mythic Tools CSV (its `Amount` column is the fingerprint, checked before
  Archidekt's Scryfall ID because its export has both), Archidekt CSV, Moxfield CSV, then a
  plain-text card list as the fallback. Keep the text list *last* or a real CSV silently
  degrades into it. That line grammar lives in `collection_import::text_list` and is
  **shared with `deck_import::parser`** — extend the seam, don't fork a second dialect. A
  text line naming no printing resolves to the newest printing of that name
  (`reconcile::resolve_newest_printing_by_name`, also shared with deck import) — which must keep
  **excluding foil-`…★` variants**, or `4 Sol Ring` silently imports as four *foils* (the star
  shares its base's name and date, wins the id tie-break, and `consolidate` folds it on as foil).
  A line that *did* name a printing must stay unmatched when it doesn't resolve — never fall back
  to another art at another price. A Mythic Tools CSV must carry a `Finish` column (its export
  columns are user-selectable), same refusal Moxfield's `Foil` column gets.
- Card images are cached lazily on first view — self-hosts **never bulk-download**;
  image fetches are host-allow-listed with redirects disabled. **Don't add any bulk
  image path** — the one sanctioned exception is the opt-in fingerprint build
  (`FINGERPRINT_BUILD_ENABLED`, default off); read `docs/tradeoffs.md` §Visual card
  scanner before touching it.
- **Secret Lair drop titles are a runtime overlay, not just a static file.** They aren't in
  the bulk card API, so `scryfall::drops` is a swappable `RwLock<Arc<Tables>>` **seeded** by the
  committed `scryfall/sld_drops.json` (still the offline fallback; `gen-sld-drops.mjs` regenerates
  it) and **swapped** daily: the mirror origin (`MIRROR_ENABLED`) scrapes Scryfall's gallery
  (`scryfall::sld_scrape`) and serves it at `/api/mirror/scryfall/sld-drops`; every other instance
  imports it from the mirror (`scryfall::sld_sync`, `SLD_DROPS_IMPORT_ENABLED`, default on) — a
  self-host **never scrapes Scryfall itself**. `install_snapshot` **rejects a snapshot missing the
  `mtg/sld` set**, so a broken scrape can't wipe the good table. Each successful scrape/import is
  **persisted** to the DB (`scryfall::sld_persist`, the `sld_drop_snapshot` singleton table) and the
  store is **reseeded from that persisted snapshot at boot** (in `scryfall::sld_tasks`, before the
  `initial_delay` deferral) — so a restart serves the last-good drops, not the committed seed, and
  the deferral/`304` stay correct; the committed file is only the first-boot/offline fallback. Don't
  drop the reseed-before-defer ordering or persist on a `304`/`Unchanged`. `drops::table()` returns an
  owned `Arc<DropTable>`, and `sld::derivation_version` reads the **live** snapshot (computed, not
  memoised) so a refresh propagates to the sealed-contents gate — keep both dynamic.
- **Every field of a Scryfall bulk-catalog entry is optional, and the files are gzipped
  JSONL.** `cards`, `rulings` and `art_tags` all start from the one `/bulk-data` document, so
  a *required* field there is a single point of failure for the whole catalog: when upstream
  swapped `download_uri`/`size` for `jsonl_download_uri`/`compressed_size` (2026-07), serde
  rejected the list and every import died as "network error contacting the card-data source"
  — silently, since existing rows just went stale. Read the location through
  `BulkData::file_url`/`transfer_size`, never the fields. The files are served
  `Content-Type: application/gzip` with **no `Content-Encoding`** (so reqwest's transparent
  gzip never applies) and the mirror passes those bytes through compressed: `client::json_lines`
  is the one seam that sniffs the gzip magic byte and inflates, and every bulk consumer must
  read through it rather than wrapping the stream itself. `cargo test -- --ignored
  live_bulk_catalog` is the manual canary when an import starts failing with a decode error.
- `SEED_DUMMY_DATA` is upsert-only — point it at a fresh/dedicated DB.
- Dep pins: `jsonwebtoken` keeps `default-features = false` with exactly one crypto
  provider (`aws_lc_rs`, shared with rustls); enabling no provider panics and enabling
  both providers requires manual selection. `reqwest` deliberately has **no** overall
  timeout (streaming bulk downloads) — don't "fix" that when bumping.

## Conventions

- **TS/Vue:** no semicolons, single quotes, 2-space indent, max 100 cols; `<script
  setup lang="ts">`, Pinia setup stores, `@/` → `src/`. Run `npm run format` then
  `npm run lint` after editing.
- **Rust:** edition 2024; errors flow through `AppError`; `expect` only in `main.rs`
  startup. Add deps with `cargo add`.

## Environment variables

Full reference: `docs/operations.md`. Dev essentials: `JWT_SECRET` (required,
≥ 32 bytes; `ALLOW_INSECURE_DEV_SECRET=true` for local dev) · `SEED_DUMMY_DATA=true`
(offline dummy catalog + the seeded e2e account; overrides syncing) ·
`RESEND_API_KEY` or the Cloudflare Email Service pair (both unset = the email dev bypass above).
