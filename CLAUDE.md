# CLAUDE.md

Guidance for working in this repository.

## What this is

**TCGLense** is a trading-card-game tracking application. The goal is to track:

- Retail and MSRP prices over time (sealed product)
- Singles prices over time
- A user's personal collection
- Set-completion progress (how much of a set you own)

Only the **auth foundation** (register / login / session) is built so far. The
price-history, collection, and set-completion features are not yet implemented —
they are the next things to build on top of this scaffold.

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

The SQLite file is created automatically (`tcglense.db`, gitignored). Migrations
run on every startup via `Migrator::up`. The server **refuses to start without a
real `JWT_SECRET`** (≥ 32 bytes, and not the public dev constant). For local dev
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

## Backend structure (`api/src/`)

```
main.rs            bootstrap: env → tracing → DB connect → migrate → router → serve
config.rs          Config from env (DATABASE_URL, JWT_SECRET, ALLOW_INSECURE_DEV_SECRET, ACCESS_TOKEN_EXPIRY_MINUTES, REFRESH_TOKEN_EXPIRY_DAYS, COOKIE_SECURE, HOST, PORT); Debug redacts the secret
state.rs           AppState { db, config: Arc<Config>, dummy_password_hash } (cloned into handlers)
error.rs           AppError enum + IntoResponse → JSON { error }, correct status codes
extract.rs         JsonBody<T>: JSON body extractor whose rejections are JSON, not text/plain
entities/          SeaORM entities (user = `users`, refresh_token = `refresh_tokens`)
migrator/          MigratorTrait impl + one migration per file (m<date>_<n>_<name>.rs)
auth/
  password.rs      Argon2 hash / verify (PHC strings, random salt)
  jwt.rs           access-token Claims + encode/decode (HS256, expiry in minutes)
  refresh.rs       opaque refresh-token service: issue / rotate (single-use, successor-linked reuse detection) / revoke_one / revoke_all / prune_expired
  cookie.rs        build + clear the tcglense_refresh httpOnly cookie
  extractor.rs     AuthUser: FromRequestParts that validates the Bearer access token + loads the user
handlers/
  auth.rs          register / login / refresh / logout / me
  health.rs        health
```

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
App.vue            shell: top bar (brand, user, sign-out) + <RouterView>
router/index.ts    routes + global guard (requiresAuth / requiresGuest, one-time session restore)
lib/api.ts         typed fetch client (relative URLs, credentials:'include') + ApiError + types
lib/queryClient.ts createQueryClient (defaults: staleTime 5m, retry skips 4xx) + shouldRetryQuery
lib/queries.ts     useAuthedQuery / useAuthedMutation: vue-query wrappers that run through auth.authFetch
stores/auth.ts     Pinia store: in-memory accessToken + user, isAuthenticated, login/register/logout/refresh/fetchMe/tryRestore + authFetch helper
views/             LoginView, RegisterView, DashboardView
components/ui/      shadcn-vue primitives (button, input, label, card)
assets/main.css    Tailwind 4 theme + CSS variables (light/dark)
```

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
  `RUST_LOG` (`info`). See `api/.env.example`.
- **Web:** `VITE_API_URL` (default empty → relative `/api`, via the dev proxy).

## Known trade-offs / future work

- **Token storage:** the refresh token is an `HttpOnly` cookie (not readable by JS)
  and the access token is held in memory only, so an XSS can't exfiltrate the
  long-lived credential. In production set `COOKIE_SECURE=true` (HTTPS) and serve
  web + API same-origin (or configure cross-origin CORS credentials).
- **No rate limiting / brute-force protection** on login yet.
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
