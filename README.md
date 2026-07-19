# TCGLense

A self-hostable trading-card-game tracker: browse a card catalog, track singles and
sealed-product prices over time, and manage your personal collection, wish list, and
decks. Ships a public JSON API with scoped API keys (interactive docs at `/docs`).

**Stack:** a Rust API (axum · SeaORM · SQLite or Postgres) and a Vue 3 SPA (Vite ·
Pinia · Tailwind). MTG first, more games to follow.

## Self-host it

**Prerequisites:** Docker Engine + the Compose plugin. The **combined** image runs the
API and SPA in one container, backed by embedded SQLite. One volume holds the database
and cached card images — nothing else to run:

```sh
export JWT_SECRET=$(openssl rand -hex 32)
docker compose -f deploy/docker-compose.homelab.yml up -d
# then open http://<host>:8080
```

On first boot it imports the MTG catalog from Scryfall in the background (a one-off
~30s). New-account signup starts closed. On a trusted local network with no email
provider, explicitly export `SIGNUPS_ENABLED=true` and
`ALLOW_INSECURE_DEV_AUTH=true` before bringing it up; that bypass is rejected for an
internet-facing origin. For a public site, configure HTTPS (`COOKIE_SECURE=true` and a
real `PUBLIC_SITE_URL`), Resend with a verified-domain sender, and both Turnstile keys.
Images are published to GHCR and Docker Hub on every release.

**Other ways to deploy** — managed cloud (recommended for a public site), the scalable
Caddy + Postgres + Redis split, bare metal, and putting a CDN in front — are in the
**[self-hosting guide](./docs/self-hosting.md)**.

## Develop it

**Prerequisites:** [Rust](https://rustup.rs/) (cargo) and [Node](https://nodejs.org/)
22.18+ / 24.12+. SQLite is embedded — nothing to install. Run both servers (API on
`:8080`, web on `:5173`) with one command:

```sh
./scripts/dev.sh
```

It auto-creates `api/.env` (from `api/.env.example`) and installs web deps on first run,
then serves the app at http://localhost:5173. The contributor guide — conventions, the
pre-commit checks, and how to add a feature — is [`CLAUDE.md`](./CLAUDE.md).

## Command-line client

`tcglense` is a Rust CLI **and** interactive TUI over the same public API — browse the
catalog, manage your collection, wish list and decks, and mint API keys, from the
terminal. It authenticates with either a web session (email + password) or a `tcgl_` API
key. Prebuilt binaries for Linux, macOS, and Windows are attached to each
[GitHub release](https://github.com/PNRxA/tcglense/releases).

```sh
cd cli && cargo install --path .   # or download a release binary
tcglense games                     # one-shot command
tcglense                           # launch the interactive TUI
```

Full usage, auth, and the command reference: **[`cli/README.md`](./cli/README.md)**.

## Documentation

| Doc | What's in it |
|---|---|
| [`docs/self-hosting.md`](./docs/self-hosting.md) | Every deploy topology: homelab, managed cloud, production split, bare metal, CDN |
| [`docs/operations.md`](./docs/operations.md) | Running, commands, CI, releases, Docker images, and the full environment-variable reference |
| [`docs/api-contracts.md`](./docs/api-contracts.md) | Every HTTP endpoint, wire shapes, the search syntax, caching/ETags/sitemaps, import/sync |
| [`cli/README.md`](./cli/README.md) | The `tcglense` command-line client + TUI: install, auth, commands, and keybindings |
| [`docs/architecture.md`](./docs/architecture.md) | Annotated file map of `api/src` and `web/src`, plus test organization |
| [`docs/tradeoffs.md`](./docs/tradeoffs.md) | Design rationale and known trade-offs — read before "fixing" something that looks odd |
| [`docs/production-signup-checklist.md`](./docs/production-signup-checklist.md) | Fail-closed production signup rollout, backup, readiness, email, CAPTCHA, and canary checks |
| [`CLAUDE.md`](./CLAUDE.md) | Contributor guide: project conventions and step-by-step guides for adding features |

Managed-cloud walkthroughs: [DigitalOcean Droplet](./docs/deploy-digitalocean.md)
(recommended) · [DigitalOcean App Platform](./docs/deploy-app-platform.md) (PaaS).
