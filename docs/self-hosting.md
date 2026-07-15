# Self-hosting TCGLense

How to run your own TCGLense instance. Every topology needs a `JWT_SECRET` — the API
refuses to boot without one (generate with `openssl rand -hex 32`). The three Docker
images (`tcglense`, `tcglense-api`, `tcglense-web`) are published to **GHCR**
(`ghcr.io/pnrxa/…`) and **Docker Hub** (`docker.io/pnrxa/…`) on every release; the
build/publish/release mechanics and the full **environment-variable reference** live in
[`operations.md`](./operations.md).

Before accepting public accounts, work through the
[production signup launch checklist](./production-signup-checklist.md). The shipped
production manifests deliberately start with registration closed.

Pick a topology:

| Topology | When to use it | Guide |
|---|---|---|
| **Homelab** — one container + SQLite | simplest; a personal or LAN instance | [below](#homelab-one-container-and-sqlite) |
| **Managed cloud** — DO Droplet + Managed Postgres + Upstash + Cloudflare | recommended for a public site | [deploy-digitalocean.md](./deploy-digitalocean.md) |
| **Managed PaaS** — DO App Platform | low-ops; GitHub releases deploy themselves | [deploy-app-platform.md](./deploy-app-platform.md) |
| **Production split** — Caddy + web + api + Postgres + Redis | scaling to multiple API replicas | [below](#production-split-caddy-web-api-postgres-redis) |
| **Bare metal** — systemd + Caddy, no containers | no Docker | [below](#bare-metal-systemd-and-caddy) |

## Homelab: one container and SQLite

Needs Docker Engine + the Compose plugin. The **combined** image runs the API and SPA
together (the API serves the SPA via `WEB_ROOT`), backed by embedded SQLite. The
database and cached card images persist in one volume — nothing else to run.

```sh
export JWT_SECRET=$(openssl rand -hex 32)
docker compose -f deploy/docker-compose.homelab.yml up -d
# then open http://<host>:8080
```

Registration starts closed. On a trusted private LAN, you can explicitly set
`SIGNUPS_ENABLED=true` and `ALLOW_INSECURE_DEV_AUTH=true` to use the no-email setup
flow. That flow returns the completion credential to the browser instead of proving
mailbox ownership, so never expose it to the internet. For a public host, configure
HTTPS, Resend, and Turnstile as described in the launch checklist.

The compose file also reads `COOKIE_SECURE`, `PUBLIC_SITE_URL`, `MAINTENANCE_MODE`, and
`IMAGE_TAG` from the environment — `export` them before `up -d` (see the HTTPS note below and
`deploy/docker-compose.homelab.yml`).

<details>
<summary>…or a plain <code>docker run</code></summary>

```sh
docker run -d --name tcglense -p 8080:8080 \
  -e JWT_SECRET="$(openssl rand -hex 32)" \
  -v tcglense_data:/data \
  ghcr.io/pnrxa/tcglense:latest
```
</details>

On first boot the API imports MTG card data from Scryfall in the background (a
one-off ~30s; set `SYNC_ON_STARTUP=false` to skip, or `SEED_DUMMY_DATA=true` for an
offline catalog). For HTTPS, put a reverse proxy / tunnel in front and set
`COOKIE_SECURE=true` + `PUBLIC_SITE_URL=https://your-host`.

## Managed cloud (recommended for a public site)

Two step-by-step managed-service guides, both single-instance:

- **[DigitalOcean + Upstash + Cloudflare](./deploy-digitalocean.md)** (Droplet,
  recommended) — a DO Droplet runs the combined image; DigitalOcean Managed Postgres +
  Upstash Redis + a free Cloudflare CDN provide the rest. Full walkthrough covers DNS,
  TLS, backups, and rate-limit hardening.
- **[DigitalOcean App Platform](./deploy-app-platform.md)** (PaaS) — the low-ops
  variant: App Platform runs the combined image (no server to patch), Managed Postgres
  holds the data, Cloudflare is the CDN. GitHub releases **deploy themselves** once two
  DO secrets are set.

## Production split: Caddy, web, api, Postgres, Redis

The scalable split: an edge **Caddy** terminates TLS and sends static requests to the
**web** container and `/api` requests directly to the **api** container. The API is
backed by **Postgres** and **Redis** (shared rate-limiter state across instances).
Config lives in [`deploy/docker-compose.prod.yml`](../deploy/docker-compose.prod.yml)
+ [`deploy/edge.Caddyfile`](../deploy/edge.Caddyfile).

```
internet ──443──▶ caddy (TLS) ─┬─▶ web (SPA)
                               └─▶ api ─┬─▶ db    (Postgres)
                                        └─▶ cache (Redis)
```

```sh
export SITE_ADDRESS=tcglense.example.com          # your domain — Caddy auto-provisions HTTPS
export JWT_SECRET=$(openssl rand -hex 32)
export POSTGRES_PASSWORD=$(openssl rand -hex 24)
# Also set RESEND_API_KEY, EMAIL_FROM, and both TURNSTILE keys.
# Keep SIGNUPS_ENABLED=false until the linked launch checklist passes.
docker compose -f deploy/docker-compose.prod.yml up -d
```

Point DNS for `SITE_ADDRESS` at the host with ports 80+443 reachable and Caddy
obtains a Let's Encrypt certificate on first boot; the API runs migrations on start.
Pin a release across the whole stack with `IMAGE_TAG=vX.Y.Z` (defaults to `latest`).

### Behind a CDN (Cloudflare)

The public catalog (`/api/games/*`), its image/icon proxy, the XML sitemaps
(`/api/sitemap.xml`, `/api/sitemaps/*`), the public OpenAPI document
(`/api/openapi.json`), and — on a mirror host (`MIRROR_ENABLED=true`) — the dataset
mirror (`/api/mirror/*`) already emit CDN-friendly `Cache-Control` (`s-maxage` /
`immutable`), so a CDN caches them with no code change. To put Cloudflare in front:

- Set **`CDN_MODE=true`** on the `api` service — the origin then skips caching card
  images to disk (the CDN absorbs the repeats), so it needs no writable image dir.
  Leave it off if no CDN sits in front, or every view re-fetches upstream.
- **Create the three Cache Rules below** — Cloudflare doesn't cache `/api/*` paths by
  default, so you have to tell it which responses are cacheable. The origin already
  sends the right `Cache-Control` on every response, so the rules only make Cloudflare
  **honor the origin header** — there are no per-route TTLs to keep in sync.
- **TLS:** set Cloudflare SSL/TLS to **Full (strict)** and give Caddy a valid origin
  cert — a Cloudflare Origin CA cert, or Let's Encrypt via the DNS-01 challenge (the
  HTTP-01 challenge can be interfered with by Cloudflare's proxy).

Add all three under *Caching → Cache Rules* (included on the free plan). Because each is set
to honor the origin, Cloudflare gives every response the exact lifetime the API already
chose — catalog reads their hour at the edge (`s-maxage=3600`), the image/icon proxy its
30 days (`immutable`), the sitemaps their day, the dataset mirror its per-route window
(the bulk-data catalog an hour, the streamed bulk file a deliberately shorter half-hour,
MTGJSON's `AllPrintings` a cheap conditional revalidate, and TCGCSV's dated price archives
a year of `immutable` — they never change once published) — and caches **no** `no-store` response
(auth, the live `status` route, per-user collection/wishlist data, and every error), so
you never have to enumerate those to keep them out.

1. **Cache the public catalog, images, sitemaps, API docs, and the dataset mirror.** *Cache eligibility* →
   **Eligible for cache**; *Edge TTL* → **Use cache-control header if present, bypass
   cache if not** (the honor-the-origin option — pick the *bypass*-if-absent one, not
   *…use default Cloudflare caching…*); *Browser TTL* → **Respect origin**. When
   incoming requests match:

   ```
   (starts_with(http.request.uri.path, "/api/games") and not ends_with(http.request.uri.path, "/status"))
   or http.request.uri.path eq "/api/sitemap.xml"
   or starts_with(http.request.uri.path, "/api/sitemaps/")
   or http.request.uri.path eq "/api/openapi.json"
   or starts_with(http.request.uri.path, "/api/mirror/")
   ```

   *Note: `/api/mirror/*` (the dataset mirror) is served only on a mirror host
   (`MIRROR_ENABLED=true`, like the public site) and is a harmless no-op otherwise. Keep
   it under this honor-origin rule — **never a fixed Edge TTL** — so Cloudflare respects
   each mirror route's own `s-maxage`; [`tradeoffs.md`](./tradeoffs.md) (dataset-mirror trade-off)
   explains why the bulk-file TTL is deliberately shorter than the catalog's, plus the
   CDN object-size caveat for the large decompressed Scryfall bulk file.*

2. **Bypass everything per-user or live.** *Cache eligibility* → **Bypass cache**. This
   is belt-and-braces — these routes are already `no-store` at the origin, so rule 1
   would never cache them — but an explicit bypass documents the intent. When incoming
   requests match:

   ```
   starts_with(http.request.uri.path, "/api/auth/")
   or starts_with(http.request.uri.path, "/api/collection/")
   or starts_with(http.request.uri.path, "/api/wishlist/")
   or (starts_with(http.request.uri.path, "/api/games") and ends_with(http.request.uri.path, "/status"))
   ```

3. **Cache the SPA HTML pages and assets.** *Cache eligibility* → **Eligible for cache**;
   *Edge TTL* → **Use cache-control header if present, bypass cache if not**; *Browser TTL* →
   **Respect origin**. When incoming requests match:

   ```
   not starts_with(http.request.uri.path, "/api/")
   ```

   *Note: The origin automatically serves HTML pages with `public, no-cache` (cached but revalidated on every request via ETags so builds stay fresh on deploy) and hashed static assets with `public, max-age=31536000, immutable`.*

The expressions are structured so you can apply them in order. If you ever add *overlapping* cache rules, note that Cloudflare applies **all** that match and the **last** one wins per setting.

#### Verify caching is actually live

A cache that isn't working fails **silently** — the site behaves correctly, just uncached,
and the origin quietly serves everything. A correct rule set is also **not sufficient**: an
unrelated zone setting can un-cache a surface without touching your rules (see the Bot
Fight Mode box below), and it does so *invisibly to a browser*. So measure each surface
with `cf-cache-status` rather than assuming, and re-check after editing rules:

```sh
# curl sends no cookies — which is the point: that is exactly what a mirror consumer,
# a crawler, or an uptime monitor looks like. Run it twice; the first hit may be a cold MISS.
for p in /api/games /api/openapi.json /api/sitemap.xml /api/mirror/tcgcsv/last-updated.txt /api/health; do
  printf '%-44s ' "$p"
  curl -sI "https://YOUR-DOMAIN$p" \
    | grep -iE '^(HTTP/|cf-cache-status|set-cookie: __cf_bm)' \
    | sed -E 's/^(set-cookie): (__cf_bm).*/\1: \2 (!)/I' | tr -d '\r' | tr '\n' ' '
  echo
done
```

| Path | Expected | Meaning |
|------|----------|---------|
| `/api/games` | `HIT` / `MISS` / `EXPIRED` | rule 1 live (`MISS` just means cold — re-run it) |
| `/api/openapi.json` | `HIT` / `REVALIDATED` | rule 1 live |
| `/api/sitemap.xml` | `HIT` / `MISS` | rule 1's sitemap clause live |
| `/api/mirror/*` | `HIT` / `MISS` on a **mirror host**; `BYPASS` otherwise | rule 1's mirror clause live. `MIRROR_ENABLED` is off by default, and then the route is a `404` — so `BYPASS` here says nothing about your rules unless you run a mirror |
| `/api/health` | `DYNAMIC` | correct — no rule matches it, and none should |

Read a failure precisely — **check the status code first** (`curl -sI` shows it), because
the same `cf-cache-status` has very different causes:

- **`DYNAMIC`** on a path that should cache = **no rule matches it**. Rule 1 is missing
  that clause, or the rule is disabled. `DYNAMIC` on `/api/health` is the expected
  baseline — it proves there is no blanket `/api/*` rule.
- **`BYPASS`** on a **non-`200`** is expected and means nothing about your rules. The
  origin marks *every* error `no-store` by design (so a CDN never pins a `404`), and
  rule 1's *bypass cache if not present* Edge TTL also yields `BYPASS` for any response
  carrying no `Cache-Control` at all — which is what an unrouted path returns.
- **`BYPASS`** on a genuine **`200`** — look for a **`set-cookie`** on the same response
  before you touch any rule:
  - **With `set-cookie: __cf_bm`** → this is **Bot Fight Mode**, not your rules. See the
    box below; your Cache Rules are fine and editing them will not help.
  - **Without a `set-cookie`** → *then* it's a Cache Rule set to *Bypass cache* whose
    expression is broader than intended, matching and **ordered after** rule 1 (the last
    matching rule wins per setting). **Narrow that rule's expression**; do *not* reorder
    it before rule 1, or rule 1 would start winning on `/api/auth/*`,
    `/api/collection/*`, `/api/wishlist/*` and the live `status` route, undoing rule 2.

> **Bot Fight Mode silently un-caches the mirror.** *Security → Bots → Bot Fight Mode*
> attaches a `__cf_bm` **`Set-Cookie`** to any response filled from the origin, and
> Cloudflare does not cache a response that sets a cookie — it returns `BYPASS`. (Free/Pro/
> Business have *Origin Cache Control* permanently enabled, so this branch can't be turned
> off.) Cloudflare doesn't state this in one place; it follows from its
> [cookies](https://developers.cloudflare.com/fundamentals/reference/policies-compliances/cloudflare-cookies/)
> and [cache-behavior](https://developers.cloudflare.com/cache/concepts/cache-behavior/) docs.
>
> A client that **already holds** `__cf_bm` is not re-issued one, so its responses cache
> normally — which is why **browser traffic looks fine** and only cookieless clients bypass.
> That is the trap: the dataset mirror's entire audience is cookieless. TCGLense's own HTTP
> client builds `reqwest` **without** the `cookies` feature (`api/Cargo.toml`), so every
> consumer instance is permanently cookieless, and **every** mirror pull bypasses the edge —
> forever, and invisibly if you only ever check the site in a browser. Search-engine
> crawlers hitting `/api/sitemap*` are cookieless too.
>
> On **Free** it cannot be scoped: *"You cannot bypass or skip Bot Fight Mode using WAF
> custom rules or Page Rules"* — it doesn't run on the Ruleset Engine. Options, in order:
> turn Bot Fight Mode **off**; or, on **Pro+**, use Super Bot Fight Mode with a WAF custom
> rule using the **Skip** action for the public cacheable paths; or give rule 1 an explicit
> *Edge TTL* (Cloudflare then strips the `Set-Cookie` and caches) — but that discards the
> origin's `Cache-Control` for edge TTL, which **breaks the mirror's per-route windows**
> (see rule 1's note and `tradeoffs.md`), so it needs one rule per mirror sub-surface with
> TTLs hand-synced to the code. Prefer the first two.

A `BYPASS` on `/api/mirror/*` is worth catching early: it makes every self-host's dataset
pull — and especially the ~900-day archive walk of the one-time price backfill
(`PRICE_BACKFILL_ENABLED`, see [`operations.md`](./operations.md)) — miss the edge and hit
your origin, which then re-fetches each file from the upstream under *your* User-Agent and
IP. Cached, the edge absorbs those repeats and the upstream is hit about once per file.

> **Real client IP (important).** With Cloudflare in front, the edge Caddy's immediate
> peer is *Cloudflare*, so the default `edge.Caddyfile` line
> `header_up X-Forwarded-For {http.request.remote.host}` makes **every** request key to
> a Cloudflare IP — collapsing per-IP auth rate limiting. Switch it to Cloudflare's
> real-client header **and restrict origin ingress to Cloudflare's IP ranges** (a
> firewall allowlist or a Cloudflare Tunnel) so it can't be spoofed by hitting the
> origin directly:
>
> ```caddyfile
> @backend path /api/* /sitemap.xml /sitemaps/*
> handle @backend {
> 	reverse_proxy api:8080 {
> 		header_up X-Forwarded-For {http.request.header.CF-Connecting-IP}
> 	}
> }
> ```

## Bare metal: systemd and Caddy

Run the binary + static SPA directly, no containers.

1. **Build both:**
   ```sh
   cd web && npm ci && npm run build       # -> web/dist/
   cd ../api && cargo build --release       # -> api/target/release/tcglense-api
   ```
2. **Place the artifacts** on the server, e.g.:
   ```sh
   install -D api/target/release/tcglense-api /srv/tcglense/tcglense-api
   mkdir -p /srv/tcglense/web && cp -r web/dist/. /srv/tcglense/web/
   ```
3. **Configure** `/srv/tcglense/api.env`. Use `api/.env.example` as a reference,
   but do not copy its local-development auth opt-ins into production. Start with
   registration closed and set at least:
   ```sh
   JWT_SECRET=...                       # openssl rand -hex 32 — required
   ALLOW_INSECURE_DEV_SECRET=false
   ALLOW_INSECURE_DEV_AUTH=false
   COOKIE_SECURE=true                   # HTTPS-only refresh cookie
   PUBLIC_SITE_URL=https://cards.example.com
   HOST=127.0.0.1                       # API listens on localhost; only Caddy reaches it
   MAINTENANCE_MODE=false               # planned-upgrade switch; restart after changing
   SIGNUPS_ENABLED=false                # keep closed through the first production deploy
   DATABASE_URL=sqlite:///var/lib/tcglense/tcglense.db?mode=rwc   # dir must exist + be writable
   DATA_DIR=/var/lib/tcglense/data       # persistent + writable; holds cached card images
   ```
   Before changing `SIGNUPS_ENABLED=true`, add `RESEND_API_KEY`, an `EMAIL_FROM`
   address on your verified domain, and both `TURNSTILE_SECRET_KEY` and
   `TURNSTILE_SITE_KEY`. The API refuses to boot with public signups if any of
   those protections is missing. Follow the
   [production signup checklist](./production-signup-checklist.md) for the rollout.
4. **Run the API as a service** (Linux/systemd): copy
   [`deploy/tcglense-api.service`](../deploy/tcglense-api.service) to
   `/etc/systemd/system/`, adjust paths/user, then
   ```sh
   sudo systemctl daemon-reload && sudo systemctl enable --now tcglense-api
   ```
5. **Run Caddy:** set your domain and site root in
   [`deploy/Caddyfile`](../deploy/Caddyfile), then
   `caddy run --config deploy/Caddyfile` (HTTPS is automatic for a real domain).

To ship an update: rebuild, copy the new `tcglense-api` + `web/dist`, then
`sudo systemctl restart tcglense-api`.

## Docker images, releases, and the env-var reference

The three images, how they're built and published, how to cut a release, and the
complete annotated environment-variable reference are all in
[`operations.md`](./operations.md) — see *Docker images & releases* and *Environment
variables*. The `deploy/` directory inventory (every compose file and Caddyfile) is in
the same doc.
