/*
 * The homepage's universal search box, end to end: type into it, see the grouped
 * dropdown answer across the seeded catalog, open a hit, and hand off to the full card
 * search with Enter.
 *
 * Needs the API running with the offline dummy catalog (SEED_DUMMY_DATA) — see
 * security.spec.ts for why readiness, not liveness, is the gate. Without an API the
 * specs skip rather than fail.
 */
/* eslint-disable playwright/no-skipped-test */
import { test, expect, type APIRequestContext, type Page } from '@playwright/test'

async function apiReachable(request: APIRequestContext): Promise<boolean> {
  try {
    const res = await request.get('/api/ready')
    return res.ok()
  } catch {
    return false
  }
}

function searchBox(page: Page) {
  return page.getByRole('combobox', { name: /search cards, sets, sealed products/i })
}

test.describe('homepage universal search', () => {
  test.beforeEach(async ({ request }) => {
    test.skip(!(await apiReachable(request)), 'API not reachable; skipping')
  })

  test('answers across cards, sets, sealed products and precons as you type', async ({
    page,
  }) => {
    await page.goto('/')
    const box = searchBox(page)
    await box.fill('dummy')

    const listbox = page.getByRole('listbox', { name: 'Search results' })
    await expect(listbox).toBeVisible()
    // Every seeded card, set, product and precon is named "Dummy …", so all four groups
    // answer, and the seven products overflow the group into a "see all" row.
    await expect(listbox.getByRole('group', { name: 'Cards' })).toBeVisible()
    await expect(listbox.getByRole('group', { name: 'Sets' })).toBeVisible()
    const sealed = listbox.getByRole('group', { name: 'Sealed products' })
    await expect(sealed.getByRole('option', { name: /All sealed products matching/ })).toBeVisible()
    await expect(listbox.getByRole('group', { name: 'Preconstructed decks' })).toBeVisible()
    // Signed out, there is no "Your decks" group.
    await expect(listbox.getByRole('group', { name: 'Your decks' })).toHaveCount(0)

    // A term that is a set name asks about the set: "dummy universe" answers the set (and
    // the product and precon named after it), and no card — cards then match by name
    // alone, and none is called "Dummy Universe".
    await box.fill('dummy universe')
    const precons = listbox.getByRole('group', { name: 'Preconstructed decks' })
    await expect(precons.getByRole('option', { name: /Dummy Universe Commander/ })).toBeVisible()
    const sets = listbox.getByRole('group', { name: 'Sets' })
    await expect(sets.getByRole('option', { name: /^Dummy Universe/ })).toBeVisible()
    await expect(listbox.getByRole('group', { name: 'Cards' })).toHaveCount(0)

    // But a word the card name lacks may name its set: "relic universe" is no set name, so
    // "universe" narrows the reprinted relic to its Dummy Universe printing, and the row
    // names that printing.
    await box.fill('relic universe')
    const cards = listbox.getByRole('group', { name: 'Cards' })
    const relic = cards.getByRole('option', { name: /Dummy Reprinted Relic/ })
    await expect(relic).toBeVisible()
    await expect(relic).toContainText('Dummy Universe #')
    await expect(listbox.getByRole('group', { name: 'Sets' })).toHaveCount(0)
  })

  test('opens a set from its row', async ({ page }) => {
    await page.goto('/')
    await searchBox(page).fill('dummy base')
    const listbox = page.getByRole('listbox', { name: 'Search results' })
    await listbox
      .getByRole('group', { name: 'Sets' })
      .getByRole('option', { name: /^Dummy Base Set/ })
      .first()
      .click()
    await expect(page).toHaveURL(/\/cards\/mtg\/sets\/dmb$/)
  })

  test('finds a keyword by name and opens its glossary page', async ({ page }) => {
    await page.goto('/')
    await searchBox(page).fill('vigilance')
    const listbox = page.getByRole('listbox', { name: 'Search results' })
    await listbox
      .getByRole('group', { name: 'Keywords' })
      .getByRole('option', { name: /^Vigilance/ })
      .click()
    await expect(page).toHaveURL(/\/keywords\/mtg\/vigilance$/)
  })

  test('Enter hands off to the full card search for what was typed', async ({ page }) => {
    await page.goto('/')
    const box = searchBox(page)
    await box.fill('relic')
    await expect(page.getByRole('listbox', { name: 'Search results' })).toBeVisible()
    await box.press('Enter')
    await expect(page).toHaveURL(/\/cards\/mtg\/cards\?q=relic$/)
  })

  test('arrow keys highlight rows and Enter opens the highlighted one', async ({ page }) => {
    await page.goto('/')
    const box = searchBox(page)
    await box.fill('reprinted')
    const listbox = page.getByRole('listbox', { name: 'Search results' })
    // The reprint folds to one card row; ArrowDown highlights it.
    const option = listbox.getByRole('option', { name: /Dummy Reprinted Relic/ })
    await expect(option).toBeVisible()
    await box.press('ArrowDown')
    await expect(option).toHaveAttribute('aria-selected', 'true')
    await box.press('Enter')
    await expect(page).toHaveURL(/\/cards\/mtg\/cards\/[^/?]+$/)
  })

  test('says so when nothing matches, and still offers the full search', async ({ page }) => {
    await page.goto('/')
    await searchBox(page).fill('zzzz nothing')
    const listbox = page.getByRole('listbox', { name: 'Search results' })
    await expect(listbox.getByRole('status')).toContainText(/no cards, sets, sealed products/i)
    await expect(listbox.getByRole('option', { name: /Search all cards for/ })).toBeVisible()
  })
})
