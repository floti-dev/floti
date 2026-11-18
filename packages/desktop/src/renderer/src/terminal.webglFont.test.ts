import { readFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'

/**
 * The WebGL renderer draws every glyph on a canvas whose font it sets as
 * `${italic} ${weight} ${size}px ${family}`, and for upright text `italic` is
 * empty, so the font starts with a space. Chrome 150 (Electron 43.8.10)
 * refuses a canvas font that starts with a space and keeps `10px sans-serif`:
 * the grid, measured by xterm's core without the space, stays the terminal
 * font's, and every glyph is drawn small and proportional inside it.
 *
 * `patches/@xterm__addon-webgl@0.19.0.patch` trims both fonts the addon
 * builds. This reads the addon the renderer bundles and fails while either is
 * untrimmed, so a new addon version cannot bring the bug back unnoticed.
 */

const ADDON_LIB = join(resolve(import.meta.dirname, '../../../../..'), 'node_modules', '@xterm', 'addon-webgl', 'lib')

/** Every template literal in `source` that builds a canvas font: `... ${size}px ${family}`, and what follows it. */
function canvasFonts(source: string): string[] {
  return [...source.matchAll(/`[^`]*\}px \$\{[^`]*`(\.trim\(\))?/g)].map(([font]) => font)
}

describe('the WebGL renderer’s canvas fonts', () => {
  for (const file of ['addon-webgl.mjs', 'addon-webgl.js']) {
    it(`never start with a space, in ${file}`, () => {
      const fonts = canvasFonts(readFileSync(join(ADDON_LIB, file), 'utf8'))
      // The glyph atlas's font and the canvas layer's `_getFont`.
      expect(fonts).toHaveLength(2)
      for (const font of fonts) expect(font, font).toMatch(/\.trim\(\)$/)
    })
  }
})
