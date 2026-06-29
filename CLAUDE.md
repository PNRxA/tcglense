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
`:5173`; the API's CORS layer allows exactly that web origin.

## Running it

Two terminals.

**API** (from `api/`):

```sh
cp .env.example .env      # first run only; edit JWT_SECRET
cargo run                 # serves http://localhost:8080, runs DB migrations on boot
```

The SQLite file is created automatically (`tcglense.db`, gitignored). Migrations
run on every startup via `Migrator::up`. If `JWT_SECRET` is unset the server
boots with an **insecure dev-only secret and logs a warning** — set a real one
before deploying.

**Web** (from `web/`):

```sh
npm install               # first run only
npm run dev               # serves http://localhost:5173
```

Point the frontend at a non-default API with `VITE_API_URL` (default
`http://localhost:8080`).

## Commands

**API** (run from `api/`):

| Command | Purpose |
|---------|---------|
| `cargo run` | Run the server (migrations included) |
| `cargo check` | Fast type/borrow check |
| `cargo test` | Unit tests (password + JWT round-trips) |
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

| Method & path | Body | Success | Errors |
|---------------|------|---------|--------|
| `POST /api/auth/register` | `{ email, password, display_name? }` | `201 { token, user }` | `409` email taken · `422` validation |
| `POST /api/auth/login` | `{ email, password }` | `200 { token, user }` | `401 "invalid email or password"` (generic) |
| `GET /api/auth/me` | — (header `Authorization: Bearer <token>`) | `200 { user }` | `401` missing/invalid/expired token |
| `GET /api/health` | — | `200 { status: "ok" }` | — |

Rules baked into the backend: emails are trimmed + lowercased (case-insensitive
accounts); passwords must be ≥ 8 chars and contain `@` for the email; login
returns a **generic** 401 that never reveals which field was wrong (and equalizes
timing on the user-not-found path); JWTs are HS256 with `exp`, decoded with the
algorithm pinned to HS256 (no `alg:none`/confusion).

## Backend structure (`api/src/`)

```
main.rs            bootstrap: env → tracing → DB connect → migrate → router → serve
config.rs          Config from env (DATABASE_URL, JWT_SECRET, JWT_EXPIRY_DAYS, PORT)
state.rs           AppState { db, config: Arc<Config> } (cloned into handlers)
error.rs           AppError enum + IntoResponse → JSON { error }, correct status codes
extract.rs         JsonBody<T>: JSON body extractor whose rejections are JSON, not text/plain
entities/          SeaORM entities (entities/user.rs = `users` table)
migrator/          MigratorTrait impl + one migration per file (m<date>_<n>_<name>.rs)
auth/
  password.rs      Argon2 hash / verify (PHC strings, random salt)
  jwt.rs           Claims + encode/decode (HS256)
  extractor.rs     AuthUser: FromRequestParts that validates the Bearer token + loads the user
handlers/
  auth.rs          register / login / me
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
router/index.ts    routes + global guard (requiresAuth / requiresGuest, one-time token validation)
lib/api.ts         typed fetch client + ApiError + User/AuthResponse/... types
stores/auth.ts     Pinia store: token (localStorage), user, isAuthenticated, login/register/logout/fetchMe
views/             LoginView, RegisterView, DashboardView
components/ui/      shadcn-vue primitives (button, input, label, card)
assets/main.css    Tailwind 4 theme + CSS variables (light/dark)
```

The router guard validates a persisted token **once** before the first route
resolves (calls `fetchMe`, which logs out on a 401), so an expired token redirects
to `/login` instead of flashing the dashboard.

### Adding a frontend feature

- **State that talks to the API:** add functions + types to `lib/api.ts`, then a
  Pinia setup store under `stores/`. Read the token from the `auth` store and pass
  it to API calls.
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
  (dev fallback + warning if unset), `JWT_EXPIRY_DAYS` (7), `PORT` (8080),
  `RUST_LOG` (`info`). See `api/.env.example`.
- **Web:** `VITE_API_URL` (default `http://localhost:8080`).

## Known trade-offs / future work

- **Token storage:** the JWT lives in `localStorage`, which is readable by any
  injected script (XSS exfiltration risk). Acceptable for this scaffold; for
  production prefer an httpOnly Secure SameSite cookie set by the backend + a
  strict CSP.
- **No rate limiting / brute-force protection** on login yet.
- **Gotcha — `jsonwebtoken`:** v10 needs a crypto provider feature or it panics at
  runtime; this crate pins `default-features = false, features = ["rust_crypto"]`
  (pure Rust, no C toolchain). Don't drop that when bumping it.
