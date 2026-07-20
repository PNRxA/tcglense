# Deploy: DigitalOcean App Platform (PaaS)

A managed-platform deployment of TCGLense: DigitalOcean **App Platform** runs the app
container (no server to patch), **Managed Postgres** holds the data, and **Cloudflare**
provides the CDN. This is the low-ops alternative to the Droplet stack in
[`deploy-digitalocean.md`](./deploy-digitalocean.md) — pick this one when you want
GitHub releases to deploy themselves and you don't need to run a box.

This deployment is **single-instance and Redis-free** by design.
Before accepting accounts, complete the
[production signup launch checklist](./production-signup-checklist.md). The app spec
starts with `SIGNUPS_ENABLED=false` so its first rollout stays closed.

The spec is applied by `doctl apps create/update`; the release workflow only asks an
existing app to pull the latest image and does **not** push later spec changes. For an
already-running app, confirm the live component has `SIGNUPS_ENABLED=false` and
`MAINTENANCE_MODE=false`, and change its health-check path to `/api/health` in App
Platform before this rollout (or apply the reviewed spec with `doctl apps update`,
preserving its secrets and domain). The API binds the listener as soon as the database
connection is open and runs the schema migrations in the background, so `/api/health`
answers the platform health check within its window even when a large migration takes
minutes; a startup gate presents the site as under maintenance until the migrations
finish — `/api/config` reports `maintenance_mode: true`, `/api/ready` drains with
`{"status":"maintenance"}`, and application traffic gets the maintenance `503` — so the
SPA shows its maintenance screen and nothing queries a half-migrated schema. Using
liveness for platform routing also keeps planned-maintenance responses reachable, while
`/api/ready` remains the separately monitored dependency/drain signal.

```
internet ─▶ Cloudflare (DNS · CDN · TLS · WAF)
         ─▶ DigitalOcean App Platform  (managed TLS · ingress · health checks · rollouts)
              ─▶ web  (the ONE combined-image instance: SPA + /api)
                   └─▶ DigitalOcean Managed Postgres  (private VPC, ?sslmode=require)
```

Rough cost: **~$20–27/mo** — App Platform Basic instance ~$5–12 + Managed Postgres ~$15
+ Cloudflare free.

## Why this shape

App Platform is a PaaS, which changes three things versus the Droplet:

- **Container disk is ephemeral** (wiped on every redeploy). TCGLense caches card images
  to disk lazily, so on ephemeral disk that cache would go cold on each deploy and
  re-fetch from Scryfall. The fix is **`CDN_MODE=true`**: the origin keeps no image
  directory and streams from upstream, letting the fronting CDN hold the images — which
  is why **Cloudflare (or another caching CDN) is required**, not optional. Without a CDN
  in front, every image view re-fetches from Scryfall.
- **No custom edge or source-IP firewall.** You can't reproduce the Droplet's Caddy
  `X-Forwarded-For` rewrite or the Cloudflare-IP-only firewall, so per-IP rate limiting
  is **best-effort** here — see [the rate-limit caveat](#the-rate-limit-caveat).
- **The DB is external anyway.** SQLite on ephemeral disk wouldn't survive, so this
  deployment uses Managed Postgres — the same durable store the Droplet guide recommends.

Two things stay drop-in on the published image with **no rebuild** (same as the Droplet
guide): the Turnstile **site key is applied at runtime** (`GET /api/config`), and
**`rediss://` TLS Redis is supported** if you ever add it.

Reference file: [`.do/app.yaml`](../.do/app.yaml) — the App Platform spec.

---

## 1. DigitalOcean Managed Postgres

1. **Create → Databases → PostgreSQL** (latest major), cheapest single-node tier (1 GB
   RAM / 10 GB disk holds the ~2 GB catalog with room to grow). Put it in a **region you
   will also deploy the app to**, attached to a **VPC**.
2. Create a database named `tcglense` (or use `defaultdb`).
3. Your `DATABASE_URL` uses the **private** host, port `25060`, and `?sslmode=require`
   (DO requires TLS; the API supports it):
   ```
   postgres://doadmin:<password>@<private-db-host>:25060/tcglense?sslmode=require
   ```

After the app exists (step 3), add it to the database's **Trusted sources** so only it
can connect. You can instead **attach** the database to the app and reference
`${db.DATABASE_URL}` in the spec — either works; the standalone secret above keeps the
spec portable.

## 2. Cloudflare (required — it is the image CDN)

1. **DNS** — add an `A`/`CNAME` record for your domain pointing at the app's
   `*.ondigitalocean.app` hostname (from step 3), **proxied** (orange cloud).
2. **SSL/TLS → Overview** — set **Full (strict)** and enable **Always Use HTTPS**.
3. **Caching → Cache Rules** — add the same three rules as the Droplet guide
   ([`deploy-digitalocean.md` §4](./deploy-digitalocean.md)) so Cloudflare caches the
   catalog, **images**, sitemaps, and API docs by the origin's `Cache-Control`, and
   bypasses the per-user/live routes. The image cache is what makes `CDN_MODE=true`
   correct. (Rule 1's `/api/mirror/*` clause is a no-op here — App Platform runs with the
   dataset mirror disabled.)

> App Platform serves TLS on `*.ondigitalocean.app` directly, so this origin stays
> publicly reachable even behind Cloudflare — you can't lock ingress to Cloudflare IPs
> the way the Droplet firewall does. That's the root of the rate-limit caveat below.

## 3. Create the app

From the repo root with [`doctl`](https://docs.digitalocean.com/reference/doctl/)
authenticated (`doctl auth init`):

```sh
doctl apps create --spec .do/app.yaml
```

Before running, edit [`.do/app.yaml`](../.do/app.yaml):

- `region` — match your Managed Postgres region.
- `image.registry` — your GHCR owner (default `pnrxa`). Make the `tcglense` GHCR package
  **public** (repo → Packages → package settings) so App Platform can pull it, or add a
  `registry_credentials` block for a private pull.
- `instance_size_slug` — `basic-xs` (~1 GB) is the floor; the **first catalog sync is
  memory-hungry**, so bump a tier if the app OOMs on first boot. Confirm the slug with
  `doctl apps tier instance-size list`.
- `EMAIL_FROM` / `PUBLIC_SITE_URL` — your sending address and domain.

## 4. Set the secrets

The spec declares the secret env vars but not their values. In **App → Settings →
`web` component → Environment Variables**, set:

| Variable | Value |
|---|---|
| `JWT_SECRET` | `openssl rand -hex 32` |
| `DATABASE_URL` | the Managed Postgres **private** URL with `?sslmode=require` |
| `RESEND_API_KEY` | your [Resend](https://resend.com) key — an email provider is required before public signups (or use the Cloudflare pair below) |
| `CLOUDFLARE_EMAIL_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` | alternative to Resend: [Cloudflare Email Service](https://developers.cloudflare.com/email-service/) — set both, or neither; configure only one provider |
| `TURNSTILE_SECRET_KEY` / `TURNSTILE_SITE_KEY` | both; required before public signups can be enabled |

Saving triggers a redeploy. The app runs migrations against Postgres on first boot, then
pulls the catalog from the TCGLense mirror in the background (Postgres fills to ~2 GB
over a few minutes).

> **An email provider is mandatory for public signups.** The server refuses to enable
> public registration without one — either `RESEND_API_KEY` or the Cloudflare Email
> Service pair (`CLOUDFLARE_EMAIL_API_TOKEN` + `CLOUDFLARE_ACCOUNT_ID`); configure only
> one. In local development, leaving email unset returns the
> completion token in the response and skips the verified-email login gate. Use a
> verified sending domain and point `EMAIL_FROM` at it (the default
> `onboarding@resend.dev` only delivers to the Resend account owner).
>
> **Turnstile keys are all-or-nothing and required for public registration** — set
> both before changing `SIGNUPS_ENABLED` to `true`.

## 5. Add your domain

**App → Settings → Domains** → add your domain, then create the Cloudflare DNS record
(step 2) pointing at the app's `*.ondigitalocean.app` hostname, proxied. Set
`PUBLIC_SITE_URL` to `https://<your-domain>`.

## 6. Auto-deploy on release (CI)

The release workflow ([`.github/workflows/release.yml`](../.github/workflows/release.yml))
has a `deploy-app-platform` job that, after it publishes the images for a **non-prerelease
GitHub Release**, tells App Platform to redeploy and pull the new `combined` image. It is
a **no-op until you add two repo secrets**:

| Secret | How to get it |
|---|---|
| `DIGITALOCEAN_ACCESS_TOKEN` | DO → API → Tokens (needs app read/write scope) |
| `DIGITALOCEAN_APP_ID` | `doctl apps list` (the app's UUID) |

With both set, cutting a release (`scripts/release.sh`) builds the images and then runs
`doctl apps create-deployment <APP_ID> --force-rebuild --wait`. We trigger a redeploy
(rather than pushing the whole spec) so the app's console-set secrets and domain are
**preserved** — the running app tracks the combined image's `:latest` tag, and each
release moves that tag.

**Rollback / pinning:** to pin or roll back to a specific version, set
`image.tag: vX.Y.Z` in [`.do/app.yaml`](../.do/app.yaml) and run
`doctl apps update <APP_ID> --spec .do/app.yaml` (this one *does* push the whole spec, so
include your envs), or roll back from the App Platform **Activity** tab.

**Planned maintenance:** set the live `MAINTENANCE_MODE` environment variable to `true`.
Saving it triggers a deployment; migrations complete before the server starts, then the
platform `/api/health` check keeps the instance routed so the cached SPA can read fresh
`/api/config` and application requests receive the coded maintenance `503`. Confirm
`/api/ready` reports maintenance, then set the variable back to `false` to leave the mode.

## The rate-limit caveat

The per-IP auth rate limiter keys on the client IP from the **left-most `X-Forwarded-For`
entry** when `TRUST_PROXY_HEADERS=true`. That is only spoof-proof behind a proxy that
**overwrites** XFF — which the Droplet's Caddy does (it rewrites XFF from the
un-forgeable `CF-Connecting-IP`). App Platform gives you no place to insert that rewrite,
and its `*.ondigitalocean.app` origin stays directly reachable, so a determined client
could present a spoofed left-most XFF and dodge the per-IP limit.

This is a **hardening downgrade, not a hole**:

- Rate limiting here is **abuse protection that fails open** — it is not an integrity
  control.
- The **Turnstile CAPTCHA is the real gate on auth**, and it **fails closed**.
- The spec still sets `TRUST_PROXY_HEADERS=true` on purpose: the alternative
  (`false`) would key every request on App Platform's internal router IP — one shared
  bucket that throttles *all* users together. Per-IP-but-spoofable is the better trade.

If you need spoof-proof per-IP limiting, use the Droplet stack instead
([`deploy-digitalocean.md`](./deploy-digitalocean.md)), whose Caddy + Cloudflare-only
firewall reproduce it exactly.

## Do not scale past one instance without Redis

`.do/app.yaml` sets `instance_count: 1` and no `autoscaling` block **deliberately**. The
rate limiters run **in-process** with no shared store, so a second replica keeps its own
separate buckets and the limits diverge per process. Before raising `instance_count` or
adding autoscaling, provision Redis (e.g. Upstash `rediss://…`) and set `REDIS_URL` on
the component so the limiters share state. Postgres is already shared, so that is the only
change needed to go multi-instance.

## 7. Verify

```sh
curl -s https://<your-domain>/api/health         # -> {"status":"ok"}
curl -s https://<your-domain>/api/ready          # -> {"status":"ready"} after a DB round-trip
curl -s https://<your-domain>/api/config          # -> {"maintenance_mode":false,"turnstile_site_key":...}
doctl apps logs <APP_ID> --type run | grep -iE "backend|migrat|listening"
```

Open the site; the catalog populates as the background sync progresses.
