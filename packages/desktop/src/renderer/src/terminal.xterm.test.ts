import { afterEach, describe, expect, it, vi } from 'vitest'
import { Terminal } from '@xterm/xterm'

/**
 * The real xterm, opened in jsdom, for the parts of it Helm reaches that have
 * no public handle. `keepScrollbarInStep` in `terminal.ts` sizes the scrollbar
 * through xterm's own viewport and renderer, and leaves a terminal alone when
 * they are not where it looks. An xterm that moved them would bring the dead
 * scrollbar back with nothing failing, so this is what fails instead.
 */

const opened: Terminal[] = []

afterEach(() => {
  for (const term of opened.splice(0)) term.dispose()
  document.body.replaceChildren()
  vi.unstubAllGlobals()
})

describe('the xterm Floti builds on', () => {
  it('keeps its viewport and its renderer’s screen size where the scrollbar is kept in step through them', () => {
    // jsdom has no media queries; xterm asks one for the pixel ratio.
    vi.stubGlobal('matchMedia', (query: string) => ({
      matches: false,
      media: query,
      addListener: () => undefined,
      removeListener: () => undefined,
      addEventListener: () => undefined,
      removeEventListener: () => undefined
    }))
    const term = new Terminal({ cols: 80, rows: 24 })
    opened.push(term)
    const box = document.createElement('div')
    document.body.appendChild(box)
    term.open(box)

    const core = (term as unknown as {
      _core?: {
        _viewport?: { queueSync?: unknown }
        _renderService?: { dimensions?: { css?: { canvas?: { height?: unknown } } } }
      }
    })._core
    expect(typeof core?._viewport?.queueSync).toBe('function')
    expect(typeof core?._renderService?.dimensions?.css?.canvas?.height).toBe('number')
    expect(typeof term.onRender).toBe('function')
  })
})
