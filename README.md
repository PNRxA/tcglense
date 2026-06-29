# TCGLense

A trading-card-game tracking app — track retail/MSRP and singles prices over time,
your personal collection, and set-completion progress.

> Status: early scaffold. Auth (register / login / refresh / logout) is built;
> price-tracking, collection, and set-completion features are next.

## Stack

- **`api/`** — Rust API: [axum](https://github.com/tokio-rs/axum) · [SeaORM](https://www.sea-ql.org/SeaORM/) over SQLite · JWT access tokens + httpOnly refresh cookies · Argon2
- **`web/`** — Vue 3 SPA: Vite · Pinia · Vue Router · Tailwind 4 · shadcn-vue · TypeScript

## Getting started

**Prerequisites:** [Rust](https://rustup.rs/) (cargo) and [Node](https://nodejs.org/) 22.18+ / 24.12+. SQLite is embedded — nothing to install.

Run both servers (API on `:8080`, web on `:5173`) with one command:

```sh
./scripts/dev.sh
```

It auto-creates `api/.env` (from `api/.env.example`) and installs web deps on first run.
Then open http://localhost:5173.

<details>
<summary>Run them manually instead</summary>

```sh
# terminal 1 — API
cd api && cp .env.example .env && cargo run

# terminal 2 — web
cd web && npm install && npm run dev
```
</details>

## Layout

```
api/        Rust HTTP JSON API (auth, database, migrations)
web/        Vue single-page app
scripts/    dev helpers (dev.sh runs both servers)
CLAUDE.md   architecture, conventions, and how to add features
```

## Common commands

| | API (`cd api`) | Web (`cd web`) |
|---|---|---|
| Run (dev) | `cargo run` | `npm run dev` |
| Test | `cargo test` | `npm run test:unit` |
| Lint | `cargo clippy` | `npm run lint` |
| Build | `cargo build --release` | `npm run build` |

## Docs

See [`CLAUDE.md`](./CLAUDE.md) for the architecture, the auth API contract,
project conventions, and step-by-step guides for adding new features.
