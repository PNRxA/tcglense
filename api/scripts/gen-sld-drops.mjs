// Regenerates `api/src/scryfall/sld_drops.json` — the committed **seed / offline
// fallback** for the Secret Lair drop store. Scryfall's curated drop titles are NOT in
// its bulk card API; they exist only on a set's gallery page, which groups the set's
// cards into named sections by collector number: the Secret Lair Drop set (`sld`) into
// its "drops" (e.g. "Wild in Bloom"), and The Zeta Set (`slz`, a Secret Lair release
// Scryfall files as its own top-level set) into its three print treatments (Photocopy /
// Photocopy Negatives / Color Banding), which the card data doesn't distinguish either.
// `SETS` names every gallery scraped and mirrors `scryfall::sld_scrape::GALLERY_SETS` —
// keep the two in step.
//
// This scrape is ALSO ported to Rust (`scryfall::sld_scrape`) and run at runtime by
// the mirror origin, which re-scrapes daily and re-serves the fresh snapshot at
// `/api/mirror/scryfall/sld-drops` for other instances to import — so the API no longer
// depends on a human re-running this script. This script stays the way to regenerate the
// committed fallback (kept in step with the Rust parser). Re-run after new drops release:
//
//   node api/scripts/gen-sld-drops.mjs            # re-scrape every set in SETS
//   node api/scripts/gen-sld-drops.mjs slz        # re-scrape only the named set(s);
//                                                 # every other set keeps its current
//                                                 # entry from the committed file
//
// The per-set form is for landing one set's sections without churning the others (a
// re-scrape of `sld` moves hundreds of drops whenever new ones have released, and a few
// tests pin its seeded order).
//
// Requires Node 18+ (global fetch). No npm dependencies.

import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

const GAME = 'mtg'
// Mirrors `GALLERY_SETS` in `api/src/scryfall/sld_scrape.rs`; `sld` first (the set the drop
// store's install guard requires).
const SETS = ['sld', 'slz']
const OUT = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'scryfall', 'sld_drops.json')

const galleryUrl = (set) => `https://scryfall.com/sets/${set}`

const NAMED_ENTITIES = {
  amp: '&',
  lt: '<',
  gt: '>',
  quot: '"',
  apos: "'",
  nbsp: ' ',
  mdash: '—',
  ndash: '–',
  hellip: '…',
  rsquo: '’',
  lsquo: '‘',
  ldquo: '“',
  rdquo: '”',
}

function decodeEntities(s) {
  return s.replace(/&(#x?[0-9a-fA-F]+|[a-zA-Z]+);/g, (whole, body) => {
    if (body[0] === '#') {
      const code =
        body[1] === 'x' || body[1] === 'X'
          ? parseInt(body.slice(2), 16)
          : parseInt(body.slice(1), 10)
      return Number.isFinite(code) ? String.fromCodePoint(code) : whole
    }
    return Object.prototype.hasOwnProperty.call(NAMED_ENTITIES, body)
      ? NAMED_ENTITIES[body]
      : whole
  })
}

function cleanTitle(html) {
  return decodeEntities(html.replace(/<[^>]+>/g, ' '))
    .replace(/\s+/g, ' ')
    .trim()
}

// Parse one set's gallery HTML into its ordered sections. Each is an
// <h2 class="card-grid-header" id="slug"> whose body holds the title, followed by a card
// grid of /card/<set>/<collector-number>/... links (only the links into `set` count — a
// gallery page can link to another set's printing).
function parseGallery(html, set) {
  const headerRe = /<h2 class="card-grid-header" id="([^"]+)">([\s\S]*?)<\/h2>/g
  const headers = [...html.matchAll(headerRe)]
  if (headers.length === 0) {
    throw new Error(`${set}: no card-grid-header blocks found — Scryfall markup may have changed`)
  }

  const seen = new Map() // collector_number -> drop slug (collision detection)
  let collisions = 0
  const drops = []

  headers.forEach((h, i) => {
    const slug = h[1]
    const titleMatch = h[2].match(
      /card-grid-header-content"\s*>([\s\S]*?)<span class="card-grid-header-dot/,
    )
    const title = titleMatch ? cleanTitle(titleMatch[1]) : cleanTitle(h[2])

    // A drop's membership is the markup between its own header and the next one.
    // Slice it from the same match positions (rather than a second split regex,
    // which could match a different set of <h2>s and silently misalign bodies).
    const bodyStart = h.index + h[0].length
    const bodyEnd = i + 1 < headers.length ? headers[i + 1].index : html.length
    const body = html.slice(bodyStart, bodyEnd)
    const cnRe = new RegExp(`/card/${set}/([^/"]+)/`, 'g')
    const seenHere = new Set()
    const collectorNumbers = []
    for (const m of body.matchAll(cnRe)) {
      const cn = decodeURIComponent(m[1])
      if (seenHere.has(cn)) continue // dedupe variant printings sharing a number
      seenHere.add(cn)
      if (seen.has(cn)) {
        collisions++
        continue // first drop to list a number keeps it
      }
      seen.set(cn, slug)
      collectorNumbers.push(cn)
    }

    if (collectorNumbers.length > 0) drops.push({ slug, title, collector_numbers: collectorNumbers })
  })
  if (drops.length === 0) {
    throw new Error(`${set}: every section was empty — Scryfall markup may have changed`)
  }
  return { drops, numbers: seen.size, collisions }
}

async function scrape(set) {
  const url = galleryUrl(set)
  const res = await fetch(url, {
    headers: { 'User-Agent': 'TCGLense-dev-tooling/1.0 (sld drop snapshot generator)' },
  })
  if (!res.ok) throw new Error(`GET ${url} -> HTTP ${res.status}`)
  return parseGallery(await res.text(), set)
}

// The committed file's current per-set entries, so a partial regeneration keeps the sets
// it wasn't asked to re-scrape. Absent file = nothing to keep.
function currentEntries() {
  if (!existsSync(OUT)) return new Map()
  const parsed = JSON.parse(readFileSync(OUT, 'utf8'))
  return new Map(parsed.sets.filter((s) => s.game === GAME).map((s) => [s.set, s.drops]))
}

async function main() {
  const requested = process.argv.slice(2)
  for (const set of requested) {
    if (!SETS.includes(set)) {
      throw new Error(`unknown set '${set}' — add it to SETS (and to GALLERY_SETS in sld_scrape.rs)`)
    }
  }
  const toScrape = requested.length > 0 ? requested : SETS
  const kept = currentEntries()

  const blocks = []
  for (const set of SETS) {
    let drops
    if (toScrape.includes(set)) {
      // Scryfall's request-rate guidance: a short pause between pages.
      if (blocks.length > 0) await new Promise((resolve) => setTimeout(resolve, 100))
      const scraped = await scrape(set)
      drops = scraped.drops
      console.log(
        `${set}: scraped ${drops.length} drops, ${scraped.numbers} collector numbers` +
          (scraped.collisions ? `, ${scraped.collisions} cross-drop collisions skipped` : ''),
      )
    } else if (kept.has(set)) {
      drops = kept.get(set)
      console.log(`${set}: kept ${drops.length} drops from the committed file`)
    } else {
      console.warn(`${set}: not re-scraped and absent from the committed file — omitted`)
      continue
    }
    const lines = drops.map((d) => `        ${JSON.stringify(d)}`).join(',\n')
    blocks.push(`    {
      "game": "${GAME}",
      "set": "${set}",
      "drops": [
${lines}
      ]
    }`)
  }

  const json = `{
  "//": "GENERATED by api/scripts/gen-sld-drops.mjs from Scryfall's set galleries (https://scryfall.com/sets/<set>) — do not edit by hand.",
  "//2": "Scryfall's curated Secret Lair section titles (sld drops; slz print treatments), not present in the bulk card API.",
  "sets": [
${blocks.join(',\n')}
  ]
}
`
  writeFileSync(OUT, json)
  console.log(` -> ${OUT}`)
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
