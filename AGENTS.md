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
CI runs the test suites **plus a ts-rs drift check** (generated types in
`web/src/lib/api/generated/` must match the Rust DTOs) but does **not** run
lint/format/clippy — the checklist above is the only thing catching those.

**e2e gotcha:** Playwright starts only the *web* server. Start the API yourself with
`SEED_DUMMY_DATA=true` first — the specs probe `/api/health` and **silently skip**
when it's unreachable, so the suite can "pass" without testing anything.

## Where code lives

Skeleton only — the full annotated map is [`docs/architecture.md`](./docs/architecture.md).

- `api/src/`: `router.rs` (every route + middleware) · `handlers/` · `entities/` +
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
- No `RESEND_API_KEY` = the email dev bypass (register returns the completion token;
  login skips the verified gate). **Never run prod without the key** (anyone could
  activate any address).
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
  `handlers/shared/holdings.rs` — editing a shared shape changes both wire surfaces.
  Holdings use **external** card ids; both counts zero deletes the row. The wish list
  additionally holds **sealed products** in its own `wishlist_product_items` table
  (`/api/wishlist/{game}/products*`, external TCGplayer ids on the wire, same
  both-zero-deletes rule); the collection deliberately has **no** sealed surface — don't
  add one.
- **Decks** (`/api/decks/{game}*`, issue #363) are a **container** surface — many per user,
  in `decks`/`deck_sections`/`deck_cards`/`deck_folders` — **not** a collection/wishlist twin,
  so they don't ride `makeHoldingApi`/`makeHoldingQueries`; they live beside it and only reuse
  the *lower* seams (`deck_card::Model` impls `HoldingCounts`; the `Card` DTO). A `deck_card`
  has **no `user_id`** (it hangs off `deck_id`), so every deck route must `load_deck` to prove
  ownership first — a deck that isn't the caller's is **404, not 403**. Per-deck sharing is an
  `is_public` **column** on the deck row (not a `collection_visibility`-style table — a deck is
  1:1 with the shareable unit), public at `/api/u/{handle}/decks/{id}` (username-first `409`,
  reusing #361's `resolve_public_user`). Deck **import/export** (issue #389) lives in the sibling
  `deck_import/` pipeline: categories/boards become exact sections and a new deck is written
  whole, never through the `collection_items` reconcile engine. It reuses the lower provider
  throttling, foil, and card-resolution seams; imports are capped at 2000 source rows and return
  a lightweight deck header; Moxfield live URLs keep the collection import gate.
- A replace-mode import matching **zero** catalog cards is refused (wipe guard);
  **smart sync never deletes** upstream-removed cards — only a full replace does.
  Moxfield **URL** import is deliberately disabled
  (`Provider::network_import_enabled()` is the switch; CSV is the supported path) —
  a 422 there is not a regression.
- Card images are cached lazily on first view — self-hosts **never bulk-download**;
  image fetches are host-allow-listed with redirects disabled. **Don't add any bulk
  image path** — the one sanctioned exception is the opt-in fingerprint build
  (`FINGERPRINT_BUILD_ENABLED`, default off); read `docs/tradeoffs.md` §Visual card
  scanner before touching it.
- `SEED_DUMMY_DATA` is upsert-only — point it at a fresh/dedicated DB.
- Dep pins: `jsonwebtoken` keeps `default-features = false, features =
  ["rust_crypto"]` (panics at runtime otherwise); `reqwest` deliberately has **no**
  overall timeout (streaming bulk downloads) — don't "fix" either when bumping.

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
`RESEND_API_KEY` (unset = the email dev bypass above).
