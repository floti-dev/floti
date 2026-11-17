/**
 * Rasterises the brand's colour mark (`assets/brand/mark-colour-circle.svg`,
 * cropped to its circle) into `build/icon-<size>.png`, `icon.png`, `icon.ico`
 * and `icon.icns`, and writes `preview.png`: 16 to 48px magnified on a dark and
 * a light taskbar, since a transparent icon is only ever seen on one of them.
 *
 * Each size is rendered from the vector at its own resolution by a headless
 * Chrome (Edge, then Playwright's own Chromium, when Chrome is not installed).
 * The ICO and ICNS are packed here, with nothing beyond Node.
 *
 * Run with `pnpm icon` when the artwork changes and commit the result; it is
 * not part of the build.
 */
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from '@playwright/test'

const HERE = dirname(fileURLToPath(import.meta.url))
const BUILD = join(HERE, '..', 'build')
const SOURCE = join(HERE, '..', '..', '..', 'assets', 'brand', 'mark-colour-circle.svg')

/** 256 must be present - electron-builder rejects an icon without it. */
const SIZES = [16, 24, 32, 48, 64, 128, 256]

/** The circle's bounds in the artboard (getBBox), so it fills the icon edge to edge. */
const CROP = '23.2 23.8 243 243'

/**
 * The share of the canvas a macOS app icon's artwork takes. Apple's icon grid
 * puts the body in an 824px square centred on the 1024px canvas, and the Dock
 * and Finder draw every app's icon at the canvas size, so artwork that fills
 * the canvas the way the Windows icon does stands visibly larger than the icons
 * beside it.
 */
const MAC_FILL = 824 / 1024

/** The source with its viewBox cropped to the circle, as a data URL. */
function croppedSource() {
  const svg = readFileSync(SOURCE, 'utf8')
  const cropped = svg.replace(/(<svg\b[^>]*?\sviewBox=")[^"]*(")/, `$1${CROP}$2`)
  if (cropped === svg) throw new Error(`${SOURCE} has no viewBox on its <svg> to crop`)
  return `data:image/svg+xml;base64,${Buffer.from(cropped).toString('base64')}`
}

/** A page showing `body`, with no margin and no background of its own. */
const page = (body) =>
  `<!doctype html><html><head><style>html,body{margin:0;background:transparent}img{display:block}</style></head><body>${body}</body></html>`

/**
 * Chrome, then Edge, then the Chromium Playwright installs: the first that is
 * installed. Any other failure to start is a real one and is thrown.
 */
async function launch() {
  for (const channel of ['chrome', 'msedge']) {
    try {
      return await chromium.launch({ channel })
    } catch (error) {
      // Playwright's own wording for a channel with no browser on this machine.
      const missing = error instanceof Error && /distribution '[^']+' is not (found|supported)/.test(error.message)
      if (!missing) throw error
    }
  }
  return chromium.launch()
}

/**
 * A screenshot's PNG, checked to be what an ICO or ICNS entry has to be: 8-bit
 * RGBA, `size` pixels square. An entry of another kind packs without complaint
 * and then shows as a blank square on the taskbar.
 */
function checkedPng(png, size) {
  const ok =
    png.toString('ascii', 12, 16) === 'IHDR' &&
    png.readUInt32BE(16) === size &&
    png.readUInt32BE(20) === size &&
    png[24] === 8 &&
    png[25] === 6
  if (!ok) throw new Error(`the ${String(size)}px render is not an 8-bit RGBA PNG of that size`)
  return png
}

// ---------------------------------------------------------------------------
// ICO: a 6-byte header, a 16-byte entry per image, then the payloads.
// Entries carry PNG rather than BMP, which every Windows since Vista reads.
// ---------------------------------------------------------------------------

function buildIco(entries) {
  const header = Buffer.alloc(6)
  header.writeUInt16LE(0, 0)
  header.writeUInt16LE(1, 2)
  header.writeUInt16LE(entries.length, 4)

  const dir = Buffer.alloc(16 * entries.length)
  let offset = 6 + 16 * entries.length
  entries.forEach(({ size, png }, i) => {
    const at = i * 16
    // 256 is written as 0: the field is one byte and 256 does not fit in it.
    dir.writeUInt8(size >= 256 ? 0 : size, at)
    dir.writeUInt8(size >= 256 ? 0 : size, at + 1)
    dir.writeUInt8(0, at + 2)
    dir.writeUInt8(0, at + 3)
    dir.writeUInt16LE(1, at + 4)
    dir.writeUInt16LE(32, at + 6)
    dir.writeUInt32LE(png.length, at + 8)
    dir.writeUInt32LE(offset, at + 12)
    offset += png.length
  })

  return Buffer.concat([header, dir, ...entries.map((e) => e.png)])
}

// ---------------------------------------------------------------------------
// ICNS: the magic `icns` and the file's length, then one element per image -
// a four-character type, the element's length including its 8-byte header, and
// a PNG. Every type below takes PNG data on the macOS versions Electron runs on.
// ---------------------------------------------------------------------------

/**
 * The iconset `iconutil` would build: each point size at 1x and 2x, as
 * [type, pixels]. The pixel size is what the PNG must be; two types can share
 * one (a 16pt@2x image is a 32px image).
 */
const ICNS_TYPES = [
  ['icp4', 16],
  ['ic11', 32],
  ['icp5', 32],
  ['ic12', 64],
  ['ic07', 128],
  ['ic13', 256],
  ['ic08', 256],
  ['ic14', 512],
  ['ic09', 512],
  ['ic10', 1024]
]

/**
 * The pixel sizes taken from the Windows images rather than rendered on the
 * macOS grid: they appear only in Finder's list and sidebar, where every icon
 * is drawn full, and the inset would leave the ship 13 and 26px across.
 */
const ICNS_FROM_WINDOWS = new Set([16, 32])

function buildIcns(pngBySize) {
  const elements = ICNS_TYPES.map(([type, size]) => {
    const png = pngBySize.get(size)
    if (png === undefined) throw new Error(`no ${String(size)}px image for icns type ${type}`)
    const header = Buffer.alloc(8)
    header.write(type, 0, 'ascii')
    header.writeUInt32BE(8 + png.length, 4)
    return Buffer.concat([header, png])
  })
  const header = Buffer.alloc(8)
  header.write('icns', 0, 'ascii')
  header.writeUInt32BE(8 + elements.reduce((sum, e) => sum + e.length, 0), 4)
  return Buffer.concat([header, ...elements])
}

const browser = await launch()
try {
  const tab = await (await browser.newContext({ deviceScaleFactor: 1 })).newPage()
  const source = croppedSource()

  /**
   * The mark rendered `size` pixels square, its art about `fill` of that and
   * centred. The inset is a whole number of pixels, so the circle's edge is
   * not smeared across half of one (`fill` of 64 would put it 6.25px in).
   */
  async function render(size, fill = 1) {
    const at = Math.round((size * (1 - fill)) / 2)
    const art = size - 2 * at
    await tab.setViewportSize({ width: size, height: size })
    await tab.setContent(
      page(
        `<img src="${source}" style="position:absolute;left:${String(at)}px;top:${String(at)}px;width:${String(art)}px;height:${String(art)}px">`
      )
    )
    const png = await tab.screenshot({ omitBackground: true, clip: { x: 0, y: 0, width: size, height: size } })
    return checkedPng(png, size)
  }

  const entries = []
  for (const size of SIZES) {
    const png = await render(size)
    writeFileSync(join(BUILD, `icon-${String(size)}.png`), png)
    process.stdout.write(`  ${String(size).padStart(3)}px  ${String(png.length).padStart(6)} bytes\n`)
    entries.push({ size, png })
  }

  writeFileSync(join(BUILD, 'icon.ico'), buildIco(entries))
  // electron-builder's Linux target and the runtime window icon both want a PNG.
  writeFileSync(join(BUILD, 'icon.png'), entries[entries.length - 1].png)
  process.stdout.write(`icon.ico written with ${String(entries.length)} sizes\n`)

  const macPngs = new Map()
  for (const [, size] of ICNS_TYPES) {
    if (macPngs.has(size)) continue
    const fromWindows = ICNS_FROM_WINDOWS.has(size) ? entries.find((e) => e.size === size) : undefined
    macPngs.set(size, fromWindows?.png ?? (await render(size, MAC_FILL)))
  }
  writeFileSync(join(BUILD, 'icon.icns'), buildIcns(macPngs))
  process.stdout.write(`icon.icns written with ${String(ICNS_TYPES.length)} images\n`)

  // Not shipped: the sizes that actually get looked at, magnified nearest
  // neighbour from the PNGs just packed, on Windows 11's dark and light
  // taskbars. Every scale is a whole number, so each pixel stays square.
  const shown = [16, 24, 32, 48]
  const cell = 48 * 8
  const pad = 12
  const width = shown.length * cell + (shown.length + 1) * pad
  const height = 2 * (cell + 2 * pad)
  const images = shown
    .map((size) => {
      const png = entries.find((e) => e.size === size).png
      return `<img src="data:image/png;base64,${png.toString('base64')}" width="${String(cell)}" height="${String(cell)}" style="image-rendering:pixelated">`
    })
    .join('')
  const row = (ground) =>
    `<div style="display:flex;gap:${String(pad)}px;padding:${String(pad)}px;background:${ground}">${images}</div>`
  await tab.setViewportSize({ width, height })
  await tab.setContent(page(row('#202020') + row('#f3f3f3')))
  writeFileSync(join(BUILD, 'preview.png'), await tab.screenshot({ clip: { x: 0, y: 0, width, height } }))
  process.stdout.write('preview.png written\n')
} finally {
  await browser.close()
}
