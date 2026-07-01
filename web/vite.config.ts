import { fileURLToPath, URL } from 'node:url'
import type { IncomingMessage, ServerResponse } from 'node:http'

import { defineConfig, type Plugin } from 'vite'
import vue from '@vitejs/plugin-vue'
import vueDevTools from 'vite-plugin-vue-devtools'
import tailwindcss from '@tailwindcss/vite'

// Proxy /api to the backend so the browser stays same-origin (the httpOnly
// refresh cookie is first-party). Shared by the dev server and the preview
// server — the latter backs the production-like e2e run on CI.
const apiProxy = {
  '/api': {
    target: 'http://localhost:8080',
    changeOrigin: true,
  },
}

// Public site URL, used to build the absolute <loc>s a valid sitemap requires and
// the Sitemap: line in robots.txt. Canonical/OG URLs are resolved at runtime from
// the live origin (see src/lib/seo.ts), so ONLY the build-time sitemap needs this.
// Set VITE_SITE_URL in production CI; the localhost default keeps dev/e2e valid.
const SITE_URL = (process.env.VITE_SITE_URL ?? 'http://localhost:5173').replace(/\/$/, '')

// Static public routes worth advertising to crawlers. The dynamic catalog (games,
// sets, hundreds of thousands of cards) is discovered by following links — a full
// per-card sitemap would need a DB-backed, paginated generator (future work).
const SITEMAP_ROUTES = ['/', '/cards']

function robotsTxt(): string {
  return [
    'User-agent: *',
    'Allow: /',
    // Keep auth + signed-in app pages out of the index (also set via `noindex`
    // at runtime, but declared here for non-JS crawlers).
    'Disallow: /login',
    'Disallow: /register',
    'Disallow: /dashboard',
    'Disallow: /profile',
    '',
    `Sitemap: ${SITE_URL}/sitemap.xml`,
    '',
  ].join('\n')
}

function sitemapXml(): string {
  const urls = SITEMAP_ROUTES.map((route) => `  <url><loc>${SITE_URL}${route}</loc></url>`).join(
    '\n',
  )
  return `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`
}

// Emit robots.txt + sitemap.xml at build time (correct absolute URLs), and serve
// the same content in the dev/preview servers so both environments match. They're
// generated rather than committed to public/ so the URLs track VITE_SITE_URL and
// can't go stale against the route list above.
function seoDiscoveryFiles(): Plugin {
  const files: Record<string, { body: string; type: string }> = {
    '/robots.txt': { body: robotsTxt(), type: 'text/plain' },
    '/sitemap.xml': { body: sitemapXml(), type: 'application/xml' },
  }
  const handle = (req: IncomingMessage, res: ServerResponse, next: () => void) => {
    const path = req.url?.split('?')[0]
    const file = path ? files[path] : undefined
    if (!file) return next()
    res.setHeader('Content-Type', file.type)
    res.end(file.body)
  }
  return {
    name: 'tcglense-seo-discovery-files',
    // The dev and preview servers expose different types, so register on each.
    configureServer(server) {
      server.middlewares.use(handle)
    },
    configurePreviewServer(server) {
      server.middlewares.use(handle)
    },
    generateBundle() {
      this.emitFile({ type: 'asset', fileName: 'robots.txt', source: robotsTxt() })
      this.emitFile({ type: 'asset', fileName: 'sitemap.xml', source: sitemapXml() })
    },
  }
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [vue(), vueDevTools(), tailwindcss(), seoDiscoveryFiles()],
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
