import { afterEach, describe, expect, it } from 'vitest'
import { effectScope, nextTick, ref } from 'vue'
import { absoluteUrl, SITE_DESCRIPTION, SITE_NAME, usePageMeta } from '../seo'

// Read the current value of a managed head tag, or null if absent.
function meta(attr: 'name' | 'property', key: string): string | null {
  return document.head.querySelector(`meta[${attr}="${key}"]`)?.getAttribute('content') ?? null
}
function canonical(): string | null {
  return document.head.querySelector('link[rel="canonical"]')?.getAttribute('href') ?? null
}
function jsonLd(): string | null {
  return document.head.querySelector('script[type="application/ld+json"]')?.textContent ?? null
}

// Run the composable inside an effect scope so watchEffect / onScopeDispose behave
// as they would in a mounted component. Returns the scope so tests can stop it.
function run(options: Parameters<typeof usePageMeta>[0]) {
  const scope = effectScope()
  scope.run(() => usePageMeta(options))
  return scope
}

afterEach(() => {
  // Reset the document head between tests (jsdom persists it across cases).
  document.head.querySelectorAll('meta, link[rel="canonical"], script').forEach((el) => el.remove())
  document.title = ''
})

describe('absoluteUrl', () => {
  it('passes through absolute URLs and makes paths absolute', () => {
    expect(absoluteUrl('https://cdn.example.com/x.png')).toBe('https://cdn.example.com/x.png')
    expect(absoluteUrl('/cards/mtg')).toBe(`${window.location.origin}/cards/mtg`)
    expect(absoluteUrl('cards/mtg')).toBe(`${window.location.origin}/cards/mtg`)
    expect(absoluteUrl(undefined)).toBeUndefined()
    expect(absoluteUrl('')).toBeUndefined()
  })
})

describe('usePageMeta', () => {
  it('sets the title, description, canonical, and og/twitter tags', () => {
    run({ title: 'All cards', description: 'Every card', canonicalPath: '/cards/mtg' })

    expect(document.title).toBe(`All cards · ${SITE_NAME}`)
    expect(meta('name', 'description')).toBe('Every card')
    expect(meta('name', 'robots')).toBe('index, follow')
    expect(canonical()).toBe(`${window.location.origin}/cards/mtg`)

    expect(meta('property', 'og:title')).toBe('All cards')
    expect(meta('property', 'og:description')).toBe('Every card')
    expect(meta('property', 'og:type')).toBe('website')
    expect(meta('property', 'og:url')).toBe(`${window.location.origin}/cards/mtg`)
    expect(meta('property', 'og:site_name')).toBe(SITE_NAME)
    // No image → the small summary card, and no image tags.
    expect(meta('name', 'twitter:card')).toBe('summary')
    expect(meta('property', 'og:image')).toBeNull()
  })

  it('falls back to the site name and description when omitted', () => {
    run({})
    expect(document.title).toBe(SITE_NAME)
    expect(meta('name', 'description')).toBe(SITE_DESCRIPTION)
    expect(meta('property', 'og:title')).toBe(SITE_NAME)
  })

  it('marks noindex pages', () => {
    run({ title: 'Sign in', noindex: true })
    expect(meta('name', 'robots')).toBe('noindex, nofollow')
  })

  it('emits a large image card and structured data when given an image + json-ld', () => {
    run({
      title: 'Sol Ring',
      image: 'https://cdn.example.com/sol-ring.png',
      type: 'product',
      jsonLd: { '@type': 'Product', name: 'Sol Ring' },
    })

    expect(meta('property', 'og:type')).toBe('product')
    expect(meta('property', 'og:image')).toBe('https://cdn.example.com/sol-ring.png')
    expect(meta('name', 'twitter:card')).toBe('summary_large_image')
    expect(meta('name', 'twitter:image')).toBe('https://cdn.example.com/sol-ring.png')
    expect(JSON.parse(jsonLd() ?? '{}')).toMatchObject({ '@type': 'Product', name: 'Sol Ring' })
  })

  it('reacts to changing inputs', async () => {
    const title = ref('First')
    run({ title: () => title.value, canonicalPath: () => '/cards/mtg' })
    expect(document.title).toBe(`First · ${SITE_NAME}`)

    title.value = 'Second'
    await nextTick()
    expect(document.title).toBe(`Second · ${SITE_NAME}`)
    expect(meta('property', 'og:title')).toBe('Second')
  })

  it('clears the per-page image and structured data when the view unmounts', () => {
    const scope = run({
      title: 'Sol Ring',
      image: 'https://cdn.example.com/sol-ring.png',
      jsonLd: { '@type': 'Product', name: 'Sol Ring' },
    })
    expect(meta('property', 'og:image')).not.toBeNull()
    expect(jsonLd()).not.toBeNull()

    scope.stop()
    expect(meta('property', 'og:image')).toBeNull()
    expect(meta('name', 'twitter:image')).toBeNull()
    expect(jsonLd()).toBeNull()
  })
})
