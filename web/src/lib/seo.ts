// Reactive per-page `<head>` management for SEO and social/link previews.
//
// TCGLense is a client-rendered SPA, so out of the box every route shares the one
// static `<title>`/description from index.html. `usePageMeta` lets each view set a
// route-specific title, description, canonical URL, Open Graph / Twitter card tags,
// and JSON-LD structured data — updated reactively as the underlying data (a card,
// a set) loads. Search engines that execute JS (Googlebot) pick these up; the
// baseline tags in index.html cover crawlers that don't (see the file's caveat).
//
// This deliberately avoids a head-management dependency: the app is CSR-only, the
// tag set is small, and mutating the live document keeps it testable in jsdom.

import { onScopeDispose, toValue, watchEffect, type MaybeRefOrGetter } from 'vue'

/** Product/brand name, used as the title suffix and `og:site_name`. */
export const SITE_NAME = 'TCGLense'

/** Fallback description for any page that doesn't set its own. */
export const SITE_DESCRIPTION =
  'Track trading-card prices over time, catalogue your collection, and follow your ' +
  'set-completion progress across games.'

/** Site-wide default social/link-preview banner (`web/public/og-image.png`, 1200×630),
 * used as the `og:image` / `twitter:image` for any page that doesn't set its own — so
 * every share gets a branded `summary_large_image` card, not the tiny textless one.
 * Card pages override it with the card art. Regenerate via `web/scripts/gen-og-image.mjs`;
 * a matching absolute baseline copy lives in `index.html` for non-JS unfurlers. */
export const DEFAULT_OG_IMAGE = '/og-image.png'

export interface PageMetaOptions {
  /** Page title, without the site suffix (which is appended automatically). */
  title?: MaybeRefOrGetter<string | null | undefined>
  /** Meta/OG description; falls back to [`SITE_DESCRIPTION`]. */
  description?: MaybeRefOrGetter<string | null | undefined>
  /** Canonical URL as a root-relative path (e.g. `/cards/mtg`); made absolute
   * against the current origin. Omit query params so paginated/search variants
   * collapse onto one canonical page. */
  canonicalPath?: MaybeRefOrGetter<string | null | undefined>
  /** Preview image: an absolute URL or a root-relative path (made absolute). */
  image?: MaybeRefOrGetter<string | null | undefined>
  /** `og:type` (default `website`). */
  type?: MaybeRefOrGetter<string | null | undefined>
  /** Keep this page out of search indexes (auth + app-only pages). */
  noindex?: MaybeRefOrGetter<boolean | undefined>
  /** JSON-LD structured data (e.g. a `Product`); serialized into a script tag. */
  jsonLd?: MaybeRefOrGetter<Record<string, unknown> | null | undefined>
}

function currentOrigin(): string {
  return typeof window === 'undefined' ? '' : window.location.origin
}

/** Resolve a path or absolute URL to an absolute URL; `undefined` passes through. */
export function absoluteUrl(pathOrUrl: string | null | undefined): string | undefined {
  if (!pathOrUrl) return undefined
  if (/^https?:\/\//i.test(pathOrUrl)) return pathOrUrl
  const origin = currentOrigin()
  return `${origin}${pathOrUrl.startsWith('/') ? pathOrUrl : `/${pathOrUrl}`}`
}

// Tags this composable creates (as opposed to the baseline ones already in
// index.html) are marked so they can be safely removed again when a later route
// doesn't set them. Baseline tags always receive a value, so they're only updated.
const MANAGED = 'seo'

function upsertMeta(attr: 'name' | 'property', key: string, content: string | undefined) {
  const el = document.head.querySelector<HTMLMetaElement>(`meta[${attr}="${key}"]`)
  if (!content) {
    if (el?.dataset.managed === MANAGED) el.remove()
    return
  }
  if (el) {
    el.setAttribute('content', content)
    return
  }
  const created = document.createElement('meta')
  created.setAttribute(attr, key)
  created.setAttribute('content', content)
  created.dataset.managed = MANAGED
  document.head.appendChild(created)
}

function upsertLink(rel: string, href: string | undefined) {
  const el = document.head.querySelector<HTMLLinkElement>(`link[rel="${rel}"]`)
  if (!href) {
    if (el?.dataset.managed === MANAGED) el.remove()
    return
  }
  if (el) {
    el.setAttribute('href', href)
    return
  }
  const created = document.createElement('link')
  created.setAttribute('rel', rel)
  created.setAttribute('href', href)
  created.dataset.managed = MANAGED
  document.head.appendChild(created)
}

function upsertJsonLd(data: Record<string, unknown> | null | undefined) {
  const el = document.head.querySelector<HTMLScriptElement>(
    `script[type="application/ld+json"][data-managed="${MANAGED}"]`,
  )
  if (!data) {
    el?.remove()
    return
  }
  if (el) {
    el.textContent = JSON.stringify(data)
    return
  }
  const created = document.createElement('script')
  created.type = 'application/ld+json'
  created.dataset.managed = MANAGED
  created.textContent = JSON.stringify(data)
  document.head.appendChild(created)
}

/**
 * Apply reactive `<head>` metadata for the current page. Call once per view; it
 * re-applies whenever any reactive input changes, and clears the optional per-page
 * tags (preview image, JSON-LD) when the view unmounts so they don't leak to the
 * next route. No-op during SSR / when there's no document.
 */
export function usePageMeta(options: PageMetaOptions = {}): void {
  if (typeof document === 'undefined') return

  watchEffect(() => {
    const title = toValue(options.title) || undefined
    const description = toValue(options.description) || SITE_DESCRIPTION
    const canonical = absoluteUrl(toValue(options.canonicalPath))
    // Fall back to the branded default banner so every page has a large preview image
    // (card pages pass their own art, which wins).
    const image = absoluteUrl(toValue(options.image)) || absoluteUrl(DEFAULT_OG_IMAGE)
    const ogType = toValue(options.type) || 'website'
    const noindex = toValue(options.noindex) ?? false
    const jsonLd = toValue(options.jsonLd) ?? undefined

    document.title = title ? `${title} · ${SITE_NAME}` : SITE_NAME

    upsertMeta('name', 'description', description)
    upsertMeta('name', 'robots', noindex ? 'noindex, nofollow' : 'index, follow')
    upsertLink('canonical', canonical)

    upsertMeta('property', 'og:title', title ?? SITE_NAME)
    upsertMeta('property', 'og:description', description)
    upsertMeta('property', 'og:type', ogType)
    upsertMeta('property', 'og:site_name', SITE_NAME)
    upsertMeta('property', 'og:url', canonical)
    upsertMeta('property', 'og:image', image)

    upsertMeta('name', 'twitter:card', image ? 'summary_large_image' : 'summary')
    upsertMeta('name', 'twitter:title', title ?? SITE_NAME)
    upsertMeta('name', 'twitter:description', description)
    upsertMeta('name', 'twitter:image', image)

    upsertJsonLd(jsonLd)
  })

  // On unmount, reset the preview image back to the site-wide default banner (a card
  // view overrides og:image/twitter:image with its art) and drop the page-specific
  // JSON-LD, so a subsequent view that doesn't set its own can't inherit a stale card
  // image. The always-present baseline tags are overwritten by the next view.
  onScopeDispose(() => {
    const fallback = absoluteUrl(DEFAULT_OG_IMAGE)
    upsertMeta('property', 'og:image', fallback)
    upsertMeta('name', 'twitter:image', fallback)
    upsertJsonLd(null)
  })
}
