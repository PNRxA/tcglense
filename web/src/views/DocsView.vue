<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ApiReference } from '@scalar/api-reference'
import '@scalar/api-reference/style.css'
import { getOpenApiDocument } from '@/lib/api'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'
import { useThemeStore } from '@/stores/theme'

// The interactive public-API reference (issue #284), rendered in-app with Scalar's
// ApiReference component off the same `GET /api/openapi.json` document the standalone
// `/api/docs` page embeds. Public and indexable — anyone can read the catalog reads and
// see how account holders mint scoped API keys.
//
// The spec is fetched once on mount (it's a public, cacheable document) and handed to
// Scalar as `content`. A signed-in visitor's access token is attached so the fetch works
// even if the spec is ever gated; the endpoint ignores it while public. The Scalar
// configuration: no HTTP-client / MCP / agent buttons, no dev tools, and every tag
// expanded by default. Scalar's own dark-mode toggle stays hidden — instead its state is
// pinned to the app theme (the header's ThemeToggle) so the reference matches the rest of
// the site in both light and dark. Scalar only reads `forceDarkModeState` when it mounts,
// so `:key` on the component re-keys it to the resolved theme: a live toggle remounts the
// reference (cheap — the spec is already in hand) and it comes back in the new mode.
const auth = useAuthStore()
const theme = useThemeStore()
const spec = ref<unknown>(null)

usePageMeta({
  title: 'API Reference',
  description:
    'Interactive reference for the TCGLense public API — anonymous catalog reads for cards, ' +
    'sets, sealed products and prices, plus scoped API keys for your collection and wish list.',
  canonicalPath: '/docs',
})

onMounted(async () => {
  try {
    spec.value = await getOpenApiDocument(auth.accessToken)
  } catch {
    // The shared client has already signalled a coded maintenance response to App.vue.
    // Preserve the existing loading placeholder for other transient fetch failures.
  }
})
</script>

<template>
  <div class="docs-page">
    <ApiReference
      v-if="spec"
      :key="theme.resolvedTheme"
      :configuration="{
        content: spec,
        hideClientButton: true,
        showDeveloperTools: 'never',
        mcp: { disabled: true },
        agent: { disabled: true },
        forceDarkModeState: theme.resolvedTheme,
        hideDarkModeToggle: true,
        defaultOpenAllTags: true,
      }"
    />
    <p v-else class="text-muted-foreground px-4 py-16 text-center text-sm">
      Loading API reference…
    </p>
  </div>
</template>

<style scoped>
/* Let Scalar own the full content width of the page (the app shell's <main> is
   full-bleed); the component brings its own sidebar + scrolling. */
.docs-page :deep(.scalar-app) {
  min-height: calc(100vh - 3.5rem);
}

/* Scalar keeps the API-client ("Test Request") modal permanently mounted and lays its
   overlay wrapper out as an extra grid child of the reference layout. With no grid-area
   assigned it lands in the 288px sidebar column and, at z-index 10000 with pointer-events
   enabled, it swallows every click on the sidebar links + search even while the modal is
   closed (the wider content column is outside that 288px box, so it stayed clickable —
   which is why only the sidebar felt dead). Make the wrapper click-through and hand
   pointer events back to the modal panel itself, so the sidebar works and Test Request
   still opens/closes normally. */
.docs-page :deep(.scalar-app.z-overlay) {
  pointer-events: none;
}
.docs-page :deep(.scalar-app.z-overlay [aria-modal='true']) {
  pointer-events: auto;
}
</style>
