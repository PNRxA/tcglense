import { test, expect } from '@playwright/test'

// See here how to get started:
// https://playwright.dev/docs/intro
test('shows the public welcome page to everyone', async ({ page }) => {
  await page.goto('/')
  await expect(page).toHaveURL(/\/$/)
  await expect(page.getByRole('heading', { name: /track every card/i })).toBeVisible()
  // Signed out, the profile selector collapses to a sign-in link to /login.
  await expect(page.locator('a[href="/login"]').first()).toBeVisible()
})

test('redirects unauthenticated visitors away from protected pages', async ({ page }) => {
  await page.goto('/dashboard')
  await expect(page).toHaveURL(/\/login$/)
  await expect(page.getByRole('button', { name: /sign in/i })).toBeVisible()
})
