<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { ApiReference } from '@scalar/api-reference'
import '@scalar/api-reference/style.css'
import { usePageMeta } from '@/lib/seo'
import { useAuthStore } from '@/stores/auth'

// The interactive public-API reference (issue #284), rendered in-app with Scalar's
// ApiReference component off the same `GET /api/openapi.json` document the standalone
// `/api/docs` page embeds. Public and indexable — anyone can read the catalog reads and
// see how account holders mint scoped API keys.
//
// The spec is fetched once on mount (it's a public, cacheable document) and handed to
// Scalar as `content`. A signed-in visitor's access token is attached so the fetch works
// even if the spec is ever gated; the endpoint ignores it while public. The Scalar
// configuration matches the standalone page: no HTTP-client / MCP / agent buttons, no dev
// tools, forced light mode with the toggle hidden, and every tag expanded by default.
const auth = useAuthStore()
const spec = ref<unknown>(null)

usePageMeta({
  title: 'API Reference',
  description:
    'Interactive reference for the TCGLense public API — anonymous catalog reads for cards, ' +
    'sets, sealed products and prices, plus scoped API keys for your collection and wish list.',
  canonicalPath: '/docs',
})

onMounted(async () => {
  const headers: Record<string, string> = {}
  if (auth.accessToken) {
    headers['Authorization'] = `Bearer ${auth.accessToken}`
  }
  const res = await fetch('/api/openapi.json', { headers })
  if (res.ok) {
    spec.value = await res.json()
  }
})
</script>

<template>
  <div class="docs-page">
    <ApiReference
      v-if="spec"
      :configuration="{
        content: spec,
        hideClientButton: true,
        showDeveloperTools: 'never',
        mcp: { disabled: true },
        agent: { disabled: true },
        forceDarkModeState: 'light',
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
</style>
