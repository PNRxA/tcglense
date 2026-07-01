/*
 * Security end-to-end tests.
 *
 * These exercise the real auth stack through the browser and the HTTP API: the
 * httpOnly + SameSite refresh cookie, generic (non-enumerable) login failures,
 * bearer-protected routes, malformed-body handling, and that the access token
 * never reaches JS-readable storage.
 *
 * They need the backend running. The CI e2e job starts the API with the offline
 * dummy catalog and waits for /api/health before invoking Playwright, so these
 * run there. Locally (a bare `npm run test:e2e` with no API) they skip instead of
 * failing — hence the intentional API-availability guard below.
 */
/* eslint-disable playwright/no-skipped-test */
import { test, expect, type APIRequestContext, type Page } from '@playwright/test'

const PASSWORD = 'password123'

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

test.describe('security: auth API contract', () => {
  test.beforeEach(async ({ request, browserName }) => {
    // These hit the HTTP API directly (no browser), so running them once is
    // enough — skip the firefox/webkit duplicates that triple runtime.
    test.skip(browserName !== 'chromium', 'browser-agnostic API checks; run once on chromium')
    test.skip(!(await apiReachable(request)), 'API not reachable; security e2e needs the backend')
  })

  test('register sets a hardened httpOnly refresh cookie and never leaks the hash', async ({
    request,
  }) => {
    const email = uniqueEmail('reg')
    const res = await registerViaApi(request, email)
    expect(res.status()).toBe(201)

    const body = await res.json()
    expect(body.access_token).toBeTruthy()
    expect(body.user.email).toBe(email)
    const serialized = JSON.stringify(body)
    expect(serialized).not.toContain('password_hash')
    expect(serialized).not.toContain('$argon2')

    const setCookie = res
      .headersArray()
      .filter((h) => h.name.toLowerCase() === 'set-cookie')
      .map((h) => h.value)
      .find((v) => v.startsWith('tcglense_refresh='))
    expect(setCookie, 'refresh Set-Cookie present').toBeTruthy()
    const cookie = setCookie as string
    expect(cookie.toLowerCase()).toContain('httponly')
    expect(cookie.toLowerCase()).toContain('samesite=lax')
    expect(cookie).toContain('Path=/api/auth')
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
  })

  test('the /me route requires a valid bearer token', async ({ request }) => {
    const email = uniqueEmail('me')
    const reg = await registerViaApi(request, email)
    const token = (await reg.json()).access_token as string

    expect((await request.get('/api/auth/me')).status()).toBe(401)
    expect(
      (await request.get('/api/auth/me', { headers: { Authorization: 'Bearer not.a.jwt' } })).status(),
    ).toBe(401)

    const authed = await request.get('/api/auth/me', {
      headers: { Authorization: `Bearer ${token}` },
    })
    expect(authed.status()).toBe(200)
    expect((await authed.json()).user.email).toBe(email)
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

  async function registerViaUi(page: Page, email: string) {
    await page.goto('/register')
    await page.locator('#email').fill(email)
    await page.locator('#password').fill(PASSWORD)
    await page.getByRole('button', { name: /create account/i }).click()
    // Registering straight from /register (no ?redirect=) lands on the homepage.
    await expect(page).toHaveURL(/\/$/)
  }

  test('session lives in an httpOnly cookie, not JS storage, and logout protects routes', async ({
    page,
  }) => {
    await registerViaUi(page, uniqueEmail('ui'))

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

    // Sign out, then a protected route bounces to login.
    await page.getByRole('button', { name: /account menu/i }).click()
    const signOut = page.getByRole('menuitem', { name: /sign out/i })
    await expect(signOut).toBeVisible()
    await signOut.click()
    await expect(page).toHaveURL(/\/$/)

    await page.goto('/profile')
    await expect(page).toHaveURL(/\/login(\?|$)/)
  })

  test('invalid login surfaces a generic error message', async ({ page }) => {
    await page.goto('/login')
    await page.locator('#email').fill(uniqueEmail('nobody'))
    await page.locator('#password').fill('definitely-wrong')
    await page.getByRole('button', { name: /sign in/i }).click()

    await expect(page.getByRole('alert')).toHaveText(/invalid email or password/i)
  })
})
