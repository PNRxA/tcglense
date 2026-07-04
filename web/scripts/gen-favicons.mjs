// Regenerates the raster favicon assets from `web/public/favicon.svg` — the one
// hand-authored source of the app mark (black rounded-square with a white
// card + magnifying-glass). Keeps `favicon.ico`, `apple-touch-icon.png`,
// `icon-192.png`, and `icon-512.png` in sync with the SVG so a brand tweak only
// needs editing the SVG and re-running this:
//
//   node web/scripts/gen-favicons.mjs
//
// Each PNG is rendered from the SVG with Playwright's bundled Chromium (already a
// dev dependency for the e2e run) at its exact pixel size, with transparent
// corners preserved (`omitBackground`) — the same checked-in-snapshot approach as
// `gen-og-image.mjs`. `favicon.ico` is assembled from the 16px + 32px PNGs as a
// PNG-embedded ICO (supported by every modern browser; the SVG is the primary
// icon anyway via `<link rel="icon" type="image/svg+xml">`).
//
// Requires the Playwright browsers to be installed (`npx playwright install chromium`).

import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'
import { readFileSync, writeFileSync } from 'node:fs'
import { chromium } from 'playwright'

const PUBLIC = join(dirname(fileURLToPath(import.meta.url)), '..', 'public')
const SVG = readFileSync(join(PUBLIC, 'favicon.svg'), 'utf8')

// size -> output filename (favicon.ico is assembled separately from 16 + 32)
const PNGS = [
  [180, 'apple-touch-icon.png'],
  [192, 'icon-192.png'],
  [512, 'icon-512.png'],
]
const ICO_SIZES = [16, 32]

async function renderPng(page, size) {
  await page.setViewportSize({ width: size, height: size })
  await page.setContent(
    `<!doctype html><meta charset="utf-8"><style>*{margin:0;padding:0}
     html,body{width:${size}px;height:${size}px}
     svg{width:${size}px;height:${size}px;display:block}</style>${SVG}`,
  )
  return page.locator('svg').screenshot({ omitBackground: true })
}

// Assemble a PNG-embedded .ico from [{ size, png }] entries.
function buildIco(entries) {
  const header = Buffer.alloc(6)
  header.writeUInt16LE(0, 0) // reserved
  header.writeUInt16LE(1, 2) // type: 1 = icon
  header.writeUInt16LE(entries.length, 4)

  const dir = Buffer.alloc(16 * entries.length)
  let offset = header.length + dir.length
  entries.forEach((e, i) => {
    const b = i * 16
    dir.writeUInt8(e.size >= 256 ? 0 : e.size, b + 0) // width (0 = 256)
    dir.writeUInt8(e.size >= 256 ? 0 : e.size, b + 1) // height
    dir.writeUInt8(0, b + 2) // palette size
    dir.writeUInt8(0, b + 3) // reserved
    dir.writeUInt16LE(1, b + 4) // color planes
    dir.writeUInt16LE(32, b + 6) // bits per pixel
    dir.writeUInt32LE(e.png.length, b + 8) // image byte size
    dir.writeUInt32LE(offset, b + 12) // image byte offset
    offset += e.png.length
  })

  return Buffer.concat([header, dir, ...entries.map((e) => e.png)])
}

const browser = await chromium.launch()
try {
  const page = await browser.newPage({ deviceScaleFactor: 1 })

  for (const [size, name] of PNGS) {
    writeFileSync(join(PUBLIC, name), await renderPng(page, size))
    console.log(`Wrote ${name} (${size}x${size})`)
  }

  const icoEntries = []
  for (const size of ICO_SIZES) icoEntries.push({ size, png: await renderPng(page, size) })
  writeFileSync(join(PUBLIC, 'favicon.ico'), buildIco(icoEntries))
  console.log(`Wrote favicon.ico (${ICO_SIZES.join(' + ')})`)
} finally {
  await browser.close()
}
