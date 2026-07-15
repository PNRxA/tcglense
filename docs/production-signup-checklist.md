# Production signup launch checklist

Keep `SIGNUPS_ENABLED=false` through the initial deploy. The production manifests
default to closed registration so migrations and the existing-user paths can be
verified before a new account is accepted. Changing the flag does not affect login,
password reset for an active account, or authenticated users.

## Before deploying

- Generate a unique `JWT_SECRET` with `openssl rand -hex 32`. Never reuse the
  example/dev value or a secret from another environment.
- Set an exact HTTPS `PUBLIC_SITE_URL`, `COOKIE_SECURE=true`, and a verified Resend
  `EMAIL_FROM`. Send a real test email from that sender and confirm SPF/DKIM status
  in the provider dashboard.
- Configure both Turnstile keys. Restrict the widget to the production hostname;
  the app additionally validates Siteverify's hostname and `auth` action.
- Put Postgres on durable storage, verify automated backups and point-in-time
  recovery, and complete a restore drill. A backup that has never been restored is
  not a recovery plan.
- Configure Redis for shared rate limits when the API has more than one replica.
  With `TRUST_PROXY_HEADERS=true`, ensure the trusted edge **replaces** inbound
  `X-Forwarded-For`; never expose that API port around the edge.
- Confirm the production `.env`, origin certificate/key, database URL, and provider
  credentials are not tracked by Git. Rotate any secret that was ever committed.
- Confirm the `privacy@tcglense.com` and `contact@tcglense.com` mailboxes are
  monitored. The legal pages describe the implemented data flows, but are not a
  substitute for counsel reviewing the operator, jurisdiction, and audience.
- Set error/log retention and alerts for API restarts, 5xx responses, Postgres
  capacity/connections, Resend delivery failures, and unusual CAPTCHA/429 volume.

## Deploy closed

For an existing DigitalOcean App Platform service, update the **live** environment
and health-check settings before deploying the image: the release workflow requests a
new deployment but does not apply `.do/app.yaml` changes to an existing app. Set
`SIGNUPS_ENABLED=false`, configure `/api/ready`, then verify the live `/api/config`.

1. Take a pre-deploy database snapshot, deploy the new images, and let migrations
   finish. Do not switch signups on while an older API version is still serving.
2. Confirm liveness and dependency readiness:

   ```sh
   curl -fsS https://YOUR_DOMAIN/api/health
   curl -fsS https://YOUR_DOMAIN/api/ready
   ```

3. Confirm the runtime switch and CAPTCHA public key:

   ```sh
   curl -fsS https://YOUR_DOMAIN/api/config
   ```

   The response must show `"signups_enabled":false` and a non-null
   `turnstile_site_key`.
4. Inspect response headers and verify at least HSTS, `Referrer-Policy: no-referrer`,
   `X-Content-Type-Options: nosniff`, frame protection, and the narrow CSP are
   present.
5. Submit a registration while closed. It must return `403` with the configured
   notice. Then smoke-test an existing user's login, refresh, logout, and password
   reset; those flows must remain available.

## Canary the real signup journey

Prefer staging. To canary on production, first restrict registration at the edge to
your test IP or an authenticated Cloudflare Access policy; `SIGNUPS_ENABLED` is a
global switch and is not itself a one-user canary. Then set it to `true` and use a
new mailbox.

- Submit the email and confirm the generic response never exposes a completion
  token in production.
- Open the message and verify the link uses the exact production HTTPS origin. The
  token must disappear from the browser address bar immediately after the page loads.
- Complete the final password/username step, including the Terms and Privacy
  acknowledgement, and confirm the resulting account is signed in.
- Refresh and open a second tab; both the session and displayed account identity must
  remain correct. Log out and confirm the refresh cookie can no longer restore it.
- Request two reset emails. After one reset succeeds, the sibling reset link, every
  pre-reset access token, and every pre-reset refresh session must be rejected.
- Replay a rotated refresh token and confirm only that login family is revoked; a
  separate browser/device session must remain signed in.
- Exercise resend/forgot with both existing and unknown addresses and confirm the UI
  and HTTP responses do not reveal which address exists.

If any check fails, immediately restore `SIGNUPS_ENABLED=false`; no rollback is
needed to keep existing accounts working.

## Open and monitor

After the canary passes, leave `SIGNUPS_ENABLED=true` and watch the first hour closely:
registration completions versus mail delivery, CAPTCHA errors, auth 4xx/429/5xx,
database latency/pool use, Redis availability, and container restarts. Keep the
signup switch documented as the first incident-response lever.
