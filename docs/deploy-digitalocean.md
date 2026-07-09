# Deploy: DigitalOcean + Upstash + Cloudflare

A recommended production deployment of TCGLense on managed services, for a single
API instance at low/moderate traffic. One DigitalOcean Droplet runs the app; the
database, cache, and CDN are managed.

- **Compute** — a DigitalOcean Droplet running the **combined** Docker image (the API
  serves both `/api` and the SPA, so everything is same-origin and the httpOnly
  refresh cookie stays first-party).
- **Database** — DigitalOcean **Managed Postgres** (daily backups + point-in-time
  recovery, on the droplet's private VPC network).
- **Cache / rate-limit store** — **Upstash Redis** (`rediss://`, TLS).
- **CDN / web / DNS / TLS** — **Cloudflare** (free plan: global CDN, TLS, WAF).

Rough cost: **~$21–36/mo** — Droplet $6–12 + Managed Postgres ~$15 + Upstash (free
tier for a rate-limiter's traffic) + Cloudflare free.

```
internet ─▶ Cloudflare (DNS · CDN · TLS · WAF)
         ─▶ [DO cloud firewall: inbound 443/80 from Cloudflare IPs only]
         ─▶ Droplet: Caddy (origin TLS · rewrites XFF ⟵ CF-Connecting-IP)
                  ─▶ api (combined image: SPA + /api)
                       ├─▶ DigitalOcean Managed Postgres  (private VPC, ?sslmode=require)
                       └─▶ Upstash Redis                  (rediss://…, TLS)
```

Two things make this stack drop-in on the published image, with **no rebuild**:

- The **Turnstile site key is set at runtime** (`TURNSTILE_SITE_KEY` on the API,
  served to the SPA via `GET /api/config`) — you don't bake a CAPTCHA key into a
  custom web build.
- **`rediss://` (TLS) Redis is supported**, so a hosted provider like Upstash works
  directly (no private-network Redis required).

Reference files live in [`deploy/`](../deploy):
[`docker-compose.do.yml`](../deploy/docker-compose.do.yml),
[`do.Caddyfile`](../deploy/do.Caddyfile), and
[`do.env.example`](../deploy/do.env.example).

---

## 1. DigitalOcean Managed Postgres

1. **Create → Databases → PostgreSQL** (latest major), the cheapest single-node tier
   (1 GB RAM / 10 GB disk holds the ~2 GB catalog with room to grow). Place it in the
   region you'll run the droplet in and attach it to a **VPC**.
2. In **Connection details**, switch to **VPC network** and copy the *private* host.
   Create a database named `tcglense` (or use the default `defaultdb`).
3. After the droplet exists (step 3), add it to the database's **Trusted sources** so
   only it can connect.
4. Your `DATABASE_URL` uses the **private** host, port `25060`, and `?sslmode=require`
   (DO requires TLS; the API supports it):
   ```
   postgres://doadmin:<password>@<private-db-host>:25060/tcglense?sslmode=require
   ```

Managed Postgres gives you **automatic daily backups + PITR** — that's the durability
you're paying for, and why the droplet itself stays stateless.

## 2. Upstash Redis

Redis backs the cross-instance rate limiters. At a **single** API instance it's
optional (the limiter runs in-memory and fails open); add it when you want shared
state or plan to scale to more than one replica.

1. Create a Redis database at [upstash.com](https://upstash.com) in (or near) your
   droplet's region.
2. Copy the **`rediss://…` connection string** (the TLS endpoint) into `REDIS_URL`.
   The API speaks the Redis protocol over TLS directly — Upstash's REST API is not
   used. A rate limiter's command volume is tiny, so the free tier is usually enough.

> The URL must be `rediss://` (TLS). A plain `redis://` to Upstash won't connect.

## 3. DigitalOcean Droplet

1. **Create → Droplet** in the **same region + VPC** as the database. Ubuntu LTS,
   **2 GB / $12** is comfortable during the first catalog sync (1 GB / $6 works but is
   tight while the initial import runs).
2. Install Docker + the compose plugin.
3. Put the deploy files on the box by cloning the repo — the commands below are run
   from the repo root, where the `deploy/` directory lives. (If you'd rather copy just
   the `deploy/` files somewhere flat, drop the `deploy/` prefix from the commands in
   step 6 and run them from that directory.)

## 4. Cloudflare

1. **DNS** — add an `A` record for your apex (and `www`) pointing at the droplet's
   public IP, **proxied** (orange cloud).
2. **SSL/TLS → Overview** — set the mode to **Full (strict)** and enable **Always Use
   HTTPS**.
3. **SSL/TLS → Origin Server → Create Certificate** — save the certificate and key as
   `origin/cert.pem` and `origin/key.pem` next to the compose file (Caddy serves them
   as the origin cert).
4. **Caching → Cache Rules** (free plan) — add these three so Cloudflare honors the
   `Cache-Control` the API already emits (the origin sends the right header per route;
   these just tell Cloudflare which paths are cacheable):

   **Rule 1 — cache the catalog, images, and sitemaps.** *Eligibility* → Eligible for
   cache; *Edge TTL* → "Use cache-control header if present, bypass if not"; *Browser
   TTL* → Respect origin. Expression (the `/api/sitemap*` lines cover the legacy
   aliases; the canonical root `/sitemap.xml` + `/sitemaps/*` paths — issue #294 —
   fall under Rule 3, which already respects the origin's sitemap `Cache-Control`):
   ```
   (starts_with(http.request.uri.path, "/api/games") and not ends_with(http.request.uri.path, "/status"))
   or http.request.uri.path eq "/api/sitemap.xml"
   or starts_with(http.request.uri.path, "/api/sitemaps/")
   ```

   **Rule 2 — bypass everything per-user or live.** *Eligibility* → Bypass cache.
   Expression:
   ```
   starts_with(http.request.uri.path, "/api/auth/")
   or starts_with(http.request.uri.path, "/api/collection/")
   or starts_with(http.request.uri.path, "/api/wishlist/")
   or (starts_with(http.request.uri.path, "/api/games") and ends_with(http.request.uri.path, "/status"))
   ```

   **Rule 3 — cache the SPA HTML pages and assets.** *Eligibility* → Eligible for
   cache; *Edge TTL* → "Use cache-control header if present, bypass if not"; *Browser
   TTL* → Respect origin. Expression:
   ```
   not starts_with(http.request.uri.path, "/api/")
   ```

   `GET /api/config` is `no-store` at the origin, so Cloudflare never caches it — the
   SPA always reads a fresh Turnstile site key.

## 5. DigitalOcean cloud firewall

The origin trusts `CF-Connecting-IP` for rate-limiting (see the Caddyfile), so the
origin must only be reachable *through* Cloudflare — otherwise a client could hit it
directly and forge that header.

Create a firewall on the droplet:

- **Inbound** TCP `443` and `80` from **Cloudflare's IP ranges only**
  (<https://www.cloudflare.com/ips>), plus TCP `22` from your admin IP. Deny the rest.

> Prefer no open ports at all? Run a **Cloudflare Tunnel** (`cloudflared`) sidecar
> instead of opening 443 — ingress is then Cloudflare-only by construction, and you
> can drop the Origin cert. You still keep Caddy for the `X-Forwarded-For` rewrite.

## 6. Configure and launch

From your deploy directory on the droplet:

```sh
cp deploy/do.env.example .env          # then fill it in (see below)
mkdir -p deploy/origin                 # place cert.pem + key.pem here (step 4.3)
docker compose -f deploy/docker-compose.do.yml --env-file .env up -d
docker compose -f deploy/docker-compose.do.yml logs -f api
```

Fill in `.env` (full annotated template in
[`deploy/do.env.example`](../deploy/do.env.example)):

| Variable | Value |
|---|---|
| `SITE_ADDRESS` | your domain, e.g. `tcglense.example.com` |
| `JWT_SECRET` | `openssl rand -hex 32` |
| `DATABASE_URL` | the DO Managed Postgres **private** URL with `?sslmode=require` |
| `REDIS_URL` | the Upstash `rediss://…` URL (or leave empty for the in-memory limiter) |
| `TURNSTILE_SECRET_KEY` / `TURNSTILE_SITE_KEY` | both, or neither (see below) |
| `RESEND_API_KEY` / `EMAIL_FROM` | required in production (see below) |

## 7. Two things not to miss

- **`RESEND_API_KEY` is mandatory in production.** With it unset, registration returns
  the completion token in the response and login skips the verified-email gate — so
  **anyone could activate any address**. Set up [Resend](https://resend.com) with a
  verified sending domain and point `EMAIL_FROM` at it (the default
  `onboarding@resend.dev` only delivers to the Resend account owner).
- **Turnstile keys are all-or-nothing.** Set **both** `TURNSTILE_SECRET_KEY` and
  `TURNSTILE_SITE_KEY` or **neither** — a mismatched pair makes the API refuse to
  boot. Both live on the API now; the public site key is served to the SPA at runtime
  (`GET /api/config`), so you never rebuild the image to change it.

## 8. Verify

```sh
curl -s https://<your-domain>/api/health         # -> {"status":"ok"}
curl -s https://<your-domain>/api/config          # -> {"turnstile_site_key": "..."|null}
docker compose -f deploy/docker-compose.do.yml logs api | grep -iE "backend|migrat|redis"
```

On first boot the API runs migrations against Postgres, then pulls the card catalog
from the TCGLense mirror in the background (Postgres fills to ~2 GB over a few
minutes — that's why the DB tier needs ≥10 GB). Open the site; the catalog populates
as the sync progresses.

## Scaling past one instance

This stack is single-instance by design. To run **multiple API replicas** (behind a
DO Load Balancer):

- Postgres is already shared — good.
- **Set `REDIS_URL`** (Upstash) on every replica so the per-IP rate limiters share
  state instead of diverging per process.
- Keep `CDN_MODE=true` so replicas don't rely on a local image cache.

Everything else is unchanged.
