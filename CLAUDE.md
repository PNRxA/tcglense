# CLAUDE.md

Guidance for working in this repository. This file is the always-loaded core; the
detail lives in `docs/` — read the relevant one before working in its area:

- [`docs/api-contracts.md`](./docs/api-contracts.md) — every HTTP endpoint, wire shape, the search syntax, caching/ETag/sitemaps, import/sync mechanics
- [`docs/architecture.md`](./docs/architecture.md) — the fully annotated file map for `api/src/` and `web/src/`, plus test organization
- [`docs/operations.md`](./docs/operations.md) — running, CI, releases, Docker/deploy, and the full environment-variable reference
- [`docs/tradeoffs.md`](./docs/tradeoffs.md) — known trade-offs and design rationale; read it before "fixing" anything that looks odd

**TCGLense** tracks trading-card games: a card catalog (MTG first, via Scryfall),
singles + sealed-product price history (TCGCSV, MTGJSON), per-user collections
(Archidekt/Moxfield import + CSV upload) and a wish list, behind email-first auth
(Turnstile CAPTCHA + rate limiting). Not yet built: set-completion progress.

## Layout

| Dir    | App                     | Stack |
|--------|-------------------------|-------|
| `api/` | Backend (HTTP JSON API) | Rust 2024 · axum 0.8 · SeaORM 1.1 · SQLite by default, Postgres picked at runtime by the `DATABASE_URL` scheme · JWT (HS256) · Argon2 |
| `web/` | Frontend (SPA)          | Vue 3.5 · Vite 8 · Pinia · TanStack Query (vue-query) · vue-router · Tailwind 4 · shadcn-vue · TypeScript |

Dev: API on `:8080`, web on `:5173`; the Vite dev server proxies `/api` → the API so
the browser is same-origin and the httpOnly refresh cookie stays first-party.

**Search trap:** `.claude/` is gitignored but holds nested full-repo worktrees. Scope
repo-wide greps/finds to `api/` and `web/`, and never edit a file through a
`.claude/worktrees/…` path — that silently changes a different branch's checkout.

## Run & verify

```sh
# API, from api/:   cp .env.example .env   # first run only
cargo run        # :8080; migrations run on boot; refuses to start without a real JWT_SECRET (≥ 32 bytes)
# Web, from web/:   npm install            # first run only (Node ^22.18 or ≥24.12)
npm run dev      # :5173
```

`./scripts/dev.sh` runs both (and refuses to start if the ports are already taken —
a stale server holds the port). The default DB is `api/tcglense.db` (SQLite; WAL
sidecar files are normal).

**Before calling a change done:** `cargo check` + `cargo test` for `api/` work;
`npm run type-check && npm run lint && npm run test:unit -- --run` for `web/` work.

**CI** runs `cargo test --locked` **plus a ts-rs drift check** (the generated types
in `web/src/lib/api/generated/` must match the Rust DTOs), the env-gated
Postgres/Redis tests, web type-check + vitest, and Playwright e2e. CI does **not**
run lint/format/clippy — the checklist above is the only thing catching those.

**e2e gotcha:** Playwright starts only the *web* server. Start the API yourself with
`SEED_DUMMY_DATA=true` first — the specs probe `/api/health` and **silently skip**
when it's unreachable, so the suite can "pass" without testing anything.

## Backend map (`api/src/`)

Full annotations: `docs/architecture.md`.

```
main.rs / router.rs    bootstrap · every route + the middleware stack (router.rs is factored out of main so the security tests drive the exact same router in-process)
tasks.rs               6h maintenance loop + startup/periodic card-data sync, or the offline dummy seed
config.rs / state.rs   env config (secrets redacted in Debug) · AppState — the one construction site, shared with the test harness
db.rs                  SQLite (WAL + REGEXP UDF) or Postgres, picked at runtime; db::Dialect is the seam for any raw/backend-specific SQL
datasets.rs            SyncSource seam: providers fetch datasets from upstream or from a TCGLense mirror
captcha.rs / client_ip.rs / ratelimit.rs / email.rs   Turnstile CAPTCHA · client-IP resolution · per-IP + per-user rate limiters · Resend email (each enum-dispatched with Disabled/test variants)
error.rs / extract.rs  AppError → JSON { error } · JsonBody<T>, the JSON body extractor with JSON rejections
auth/                  Argon2 passwords · HS256 JWTs · single-use rotating refresh tokens · purpose-scoped email tokens · AuthUser extractor
catalog/               GAMES registry + refresh_all/seed_all dispatch per game · images.rs = lazy on-disk image cache
scryfall/              MTG provider: streaming bulk ingest, the search compiler (search/), daily price snapshots, Secret Lair drops, offline dummy seed
tcgcsv/                sealed-product provider: product catalog + product price history + opt-in historic price backfill
mtgjson/               card→sealed-product contents (AllPrintings.json)
collection_import/     Archidekt + Moxfield + CSV → one aggregate/resolve/reconcile engine; in-memory job queue
handlers/              auth · cache · catalog/ (incl. products.rs + pricing.rs) · collection/ · wishlist/ · shared/ (cross-cutting DTOs + the holdings core) · sitemap · mirror · health
entities/ migrator/    SeaORM entities · one migration per file + the migrations() registry
security_tests/        HTTP-level suites driving build_router in-process; Postgres/Redis tests env-gated behind --ignored
```

### Adding a backend feature

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
   `handlers/cache.rs`; consider a `security_tests/` suite.
4. **Wire types:** derive `ts_rs::TS` on response DTOs
   (`#[cfg_attr(test, derive(ts_rs::TS))]`) — regeneration is in the frontend recipe.

Adding a TCG = a `Game` in `catalog::GAMES` + a provider module + one arm each in
`catalog::refresh_all` and `catalog::seed_all`.

## Frontend map (`web/src/`)

Full annotations: `docs/architecture.md`.

```
main.ts / App.vue / router/   shell (MainNav: Products [Cards + Sealed] · Collection · Wish list) + routes; guard handles requiresAuth/requiresGuest + one-time session restore
lib/api/           typed fetch client, one module per API surface; generated/ = ts-rs wire types
lib/queries.ts     useAuthedQuery/useAuthedMutation — vue-query wrappers routed through auth.authFetch
lib/…              pure helpers (seo, mana, money, search, setGroups, buyLinks, turnstile, …)
stores/            Pinia: auth (in-memory access token, hand-tuned single-flight refresh), theme, cardSize
composables/       query hooks: useCatalog, useProducts, useCollection, useWishlist, useCollectionImport, …
components/        cards/ · products/ · collection/ · wishlist/ · legal/ · ui/ (shadcn-vue primitives)
views/             public: /cards…, /sealed…, auth + email flows, terms/privacy; signed-in: /collection…, /wishlist…, profile
test/              vitest fixtures (the Playwright e2e specs live one level up, at web/e2e/)
```

### Adding a frontend feature

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

## Don't break these

Rationale: `docs/tradeoffs.md` · full contracts: `docs/api-contracts.md`.

- Auth answers **generically** (register/resend/forgot reveal nothing; login is a
  generic 401 whose dummy-hash verify on unknown users is timing equalization, not
  dead code). Password rules are validated **before** an email token is consumed. A
  missing/bad CAPTCHA token is deliberately **400** (never 401/403).
- No `RESEND_API_KEY` = register returns the completion token in the response and
  login skips the verified gate — the dev/CI posture. **Never run prod without the
  key** (anyone could activate any address).
- Rate limiting **fails open** (Redis outage, unresolvable IP); CAPTCHA fails
  closed. Behind a proxy set `TRUST_PROXY_HEADERS=true`, and only behind one that
  overwrites `X-Forwarded-For` (else clients spoof their IP).
- Collection and wish list are **independent tables** that share the ts-rs DTOs in
  `handlers/shared/holdings.rs` — editing a shared shape changes both wire surfaces.
  Holdings use **external** card ids; both counts zero deletes the row.
- A replace-mode import matching **zero** catalog cards is refused (wipe guard);
  **smart sync never deletes** upstream-removed cards — only a full replace does.
  Moxfield **URL** import is deliberately disabled
  (`Provider::network_import_enabled()` is the switch; CSV upload is the supported
  path) — a 422 there is not a regression.
- Card images are cached lazily on first view — **never bulk-download** (Scryfall
  guideline); image fetches are host-allow-listed with redirects disabled.
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

Full reference: `docs/operations.md` (authoritative: `api/src/config.rs`,
`api/.env.example`). The behavior-changing ones: `JWT_SECRET` (required;
`ALLOW_INSECURE_DEV_SECRET=true` for local dev) · `RESEND_API_KEY` (unset = the
email dev bypass above) · `TURNSTILE_SECRET_KEY` pairs with the web's
`VITE_TURNSTILE_SITE_KEY` (both set or both unset) · `SEED_DUMMY_DATA=true` (offline
dummy catalog + the seeded e2e account; overrides syncing) · `TRUST_PROXY_HEADERS`
(see above) · `REDIS_URL` (cross-instance rate limiters).
