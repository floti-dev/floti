import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { HUE_TINT_ALPHA, PALETTE_HUES } from '@floti/core/types'

/**
 * The primitives stylesheet a plugin page is served (`/__floti/floti.css`), read
 * as text: what it declares for the categorical palette. jsdom has no cascade
 * layers or `color-mix`, so the rules are checked as written; the shades they
 * reach are the theme's, which `bridge.test.ts` shows arrive on `<html>`.
 */
const css = readFileSync(join(import.meta.dirname, 'floti.css'), 'utf8')

/** The declarations of every rule with exactly this selector, together. */
function rule(selector: string): string {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const found = [...css.matchAll(new RegExp(`(?:^|\\n)\\s*${escaped}\\s*\\{([^}]*)\\}`, 'g'))]
  if (found.length === 0) throw new Error(`floti.css has no rule for ${selector}`)
  return found.map((match) => match[1] ?? '').join('\n')
}

describe('floti.css: a tag, chip or dot marked with a palette hue by name', () => {
  it.each(PALETTE_HUES.map((hue) => [hue] as const))('data-hue="%s" draws in that hue’s theme token', (hue) => {
    expect(rule(`[data-hue='${hue}']`)).toMatch(new RegExp(`--floti-hue:\\s*var\\(--floti-hue-${hue}\\);`))
  })

  it('names the hues Trackr stores a project colour as', () => {
    for (const hue of ['blue', 'orange', 'green', 'amber', 'pink']) expect(() => rule(`[data-hue='${hue}']`)).not.toThrow()
  })

  it('gives a chip its hue as text and a dot its hue as a fill', () => {
    expect(rule(':is(.floti-chip, .floti-dot)[data-hue]')).toMatch(/--floti-tone:\s*var\(--floti-hue\);/)
  })

  it('draws a palette tag as the hue’s tint behind the hue’s text, with no border', () => {
    const tag = rule('.floti-tag[data-hue]')
    expect(tag).toMatch(/border-color:\s*transparent;/)
    expect(tag).toMatch(/background:\s*color-mix\(in srgb, var\(--floti-hue\) var\(--floti-hue-tint\), transparent\);/)
    expect(tag).toMatch(/color:\s*var\(--floti-hue\);/)
  })

  it('mixes the tint at the weights the palette’s floors are measured at', () => {
    const weight = (selector: string): number => {
      const found = /--floti-hue-tint:\s*(\d+)%;/.exec(rule(selector))
      if (found === null) throw new Error(`${selector} sets no tint`)
      return Number(found[1]) / 100
    }
    expect(weight(':root')).toBe(HUE_TINT_ALPHA.light)
    expect(weight(':root.dark')).toBe(HUE_TINT_ALPHA.dark)
  })
})
