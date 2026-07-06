<script setup lang="ts">
import { RouterLink, useRouter } from 'vue-router'
import GitHubMark from '@/components/GitHubMark.vue'
import { prefetchRouteChunks } from '@/lib/prefetch'

// Build-time app version (injected via the `define` in vite.config.ts), shown next to the
// brand so visitors and bug reporters can tell which release is deployed (issue #250).
const version = import.meta.env.VITE_APP_VERSION

// Site-wide footer, mounted once in App.vue so it renders on every route. It carries the
// data-source credits (Scryfall / TCGCSV / MTGJSON), the GitHub links, the Terms/Privacy
// links, and the required WotC Fan Content disclaimer — placed wherever card data/images
// render (nearly every page). Fully static: no queries, no auth reads, identical signed in
// or out.

// The legal pages are lazy-loaded route chunks; warm them on hover/focus so the click
// lands on a loaded view (see lib/prefetch.ts — chunks only, never data/images). The
// other footer links are external <a> tags (no chunk) or already-loaded routes.
const router = useRouter()
const warm = (to: string) => prefetchRouteChunks(router, to)
</script>

<template>
  <footer class="border-t">
    <div class="mx-auto max-w-6xl px-4 py-10">
      <!-- Tier 1: link columns. Col 1 is the brand; cols 2–4 sit inside one Footer nav
           landmark (display:contents so the 4-col grid still lays them out). -->
      <div class="grid gap-8 sm:grid-cols-2 lg:grid-cols-4">
        <div>
          <div class="flex items-baseline gap-2">
            <RouterLink to="/" class="text-lg font-semibold tracking-tight">TCGLense</RouterLink>
            <span class="text-muted-foreground text-xs font-medium">v{{ version }}</span>
          </div>
          <p class="text-muted-foreground mt-2 text-sm text-pretty">
            Track trading-card prices and your collection.
          </p>
          <a
            href="https://github.com/PNRxA/tcglense"
            target="_blank"
            rel="noopener noreferrer"
            aria-label="TCGLense on GitHub"
            class="text-muted-foreground hover:text-foreground mt-3 inline-flex transition-colors"
          >
            <GitHubMark class="size-5" />
          </a>
        </div>

        <nav aria-label="Footer" class="contents">
          <div>
            <h2 class="text-muted-foreground text-xs font-medium tracking-wide uppercase">
              Product
            </h2>
            <ul class="mt-3 space-y-2">
              <li>
                <RouterLink
                  to="/cards"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                >
                  Cards
                </RouterLink>
              </li>
              <li>
                <RouterLink
                  to="/sealed"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                >
                  Sealed products
                </RouterLink>
              </li>
              <li>
                <RouterLink
                  to="/collection"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                >
                  Collection
                </RouterLink>
              </li>
              <li>
                <RouterLink
                  to="/wishlist"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                >
                  Wish list
                </RouterLink>
              </li>
            </ul>
          </div>

          <div>
            <h2 class="text-muted-foreground text-xs font-medium tracking-wide uppercase">
              Data sources
            </h2>
            <ul class="mt-3 space-y-2">
              <li>
                <a
                  href="https://scryfall.com"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                >
                  Scryfall
                </a>
              </li>
              <li>
                <a
                  href="https://tcgcsv.com"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                >
                  TCGCSV
                </a>
              </li>
              <li>
                <a
                  href="https://mtgjson.com"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                >
                  MTGJSON
                </a>
              </li>
            </ul>
          </div>

          <div>
            <h2 class="text-muted-foreground text-xs font-medium tracking-wide uppercase">
              Project
            </h2>
            <ul class="mt-3 space-y-2">
              <li>
                <a
                  href="https://github.com/PNRxA/tcglense"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                >
                  GitHub
                </a>
              </li>
              <li>
                <a
                  href="https://github.com/PNRxA/tcglense/issues"
                  target="_blank"
                  rel="noopener noreferrer"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                >
                  Report an issue
                </a>
              </li>
              <li>
                <RouterLink
                  to="/terms"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                  @pointerenter="warm('/terms')"
                  @focusin="warm('/terms')"
                >
                  Terms of Service
                </RouterLink>
              </li>
              <li>
                <RouterLink
                  to="/privacy"
                  class="text-muted-foreground hover:text-foreground text-sm transition-colors"
                  @pointerenter="warm('/privacy')"
                  @focusin="warm('/privacy')"
                >
                  Privacy Policy
                </RouterLink>
              </li>
            </ul>
          </div>
        </nav>
      </div>

      <!-- Tier 2: legally-required small print — the WotC Fan Content disclaimer, the
           open-data attribution, and the price-estimate caveat. -->
      <div class="text-muted-foreground mt-8 space-y-2 border-t pt-6 text-xs text-pretty">
        <p>
          TCGLense is unofficial Fan Content permitted under the Fan Content Policy. Not approved or
          endorsed by Wizards. Portions of the materials used are property of Wizards of the Coast.
          © Wizards of the Coast LLC.
        </p>
        <p>
          Card data and images courtesy of Scryfall. Sealed product pricing from TCGCSV; sealed
          product contents from MTGJSON. None of these projects endorses or is affiliated with
          TCGLense.
        </p>
        <p>
          Prices are estimates for informational purposes only and can lag the market. They are not
          financial advice.
        </p>
      </div>
    </div>
  </footer>
</template>
