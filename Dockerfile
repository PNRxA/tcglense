# syntax=docker/dockerfile:1
# check=skip=SecretsUsedInArgOrEnv
# ^ The VITE_* build args below are PUBLIC client-side config (baked into the JS
#   bundle by design), not secrets — so BuildKit's secret-in-ARG/ENV check is off.

# TCGLense container images — one multi-stage Dockerfile, three build targets:
#
#   --target api       the Rust API only (serves /api). Serve the SPA separately.
#   --target web       the built Vue SPA, served by Caddy (proxying /api upstream).
#   --target combined  the API + SPA in one image (the API serves the SPA via WEB_ROOT).
#
# The release workflow (.github/workflows/release.yml) builds all three and pushes
# them to GHCR + Docker Hub as tcglense-api / tcglense-web / tcglense. Build one
# locally with e.g. `docker build --target combined -t tcglense .` (context = repo root).

# ---------------------------------------------------------------------------
# Stage: web-builder — compile the Vue SPA to static files (web/dist).
# ---------------------------------------------------------------------------
FROM node:24-slim AS web-builder
WORKDIR /app/web

# Install deps first so the layer is cached until the lockfile changes.
COPY web/package.json web/package-lock.json ./
RUN --mount=type=cache,target=/root/.npm npm ci

# Build-time public config baked into the bundle. VITE_API_URL stays empty so the
# SPA calls same-origin /api — correct for both the combined image and the Caddy
# web image (which proxies /api). VITE_SITE_URL feeds robots.txt's absolute
# Sitemap: URL. (The Turnstile site key is NOT baked in — it's a runtime API env
# var, TURNSTILE_SITE_KEY, the SPA fetches from GET /api/config, so this image needs
# no rebuild to change it.)
ARG VITE_API_URL=""
ARG VITE_SITE_URL=""
ENV VITE_API_URL=$VITE_API_URL \
    VITE_SITE_URL=$VITE_SITE_URL

COPY web/ ./
# `build-only` = `vite build`. It skips the vue-tsc type-check that `npm run build`
# also runs — CI already type-checks, and an image build should just emit artifacts.
RUN npm run build-only

# ---------------------------------------------------------------------------
# Stage: api-builder — compile the Rust API to a release binary.
# ---------------------------------------------------------------------------
FROM rust:1-bookworm AS api-builder
# aws-lc-sys (pulled in by rustls) builds its crypto with cmake + nasm; the base
# image already ships the C toolchain. Everything else — bundled SQLite, argon2,
# the RustCrypto stack — is pure-Rust or vendored, so no other system libs are needed.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake nasm \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app/api

COPY api/ ./
# Cache the cargo registry + target dir so a rebuild only recompiles what changed.
# `--locked` fails if Cargo.lock drifts from Cargo.toml (a release-hygiene guard).
# The binary is copied out of the cache-mounted target into the image filesystem so
# the later COPY --from can pick it up (cache mounts aren't part of the layer).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/api/target \
    cargo build --release --locked \
    && cp target/release/tcglense-api /usr/local/bin/tcglense-api

# ---------------------------------------------------------------------------
# Stage: runtime-base — shared minimal runtime for the api + combined images.
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim AS runtime-base
# ca-certificates: outbound HTTPS to Scryfall / TCGCSV / Resend. tini: a tiny init
# as PID 1 so SIGTERM/SIGINT reach the API and zombies are reaped (clean shutdown).
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*
# Run as a non-root user. /data (the DATA_DIR and the default SQLite path) is owned
# by it, so a fresh named volume inherits writable ownership on first mount.
RUN useradd --system --create-home --uid 10001 app \
    && mkdir -p /data \
    && chown app:app /data
ENV HOST=0.0.0.0 \
    PORT=8080 \
    DATA_DIR=/data
WORKDIR /data
USER app
EXPOSE 8080
VOLUME ["/data"]
# JWT_SECRET is intentionally NOT defaulted — the API refuses to boot without a real
# one. Pass it at run time, e.g. `-e JWT_SECRET=$(openssl rand -hex 32)`.
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["tcglense-api"]

# ---------------------------------------------------------------------------
# Target: api — the Rust API only (serves /api; serve the SPA elsewhere).
# ---------------------------------------------------------------------------
FROM runtime-base AS api
COPY --from=api-builder /usr/local/bin/tcglense-api /usr/local/bin/tcglense-api

# ---------------------------------------------------------------------------
# Target: combined — the API + SPA in one process (the API serves the SPA via WEB_ROOT).
# ---------------------------------------------------------------------------
FROM runtime-base AS combined
COPY --from=api-builder /usr/local/bin/tcglense-api /usr/local/bin/tcglense-api
COPY --from=web-builder /app/web/dist /srv/web
ENV WEB_ROOT=/srv/web

# ---------------------------------------------------------------------------
# Target: web — the built SPA, served by Caddy (static files + /api reverse proxy).
# ---------------------------------------------------------------------------
FROM caddy:2-alpine AS web
COPY deploy/web.Caddyfile /etc/caddy/Caddyfile
COPY --from=web-builder /app/web/dist /srv
EXPOSE 80
