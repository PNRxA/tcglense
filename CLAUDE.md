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
| `web/` | Frontend (SPA) | Vue 3.5 · Vite 8 · Pinia · vue-router · Tailwind 4 · shadcn-vue (new-york) · TypeScript |

The two talk over HTTP. In dev the API runs on `:8080` and the web app on
`:5173`, and the **web dev server proxies `/api` to the API** (`web/vite.config.ts`)
so the browser is same-origin — the httpOnly refresh cookie is first-party. The
API's CORS layer also allows the `:5173` origin for any direct cross-origin calls.

## Running it

Two terminals.

**API** (from `api/`):

```sh
cp .env.example .env      # first run only; edit JWT_SECRET
cargo run                 # serves http://localhost:8080, runs DB migrations on boot
```

The SQLite file is created automatically (`tcglense.db`, gitignored). Migrations
run on every startup via `Migrator::up`. If `JWT_SECRET` is unset the server boots
with an **insecure dev-only secret and logs a warning**; with `COOKIE_SECURE=true`
(the production signal) it instead **refuses to start** without a real `JWT_SECRET`.

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

All responses (success or error) are JSON; errors are `{ "error": string }`.

Security rules baked in: emails trimmed + lowercased (case-insensitive accounts);
password ≥ 8 chars, email must contain `@`; login returns a **generic** 401 (with
timing equalization on user-not-found). Access JWTs are HS256 with `exp`, decoded
with the algorithm pinned to HS256. **Refresh tokens are single-use** — every
`/refresh` rotates them (claimed via an atomic conditional `UPDATE`) with **reuse
detection**: replaying a revoked token revokes that user's whole token family. The
cookie is `HttpOnly; SameSite=Lax; Path=/api/auth; Secure=COOKIE_SECURE`
(SameSite=Lax mitigates CSRF on `/refresh` and `/logout`).

## Backend structure (`api/src/`)

```
main.rs            bootstrap: env → tracing → DB connect → migrate → router → serve
config.rs          Config from env (DATABASE_URL, JWT_SECRET, ACCESS_TOKEN_EXPIRY_MINUTES, REFRESH_TOKEN_EXPIRY_DAYS, COOKIE_SECURE, PORT)
state.rs           AppState { db, config: Arc<Config> } (cloned into handlers)
error.rs           AppError enum + IntoResponse → JSON { error }, correct status codes
extract.rs         JsonBody<T>: JSON body extractor whose rejections are JSON, not text/plain
entities/          SeaORM entities (user = `users`, refresh_token = `refresh_tokens`)
migrator/          MigratorTrait impl + one migration per file (m<date>_<n>_<name>.rs)
auth/
  password.rs      Argon2 hash / verify (PHC strings, random salt)
  jwt.rs           access-token Claims + encode/decode (HS256, expiry in minutes)
  refresh.rs       opaque refresh-token service: issue / rotate (single-use) / revoke_one / revoke_all
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
main.ts            createApp + pinia + router
App.vue            shell: top bar (brand, user, sign-out) + <RouterView>
router/index.ts    routes + global guard (requiresAuth / requiresGuest, one-time session restore)
lib/api.ts         typed fetch client (relative URLs, credentials:'include') + ApiError + types
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

- **State that talks to the API:** add functions + types to `lib/api.ts`, then a
  Pinia setup store under `stores/`. For authenticated calls use the `auth` store's
  exported `authFetch((token) => api.xxx(token))` so access-token expiry refreshes
  transparently.
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
  (dev fallback + warning if unset; required when `COOKIE_SECURE=true`),
  `ACCESS_TOKEN_EXPIRY_MINUTES` (15), `REFRESH_TOKEN_EXPIRY_DAYS` (30),
  `COOKIE_SECURE` (false), `PORT` (8080), `RUST_LOG` (`info`). See `api/.env.example`.
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
- **Gotcha — `jsonwebtoken`:** v10 needs a crypto provider feature or it panics at
  runtime; this crate pins `default-features = false, features = ["rust_crypto"]`
  (pure Rust, no C toolchain). Don't drop that when bumping it.
