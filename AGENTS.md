# AGENTS.md

Guidance for working in this repository. This file is the always-loaded core and is kept
deliberately short; the detail lives in `docs/` — read the relevant one **before** working
in its area:

- [`docs/invariants.md`](./docs/invariants.md) — the full text of every "don't break this" rule listed one-line-each below: the seams that enforce it and the couplings a change must keep in step
- [`docs/tradeoffs.md`](./docs/tradeoffs.md) — the *why* behind those rules and every deliberate trade-off; read it before "fixing" anything that looks odd
- [`docs/api-contracts.md`](./docs/api-contracts.md) — every HTTP endpoint, wire shape, the search syntax, caching/ETag/sitemaps, import mechanics
- [`docs/architecture.md`](./docs/architecture.md) — the annotated file map for `api/src/` and `web/src/`, plus test organization
- [`docs/design-system.md`](./docs/design-system.md) — design tokens (`web/src/assets/main.css`), the status/foil/rarity color vocabulary, chart-palette validation, and the artifacts coupled to any palette change
- [`docs/operations.md`](./docs/operations.md) — running, commands, CI, e2e, releases, Docker, and the full environment-variable reference (authoritative: `api/src/config.rs`, `api/.env.example`)
- Self-hosting / deploying: [`docs/self-hosting.md`](./docs/self-hosting.md) (deploy hub), then [`docs/deploy-digitalocean.md`](./docs/deploy-digitalocean.md) (Droplet, recommended) · [`docs/deploy-app-platform.md`](./docs/deploy-app-platform.md) (PaaS)

**TCGLense** tracks trading-card games: a card catalog (MTG first, via Scryfall),
singles + sealed-product price history (TCGCSV, MTGJSON), per-user collections and a
wish list (Archidekt/Moxfield/CSV import), decks with server-side analysis, preconstructed
decks, price alerts, play-aid tools, email-first auth (Turnstile + rate limiting), and a
public API with scoped `tcgl_` API keys (OpenAPI at `/api/openapi.json`, Scalar UI at the
SPA's `/docs`).

| Dir    | App                     | Stack |
|--------|-------------------------|-------|
| `api/` | Backend (HTTP JSON API) | Rust 2024 · axum 0.8 · SeaORM 1.1 · SQLite by default, Postgres picked at runtime by the `DATABASE_URL` scheme · JWT (HS256) · Argon2 |
| `web/` | Frontend (SPA)          | Vue 3.5 · Vite 8 · Pinia · TanStack Query (vue-query) · vue-router · Tailwind 4 · shadcn-vue · TypeScript |

The `tcglense` **CLI + TUI** client lives in its own repository —
[PNRxA/tcglense-cli](https://github.com/PNRxA/tcglense-cli) — and is **not** in this tree.

**Search trap:** `.claude/` is gitignored but holds nested full-repo worktrees. Scope
repo-wide greps/finds to `api/` and `web/`, and never edit a file through a
`.claude/worktrees/…` path — that silently changes a different branch's checkout.

**No private memories:** don't stash project knowledge in a per-user/agent memory store —
put it in the repo (this file or `docs/`) via a PR so every agent sees it. A new
load-bearing rule goes in `docs/invariants.md` **plus** a one-liner in the list below.

**Scoping a "review since `<tag>`":** pin the commit range and re-verify `HEAD` before you
finalize — `main` advances by squash-merge, so a `git diff v<x>..HEAD` scope can grow while
you work.

## Run & verify

```sh
cargo run        # api/ — :8080; migrations on boot; needs a real JWT_SECRET; first run: cp .env.example .env
npm run dev      # web/ — :5173, Vite proxies /api; first run: npm install (Node ^22.18 or ≥24.12)
```

`./scripts/dev.sh` runs both (and refuses already-taken ports — a stale server holds the
port). Default DB: `api/tcglense.db` (SQLite; WAL sidecars are normal). Everything else
(Postgres, command matrix, CI, releases): `docs/operations.md`.

**Before calling a change done:** `cargo check` + `cargo test` for `api/` work;
`npm run type-check && npm run lint && npm run test:unit -- --run` for `web/` work.
Also format: `cargo fmt --all` (in `api/`) and `npm run format` (in `web/`) — CI's `format`
job gates both. CI runs the test suites, a **ts-rs drift check** (generated types in
`web/src/lib/api/generated/` must match the Rust DTOs) and the `format` gate, but **not**
lint/clippy — the checklist above is the only thing catching those.

**e2e gotcha:** Playwright starts only the *web* server; run the API yourself with
`SEED_DUMMY_DATA=true`, or the specs **silently skip**. Gate on `/api/ready`, never
`/api/health` — details in [`docs/operations.md`](./docs/operations.md#running-the-e2e-tests-locally).

## Where code lives

Skeleton only — the full annotated map is [`docs/architecture.md`](./docs/architecture.md).

- `api/src/`: `router.rs` (every route + middleware) · `handlers/` (incl. `shared/` seams
  and `tools/`) · `entities/` + `migrator/` · `auth/` · `catalog/` (GAMES registry +
  per-game dispatch) · providers (`scryfall/`, `tcgcsv/`, `mtgjson/`, `spellbook/`) ·
  `collection_import/` · `deck_import/` · `security_tests/` (HTTP-level suites driving the
  real router).
- `web/src/`: `views/` + `router/` · `components/` · `composables/` (query hooks) ·
  `stores/` (Pinia) · `lib/api/` (typed client; `generated/` = ts-rs wire types).

## Adding a backend feature

1. **Entity:** `entities/<name>.rs` (`DeriveEntityModel`); export from `entities/mod.rs`
   **and** `entities/prelude.rs`.
2. **Migration:** the date prefix is **frozen** — files are `m20240101_0000NN_<name>.rs`;
   increment only the counter. Register in `migrator/mod.rs` in **two places**: a `mod`
   line + a `Box::new(...)` entry in `migrations()`.
3. **Handler + route:** module under `handlers/`, wired in `router.rs`. Return `AppError` —
   never `unwrap`/`expect`/`panic!` on a request path. SeaORM query API only (anything raw
   goes through `db::Dialect` or it breaks one backend). Use `JsonBody<T>`, not raw
   `Json<T>`. Pick the right cache group in `handlers/cache.rs`; consider a
   `security_tests/` suite. Any public/API-key JSON endpoint needs a `#[utoipa::path]`
   (+ `ToSchema` on its DTOs, a `__path_*` re-export from the group `mod.rs`) registered in
   `openapi.rs` — its `coverage_drift` test fails until every route is documented or in
   `INTENTIONALLY_UNDOCUMENTED` with a reason.
4. **Wire types:** derive `ts_rs::TS` on response DTOs (`#[cfg_attr(test, derive(ts_rs::TS))]`).

Adding a TCG = a `Game` in `catalog::GAMES` + a provider module + one arm each in
`catalog::refresh_all` and `catalog::seed_all`.

## Adding a frontend feature

- **Wire types are generated:** derive `ts_rs::TS` on the Rust DTO, run `cargo test` from
  `api/` (only `cargo test` regenerates). Never hand-edit `lib/api/generated/*.ts` —
  **except** `generated/index.ts`, a hand-maintained barrel (as is `lib/api/index.ts`).
- **Server state → vue-query** via `useAuthedQuery`/`useAuthedMutation` (public pages use
  plain `useQuery`; never `authFetch` directly for reads). Invalidate dependent queries
  after mutations; set a per-query `staleTime`. Reactive params go **inside** `queryKey`
  as refs/computed, never `.value`. **Client state → Pinia**; never duplicate a datum in
  both. Do **not** wrap `stores/auth.ts`'s refresh in vue-query.
- **Pages:** view under `views/` + route in `router/index.ts`; authed pages need
  `meta: { requiresAuth: true }`; per-view head tags via `usePageMeta()`.
- **UI primitives:** `npx shadcn-vue@latest add <name>`; hand-written ones copy
  `components/ui/button/Button.vue`. Don't import `@vueuse/core` (transitive only); use
  `defineModel` for v-model.
- **Color is tokens, never palette classes:** `success`/`warning`/`info`/`destructive`,
  `foil`, `rarity-*` (chip idiom `bg-<token>/15 text-<token>`), never `emerald-*`/`amber-*`
  literals — `docs/design-system.md`.

## Keep it maintainable

- **Collection and wish list are twin surfaces — extend the shared engine, never
  copy-paste one to build the other.** Seams: `handlers/shared/holdings.rs`,
  `web/src/lib/api/holdings.ts` (`makeHoldingApi`), `composables/holdingQueries.ts`
  (`makeHoldingQueries`), `useHoldingsLanding`/`useHoldingsBrowse`. Asymmetries are
  flags/config in the seam, not a forked file.
- **Rule of three:** grep for an existing seam before hand-rolling a helper
  (`auth/secret.rs`, `catalog/ingest_state.rs`, `handlers/shared/`); a third copy means
  extract it.
- **One concern per file:** split orchestration / pure algorithm / bookkeeping (the
  `ratelimit/` and `mtgjson/ingest/` directory modules are the pattern). ~500 lines is the
  smell threshold — judge cohesion, not length.
- **Views stay thin:** engines in composables, repeated markup in presentational
  components; a `<script setup>` past ~150 lines is probably an unextracted engine.
- **Refactors are pure moves:** verified by the full suites and a zero-diff
  `web/src/lib/api/generated/` after `cargo test`.

## Don't break these

One line each. The full rule, its seams and its couplings are in
[`docs/invariants.md`](./docs/invariants.md) — **read the section before touching the
area**; rationale: `docs/tradeoffs.md`; wire shapes: `docs/api-contracts.md`.

**[Auth, email, API keys, rate limiting](./docs/invariants.md#auth-email-api-keys-and-rate-limiting)**
- Auth answers generically; password rules are validated before an email token is consumed; a missing/bad CAPTCHA token is **400**.
- No email provider = the local dev bypass; internet-facing configs refuse signups without a provider, a non-default `EMAIL_FROM` and Turnstile.
- API-key scope is the **extractor** (`AuthUser` reads, `WritableUser` writes → read-only key 403, `SessionUser` key management), never the HTTP method; bad key = 401; store only the hash; keep the per-user limiter `tcgl_`-aware.
- Rate limiting **fails open**, CAPTCHA **fails closed**; `TRUST_PROXY_HEADERS` only behind a proxy that overwrites `X-Forwarded-For`.

**[Collection & wish list](./docs/invariants.md#collection-and-wish-list)**
- Independent tables sharing the DTOs in `handlers/shared/holdings.rs`; external card ids; both counts zero deletes the row.
- A filter on *what is held* is a `ListParams` field resolved in `resolve_holdings_list`, **never a `q:` leaf** — the search compiler is shared with the CDN-cached catalog.
- The breakdown (`handlers/shared/breakdown.rs`) rides `analytics_cache` per surface — every wish-list card write must `bump_surface_holdings(Wishlist, …)`.
- Sealed-product holdings and public sharing ride the lower shared seams (`shared/product_holdings.rs`, `ProductHoldingSection`), gated by the same visibility flag.

**[Sealed products & booster odds](./docs/invariants.md#sealed-products-and-booster-odds)**
- **No number on a sealed product's page is a count of a copy's physical cards** (a per-pack expectation and one seeded simulation are the only exceptions, both qualified). `lib/productCounts.ts` is the one wording seam and mirrors `CardSection::classify`; sections split by source at ingest; exclusivity is a stored column stamped per sync tick, never re-derived in a read.
- Booster tables are rebuilt wholesale (ids never reach the wire; a flatten/variant/weight change needs a `DERIVATION_VERSION` bump); `total_weight` keeps unresolved cards; `fixed` sheets keep stored order; bounds (36 packs / 1200 cards) answer 422 before drawing; seedless `/open` is `no-store`; `/ev` answers `{ data: null }`, never 404; the shared RNG stream is a wire contract.

**[Decks](./docs/invariants.md#decks)**
- A container surface, not a holdings twin; `deck_card` has no `user_id`, so every route `load_deck`s first — foreign = **404**. Maybeboard is a column every "what is this deck" reader skips.
- A deck's colours are its command zone's when `rules::format_leads_with_command_zone`, else the union over the deck proper; every `DeckResponse` builds through `deck_headers`.
- Analysis is server-side (`handlers/decks/analysis/`): each read a `GET` on `AuthUser`, mirrored under `/api/u/{handle}` through the same `analyse_*` core (suggestions excepted — it reads the caller's collection). Deck writes invalidate the analysis query family client-side.
- The analysis stack sits behind one collapsible (`DeckOverview.vue`) whose chips are pure functions of the panel responses (`lib/deckOverview.ts`).
- Mana base cites Karsten's table; tokens and combos are provider data (NULL ≠ `[]`, `available: false` ≠ "no combos"); roles + bracket signals share one clause grammar (`analysis/signals/`) that declines when unsure; the bracket is a floor, only ever 2/3/4; legality = `legality` (per card) + `rules` (per deck), names via `rules::answers_to`; Rulebreaker effects read off the command zone only.
- The goldfish is stateless and bounded (SplitMix64 + Fisher–Yates, seedless mirror `no-store`, oversized library 422) — nothing on these paths goes per-copy.
- Pricing's `total_usd` **is** `summary.total_value_usd`; cheapest is judged at the row's finish split; "swap all" is the per-card `PUT` batched client-side.
- Every clone goes through `decks::copy::insert_deck_with_cards`; the diff folds by card name; adding a deck/precon to the collection is `collection_import::merge_holdings` (additive, not idempotent, `Import` rate class).
- Import/export is the sibling `deck_import/` pipeline (2000-row cap), never the `collection_items` reconcile engine.

**[Preconstructed decks](./docs/invariants.md#preconstructed-decks)**
- Catalog side, derived from MTGJSON's `decks[]` in the sealed sync and rebuilt wholesale: `slug` is the identity (a derivation change bumps `DERIVATION_VERSION`); `precon_overlay` fills upstream's dangling references and retires itself.
- Facets fold at ingest; the tile price is re-folded from live prices each tick (`precon_values`); every route shape reads one `filtered_query`.
- A row is a single finish — everything that makes deck rows folds by card; board → section is decided once in the copy (`Commander`/`Sideboard`); Secret Lair drops are dropped at derivation (`NOT_A_DECK_TYPES`).

**[Price alerts & release heads-ups](./docs/invariants.md#price-alerts-and-release-heads-ups)**
- Session-only, out of OpenAPI, edge-triggered against the live price column; Discord webhooks host-allow-listed and sent over `notify_http` (no redirects); credentials redacted in `Debug`; email off by default.
- The evaluator keyset-paginates and narrows by `updated_at` — **any new writer of `cards`/`products.price_usd*` must bump `updated_at`.**
- Release heads-ups are two opt-ins on the same `alert_channels` row, latched in `release_notifications` on delivery; what counts as a release is decided once in `catalog::releases`, shared with the public calendar.

**[Tools: the life counter](./docs/invariants.md#tools-the-life-counter)**
- Seats/events hang off `session_id` (`load_session` first, foreign = 404); a finished session is immutable (409); `life` is written in exactly two places (tap + replay fold); `deck_id` xor `commander_card_id` (both = 422); links are FK-less and orphan-tolerant; extra counters ride `life_events.counter`, never new columns; layout + counter vocabularies are mirrored in `lib/lifeLayout.ts` / `lib/lifeCounters.ts`.

**[External ids, shopping lists & exports](./docs/invariants.md#external-ids-shopping-lists-and-exports)**
- Provider ids ride `CardDetailResponse` only; a buy list is rows with `tcgplayer_id` (stores in `lib/bulkBuy.ts`); the deck buy list reuses the `needed_rows` fold.
- Every export goes through `handlers/shared/download.rs`; the card `.txt` export builds from the listing's own query builders, is uncapped and streamed in chunks, and **never holds a DB connection while awaiting the client**; a view served by a different endpoint hides the button.

**[Cards: foil folds, etched, the shared DTO](./docs/invariants.md#cards-foil-folds-etched-prices-and-the-shared-card-dto)**
- A foil-★ star folds onto its base only under `foil_variants::same_printed_card`, decided once per tick into `cards.folded_onto_id`; every grid builds from `catalog_cards`; by-id lookups are exempt; set counts subtract folds via `FoldedSetCounts`; a non-provider `cards` column is denied in **both** halves of `flush_cards`.
- Etched is a price, not a holding (held and valued as foil); USD only.
- Per-printing detail goes on `CardDetailResponse`, never the shared `Card`.

**[Collection import](./docs/invariants.md#collection-import)**
- Replace-mode matching zero cards is refused; every import is one-off; Moxfield URL import is disabled by design (a 422 there is not a regression).
- Mythic Tools and ManaBox are file/paste-only; sniff order is Mythic → ManaBox → Archidekt → Moxfield → text list (text **last**); the line grammar is `text_list`, shared with deck import; a nameless line resolves to the newest non-★ printing, a named printing never falls back.

**[Search & indexes](./docs/invariants.md#search-and-indexes)**
- The universal search composes each surface's own seam under the every-word substring rule, never the Scryfall grammar; the card leg widens by set/number behind three guards; SQL stays bounded and indexed (`indexed_name_like`, bound `set_code IN`); the response is the same for every visitor.
- A leaf that joins another table needs an index on **both** sides; an expression leaf and its index come from one seam (changing it needs a drop-and-recreate migration); a surface that never shows the count reads `/cards/preview`.

**[Images, Secret Lair drops & ingest](./docs/invariants.md#images-secret-lair-drops-and-catalog-ingest)**
- Card images cache lazily; never add a bulk image path (the opt-in fingerprint build is the one exception).
- SLD drop titles are a runtime overlay: the mirror origin scrapes, everyone else imports; persisted and reseeded at boot; `sld` is primary, secondary sets carry forward; the derivation gate hashes `sld` only.
- Every Scryfall bulk field is optional: read through `BulkData::file_url`/`transfer_size` and `client::json_lines`; one `BulkCatalog` fetch per tick; a failed fetch writes no `ingest_state` error.
- The card-sync tick self-heals — keep all four legs (anchored schedule, deadline, snapshots before **and** after, keepalive lock); `record_completed` only when every pass succeeded; the TCGCSV backfill is gap-aware.
- `SEED_DUMMY_DATA` is upsert-only; `jsonwebtoken` keeps exactly one crypto provider; `reqwest` has no overall timeout.

## Conventions

- **TS/Vue:** no semicolons, single quotes, 2-space indent, max 100 cols; `<script setup
  lang="ts">`, Pinia setup stores, `@/` → `src/`. Run `npm run format` then `npm run lint`.
- **Rust:** edition 2024; errors flow through `AppError`; `expect` only in `main.rs`
  startup. Add deps with `cargo add`.

## Environment variables

Full reference: `docs/operations.md`. Dev essentials: `JWT_SECRET` (required, ≥ 32 bytes;
`ALLOW_INSECURE_DEV_SECRET=true` for local dev) · `SEED_DUMMY_DATA=true` (offline dummy
catalog + the seeded e2e account; overrides syncing) · `RESEND_API_KEY` or the Cloudflare
Email Service pair (both unset = the email dev bypass).
