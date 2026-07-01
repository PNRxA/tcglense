// Regenerates `web/public/og-image.png` — the branded 1200x630 social/link-unfurl
// banner used as the site-wide default Open Graph / Twitter image for every page
// that doesn't set its own (card pages override it with the card art). See the
// default wiring in `web/src/lib/seo.ts` + `web/index.html` (issue #77).
//
// The banner is rendered from an inline HTML/SVG template with Playwright's bundled
// Chromium (already a dev dependency for the e2e run) and screenshotted at exactly
// 1200x630, so the committed PNG is reproducible without a design tool — the same
// checked-in-snapshot approach as `api/scripts/gen-sld-drops.mjs`. Re-run after a
// brand/wordmark/tagline change:
//
//   node web/scripts/gen-og-image.mjs
//
// Requires the Playwright browsers to be installed (`npx playwright install chromium`).

import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { chromium } from 'playwright'

const WIDTH = 1200
const HEIGHT = 630
const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', 'public', 'og-image.png')

// The favicon mark (orange magnifying-glass-over-card lockup), reused verbatim from
// `web/public/favicon.svg` so the banner matches the tab icon and app wordmark.
const MARK = `<svg viewBox="0 0 32 32" xmlns="http://www.w3.org/2000/svg">
  <defs><linearGradient id="g" x1="16" y1="0" x2="16" y2="32" gradientUnits="userSpaceOnUse">
    <stop offset="0" stop-color="#F19B50"/><stop offset="1" stop-color="#E0792C"/>
  </linearGradient></defs>
  <rect width="32" height="32" rx="7" fill="url(#g)"/>
  <rect x="4.5" y="4.5" width="15" height="18" rx="2.5" fill="#fff"/>
  <circle cx="21" cy="21" r="9.2" fill="url(#g)"/>
  <circle cx="21" cy="21" r="6.2" fill="#fff"/>
  <circle cx="21" cy="21" r="3" fill="url(#g)"/>
  <line x1="25.4" y1="25.4" x2="28.5" y2="28.5" stroke="#fff" stroke-width="3.4" stroke-linecap="round"/>
</svg>`

const html = `<!DOCTYPE html>
<html>
<head><meta charset="utf-8"><style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  html, body { width: ${WIDTH}px; height: ${HEIGHT}px; }
  .banner {
    width: ${WIDTH}px; height: ${HEIGHT}px;
    display: flex; flex-direction: column; justify-content: center;
    padding: 96px; position: relative; overflow: hidden;
    background:
      radial-gradient(1100px 700px at 8% -20%, rgba(232, 131, 58, 0.28), transparent 60%),
      linear-gradient(160deg, #17130f 0%, #0b0b0d 55%, #0b0b0d 100%);
    font-family: 'Helvetica Neue', Arial, system-ui, sans-serif;
    color: #ffffff;
  }
  /* Brand accent bar pinned to the bottom edge. */
  .banner::after {
    content: ''; position: absolute; left: 0; right: 0; bottom: 0; height: 12px;
    background: linear-gradient(90deg, #F19B50, #E0792C);
  }
  .lockup { display: flex; align-items: center; gap: 40px; }
  .mark { width: 176px; height: 176px; filter: drop-shadow(0 12px 32px rgba(224, 121, 44, 0.35)); }
  .mark svg { width: 100%; height: 100%; display: block; }
  .wordmark { font-size: 132px; font-weight: 800; letter-spacing: -0.035em; line-height: 1; }
  .tagline {
    margin-top: 52px; max-width: 900px; font-size: 42px; font-weight: 500;
    line-height: 1.32; color: #cbc6bf; letter-spacing: -0.01em;
  }
  .features {
    margin-top: 44px; display: flex; align-items: center; gap: 20px;
    font-size: 28px; font-weight: 600; color: #e8833a;
  }
  .features .dot { color: #4b463f; }
</style></head>
<body>
  <div class="banner">
    <div class="lockup">
      <div class="mark">${MARK}</div>
      <div class="wordmark">TCGLense</div>
    </div>
    <div class="tagline">
      Track trading-card prices over time, catalogue your collection, and follow
      your set-completion progress across games.
    </div>
    <div class="features">
      <span>Prices</span><span class="dot">&bull;</span>
      <span>Collection</span><span class="dot">&bull;</span>
      <span>Set completion</span>
    </div>
  </div>
</body>
</html>`

const browser = await chromium.launch()
try {
  const page = await browser.newPage({ viewport: { width: WIDTH, height: HEIGHT }, deviceScaleFactor: 1 })
  await page.setContent(html, { waitUntil: 'networkidle' })
  await page.screenshot({ path: OUT, clip: { x: 0, y: 0, width: WIDTH, height: HEIGHT } })
  console.log(`Wrote ${OUT} (${WIDTH}x${HEIGHT})`)
} finally {
  await browser.close()
}
