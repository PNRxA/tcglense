/*
 * Security end-to-end tests.
 *
 * These exercise the real auth stack through the browser and the HTTP API: the
 * httpOnly + SameSite refresh cookie, generic (non-enumerable) login failures,
 * bearer-protected routes, malformed-body handling, and that the access token
 * never reaches JS-readable storage.
 *
 * NOTE on email verification: the CI job runs the API with **no email provider**,
 * so email verification is bypassed (register verifies + signs in immediately).
 * These tests therefore assert the bypass behaviour; the verify-first flow (the
 * check-your-email step, the 403 login gate, resend) is enforced only when a
 * provider is configured and is covered by the Rust security tests (which use a
 * mail sink). Dummy mode also seeds a verified dev account (see api/src/tasks.rs)
 * used by the session flows.
 *
 * They need the backend running. The CI e2e job starts the API with the offline
 * dummy catalog (SEED_DUMMY_DATA) and waits for /api/health before invoking
 * Playwright. Locally (a bare `npm run test:e2e` with no API) they skip instead
 * of failing — hence the intentional API-availability guards below.
 */
/* eslint-disable playwright/no-skipped-test */
import { test, expect, type APIRequestContext, type Page } from '@playwright/test'

const PASSWORD = 'password123'

// The verified account seeded by SEED_DUMMY_DATA (api/src/tasks.rs).
const SEEDED_EMAIL = 'e2e@tcglense.test'
const SEEDED_PASSWORD = 'password123'

async function apiReachable(request: APIRequestContext): Promise<boolean> {
  try {
    const res = await request.get('/api/health')
    return res.ok()
  } catch {
    return false
  }
}

function uniqueEmail(tag: string): string {
  return `sec-${tag}-${Date.now()}-${Math.round(Math.random() * 1e9)}@example.com`
}

async function registerViaApi(request: APIRequestContext, email: string) {
  return request.post('/api/auth/register', { data: { email, password: PASSWORD } })
}

/** Access token for the seeded verified account, or null when it isn't there
 * (an API started without SEED_DUMMY_DATA). */
async function loginSeeded(request: APIRequestContext): Promise<string | null> {
  const res = await request.post('/api/auth/login', {
    data: { email: SEEDED_EMAIL, password: SEEDED_PASSWORD },
  })
  if (!res.ok()) return null
  return (await res.json()).access_token as string
}

test.describe('security: auth API contract', () => {
  test.beforeEach(async ({ request, browserName }) => {
    // These hit the HTTP API directly (no browser), so running them once is
    // enough — skip the firefox/webkit duplicates that triple runtime.
    test.skip(browserName !== 'chromium', 'browser-agnostic API checks; run once on chromium')
    test.skip(!(await apiReachable(request)), 'API not reachable; security e2e needs the backend')
  })

  test('register signs you in (no-email dev bypass) and never leaks the hash', async ({
    request,
  }) => {
    // CI runs the API with no email provider, so verification is bypassed:
    // register creates an already-verified account and returns a session. (With a
    // provider configured, register instead returns `access_token: null` and mails
    // a link — that verify-first path is covered by the Rust security tests.)
    const email = uniqueEmail('reg')
    const res = await registerViaApi(request, email)
    expect(res.status()).toBe(201)

    const body = await res.json()
    expect(body.access_token, 'session returned in the no-email bypass').toBeTruthy()
    expect(body.user.email).toBe(email)
    const serialized = JSON.stringify(body)
    expect(serialized).not.toContain('password_hash')
    expect(serialized).not.toContain('$argon2')

    // The refresh token rides ONLY in a hardened Set-Cookie, never the body.
    const setCookie = res
      .headersArray()
      .filter((h) => h.name.toLowerCase() === 'set-cookie')
      .map((h) => h.value)
      .find((v) => v.startsWith('tcglense_refresh=') && !v.startsWith('tcglense_refresh=;'))
    expect(setCookie, 'refresh Set-Cookie present').toBeTruthy()
    expect(serialized).not.toContain(String(setCookie).split('=')[1].split(';')[0])
  })

  test('duplicate registration is rejected with 409', async ({ request }) => {
    const email = uniqueEmail('dup')
    expect((await registerViaApi(request, email)).status()).toBe(201)
    expect((await registerViaApi(request, email)).status()).toBe(409)
  })

  test('login failures are generic — no user enumeration', async ({ request }) => {
    const email = uniqueEmail('login')
    await registerViaApi(request, email)

    const wrongPassword = await request.post('/api/auth/login', {
      data: { email, password: 'definitely-wrong' },
    })
    const noSuchUser = await request.post('/api/auth/login', {
      data: { email: uniqueEmail('ghost'), password: PASSWORD },
    })

    expect(wrongPassword.status()).toBe(401)
    expect(noSuchUser.status()).toBe(401)
    // Identical message in both cases: nothing reveals whether the account exists.
    expect((await wrongPassword.json()).error).toBe((await noSuchUser.json()).error)

    // The correct password signs in — the no-email bypass verifies at register, so
    // there's no verification gate. (The gate's 403 is covered by the Rust tests,
    // which run with a mail sink so verification is enforced.)
    const ok = await request.post('/api/auth/login', {
      data: { email, password: PASSWORD },
    })
    expect(ok.status()).toBe(200)
  })

  test('the recovery endpoints answer generically for unknown addresses', async ({ request }) => {
    // Neither endpoint may reveal whether an account exists: an unknown address
    // gets the same 204 as a real one.
    const forgot = await request.post('/api/auth/forgot-password', {
      data: { email: uniqueEmail('forgot-ghost') },
    })
    expect(forgot.status()).toBe(204)

    const resend = await request.post('/api/auth/resend-verification', {
      data: { email: uniqueEmail('resend-ghost') },
    })
    expect(resend.status()).toBe(204)

    // A garbage token can neither verify an email nor reset a password.
    const badVerify = await request.post('/api/auth/verify-email', {
      data: { token: 'deadbeef' },
    })
    expect(badVerify.status()).toBe(401)
    const badReset = await request.post('/api/auth/reset-password', {
      data: { token: 'deadbeef', password: PASSWORD },
    })
    expect(badReset.status()).toBe(401)
  })

  test('the /me route requires a valid bearer token', async ({ request }) => {
    const token = await loginSeeded(request)
    test.skip(!token, 'seeded e2e account unavailable (API not running with SEED_DUMMY_DATA)')

    expect((await request.get('/api/auth/me')).status()).toBe(401)
    expect(
      (
        await request.get('/api/auth/me', { headers: { Authorization: 'Bearer not.a.jwt' } })
      ).status(),
    ).toBe(401)

    const authed = await request.get('/api/auth/me', {
      headers: { Authorization: `Bearer ${token}` },
    })
    expect(authed.status()).toBe(200)
    expect((await authed.json()).user.email).toBe(SEEDED_EMAIL)
  })

  test('malformed request bodies get the right status and a JSON error', async ({ request }) => {
    // Send raw bytes (a Buffer) so Playwright doesn't re-encode a string `data`
    // into a valid JSON string when the content-type is application/json.
    const badJson = await request.post('/api/auth/login', {
      headers: { 'Content-Type': 'application/json' },
      data: Buffer.from('{ not valid json'),
    })
    expect(badJson.status()).toBe(400)
    expect((await badJson.json()).error).toBeTruthy()

    const wrongType = await request.post('/api/auth/login', {
      headers: { 'Content-Type': 'text/plain' },
      data: Buffer.from('hello'),
    })
    expect(wrongType.status()).toBe(415)

    // Valid JSON, wrong schema (missing password) -> 422.
    const wrongSchema = await request.post('/api/auth/login', {
      data: { email: 'someone@example.com' },
    })
    expect(wrongSchema.status()).toBe(422)
  })

  test('a missing refresh cookie cannot mint an access token', async ({ request }) => {
    // A fresh request context carries no refresh cookie.
    const res = await request.post('/api/auth/refresh')
    expect(res.status()).toBe(401)
  })
})

test.describe('security: browser session', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await apiReachable(request)), 'API not reachable; security e2e needs the backend')
  })

  async function loginViaUi(page: Page, email: string, password: string) {
    await page.goto('/login')
    await page.locator('#email').fill(email)
    await page.locator('#password').fill(password)
    await page.getByRole('button', { name: /sign in/i }).click()
    // Signing in straight from /login (no ?redirect=) lands on the homepage.
    await expect(page).toHaveURL(/\/$/)
  }

  test('session lives in an httpOnly cookie, not JS storage, and logout protects routes', async ({
    page,
    request,
  }) => {
    test.skip(
      !(await loginSeeded(request)),
      'seeded e2e account unavailable (API not running with SEED_DUMMY_DATA)',
    )
    await loginViaUi(page, SEEDED_EMAIL, SEEDED_PASSWORD)

    // The refresh token is httpOnly — invisible to document.cookie.
    const cookieString = await page.evaluate(() => document.cookie)
    expect(cookieString).not.toContain('tcglense_refresh')

    // The access token (a JWT, `eyJ...`) lives in memory only — never in storage.
    const storageDump = await page.evaluate(() =>
      JSON.stringify({ local: { ...localStorage }, session: { ...sessionStorage } }),
    )
    expect(storageDump).not.toContain('eyJ')

    // A reload restores the session from the cookie alone (the signed-in account
    // menu proves the session survived).
    await page.reload()
    await expect(page).toHaveURL(/\/$/)
    await expect(page.getByRole('button', { name: /account menu/i })).toBeVisible()

    // Sign out, then a protected route bounces to login. The account menu is a
    // reka NavigationMenu (matching the Cards/Collection nav), so "Sign out" is a
    // plain button inside the popover, not a dropdown `menuitem`.
    await page.getByRole('button', { name: /account menu/i }).click()
    const signOut = page.getByRole('button', { name: /sign out/i })
    await expect(signOut).toBeVisible()
    await signOut.click()
    await expect(page).toHaveURL(/\/$/)

    await page.goto('/profile')
    await expect(page).toHaveURL(/\/login(\?|$)/)
  })

  test('registering in the no-email bypass signs you straight in', async ({ page }) => {
    // With no email provider (the CI config), register verifies + signs in and
    // lands on the homepage — no check-your-email step. (The verify-first
    // check-your-email / resend UX applies only when a provider is configured and
    // is covered by the Rust security tests.)
    const email = uniqueEmail('reg-ui')
    await page.goto('/register')
    await page.locator('#email').fill(email)
    await page.locator('#password').fill(PASSWORD)
    await page.getByRole('button', { name: /create account/i }).click()

    await expect(page).toHaveURL(/\/$/)
    await expect(page.getByRole('button', { name: /account menu/i })).toBeVisible()
  })

  test('invalid login surfaces a generic error message', async ({ page }) => {
    await page.goto('/login')
    await page.locator('#email').fill(uniqueEmail('nobody'))
    await page.locator('#password').fill('definitely-wrong')
    await page.getByRole('button', { name: /sign in/i }).click()

    await expect(page.getByRole('alert')).toHaveText(/invalid email or password/i)
  })
})
