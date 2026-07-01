import { test, expect } from '@playwright/test'

// See here how to get started:
// https://playwright.dev/docs/intro
test('shows the public welcome page to everyone', async ({ page }) => {
  await page.goto('/')
  await expect(page).toHaveURL(/\/$/)
  await expect(page.getByRole('heading', { name: /track every card/i })).toBeVisible()
  // Signed out, the top-bar profile selector collapses to a sign-in link that carries
  // a ?redirect= back to the current page (so login returns the user here).
  await expect(page.locator('header a[href^="/login"]')).toBeVisible()
})

test('redirects unauthenticated visitors away from protected pages', async ({ page }) => {
  await page.goto('/profile')
  // The guard sends them to /login, carrying a ?redirect= back to where they
  // were headed, so match the login path with or without that query string.
  await expect(page).toHaveURL(/\/login(\?|$)/)
  await expect(page.getByRole('button', { name: /sign in/i })).toBeVisible()
})
