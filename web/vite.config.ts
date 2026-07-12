import { fileURLToPath, URL } from 'node:url'
import { readFileSync } from 'node:fs'

import { defineConfig, type Plugin } from 'vite'
import vue from '@vitejs/plugin-vue'
import vueDevTools from 'vite-plugin-vue-devtools'
import tailwindcss from '@tailwindcss/vite'

// Proxy /api to the backend so the browser stays same-origin (the httpOnly
// refresh cookie is first-party). The root sitemap paths are served by the API
// too (issue #294), mirroring the production proxies (deploy/*.Caddyfile).
// Shared by the dev server and the preview server — the latter backs the
// production-like e2e run on CI.
const apiProxy = {
  '/api': {
    target: 'http://localhost:8080',
    changeOrigin: true,
  },
  '/sitemap.xml': {
    target: 'http://localhost:8080',
    changeOrigin: true,
  },
  '/sitemaps': {
    target: 'http://localhost:8080',
    changeOrigin: true,
  },
  // robots.txt is API-served too (its Sitemap: line needs an absolute PUBLIC_SITE_URL);
  // proxy it in dev/preview so both match production (deploy/*.Caddyfile).
  '/robots.txt': {
    target: 'http://localhost:8080',
    changeOrigin: true,
  },
}

// Public site URL, used only to rewrite the baseline og:image in index.html to an
// absolute URL for non-JS unfurlers (below). Canonical/OG URLs are otherwise resolved at
// runtime from the live origin (src/lib/seo.ts), and robots.txt + the DB-backed sitemaps
// (issues #75/#294) are served by the API against PUBLIC_SITE_URL — so neither is emitted
// here. Set VITE_SITE_URL in production CI; the localhost default keeps dev/e2e valid.
const SITE_URL = (process.env.VITE_SITE_URL ?? 'http://localhost:5173').replace(/\/$/, '')

// The default social/link-preview banner (web/public/og-image.png). index.html carries
// it as a root-relative baseline og:image/twitter:image for readability; we rewrite it
// to an absolute URL below since unfurlers that don't run JS need one (see seo.ts /
// DEFAULT_OG_IMAGE for the runtime, per-route default).
const OG_IMAGE_PATH = '/og-image.png'
const OG_IMAGE_URL = `${SITE_URL}${OG_IMAGE_PATH}`

// App version, read from package.json and injected at build time so the footer can show
// which release is deployed (issue #250). Read here rather than imported so vue-tsc's
// project build doesn't pull package.json (outside src/) into the app's type graph.
const APP_VERSION = (
  JSON.parse(
    readFileSync(fileURLToPath(new URL('./package.json', import.meta.url)), 'utf-8'),
  ) as { version: string }
).version

// Rewrite the baseline og:image/twitter:image in index.html to an absolute VITE_SITE_URL
// so non-JS unfurlers get one. (robots.txt and the sitemaps used to be emitted here too;
// both are now served by the API against PUBLIC_SITE_URL — see api's handlers::robots /
// handlers::sitemap — so their URLs are correct at runtime regardless of the build args.)
function absoluteBaselineOgImage(): Plugin {
  return {
    name: 'tcglense-absolute-baseline-og-image',
    transformIndexHtml(html) {
      return html.replaceAll(`content="${OG_IMAGE_PATH}"`, `content="${OG_IMAGE_URL}"`)
    },
  }
}

// https://vite.dev/config/
export default defineConfig({
  define: {
    // Expose the package.json version to the app (consumed in AppFooter.vue). Nothing
    // sets VITE_APP_VERSION via .env, so this define is its sole source — no conflict
    // with Vite's own env injection.
    'import.meta.env.VITE_APP_VERSION': JSON.stringify(APP_VERSION),
  },
  plugins: [vue(), vueDevTools(), tailwindcss(), absoluteBaselineOgImage()],
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    proxy: apiProxy,
  },
  preview: {
    proxy: apiProxy,
  },
})
