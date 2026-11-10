// @vitest-environment jsdom
import { render } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

type Hooks = Record<string, (...args: never[]) => unknown>

interface FakeTerm {
  options: Record<string, unknown>
  write: ReturnType<typeof vi.fn>
  /** What the screen holds, row by row, as the test sets it. */
  lines: string[]
  buffer: { active: { viewportY: number; cursorY: number; getLine: (y: number) => { translateToString: () => string } } }
}

const made = vi.hoisted(() => ({
  options: [] as unknown[],
  hooks: [] as unknown[],
  prefs: [] as unknown[],
  terminals: [] as Array<{ term: FakeTerm; dispose: () => void }>
}))

vi.mock('../terminal', () => ({
  TERMINAL_DEFAULTS: { fontFamily: 'mono', fontSize: 14, cursorStyle: 'block', cursorBlink: true, scrollback: 10000 },
  terminalFontStack: (family: string | null) => family ?? 'mono',
  createTerminal: (_container: HTMLElement, opts: { rows: number }, hooks: unknown, prefs: unknown) => {
    const lines = Array.from({ length: opts.rows }, () => '')
    const term: FakeTerm = {
      options: { ...(prefs as Record<string, unknown>) },
      // The terminal has drawn a chunk once it calls back, as xterm does after parsing it.
      write: vi.fn((_text: string, done?: () => void) => done?.()),
      lines,
      buffer: {
        active: {
          viewportY: 0,
          cursorY: 0,
          getLine: (y) => ({ translateToString: () => lines[y] ?? '' })
        }
      }
    }
    const host = { term, dispose: vi.fn() }
    made.options.push(opts)
    made.hooks.push(hooks)
    made.prefs.push(prefs)
    made.terminals.push(host)
    return host
  }
}))
vi.mock('./bridge', () => ({ helm: { invoke: vi.fn() } }))

import { PeekTerminal, peekHiddenRows } from './PeekTerminal'

/** A monospace font whose cell is 0.6 of its size wide and 1.2 of it high, as the canvas measures it. */
const cellAt = (fontSize: number): { width: number; height: number } => ({ width: 0.6 * fontSize, height: 1.2 * fontSize })

class FakeCanvas {
  getContext(): { font: string; measureText: (text: string) => TextMetrics } {
    const context = {
      font: '',
      measureText: (): TextMetrics => {
        const size = Number.parseFloat(context.font)
        const cell = cellAt(size)
        return {
          width: cell.width,
          fontBoundingBoxAscent: cell.height * 0.75,
          fontBoundingBoxDescent: cell.height * 0.25
        } as TextMetrics
      }
    }
    return context
  }
}

const GRID = { cols: 90, rows: 20 }
/** The room a 470px peek's screen has, with a window tall enough for the largest cap. */
const ROOM = { width: 424, maxHeight: 340 }

beforeEach(() => {
  vi.stubGlobal('OffscreenCanvas', FakeCanvas)
})

afterEach(() => {
  vi.unstubAllGlobals()
  made.options.length = 0
  made.hooks.length = 0
  made.prefs.length = 0
  made.terminals.length = 0
})

/** A screen whose every row has something on it, the way Claude Code fills its grid. */
const full = (rows: number, cols: number): string[] => Array.from({ length: rows }, (_, y) => `${String(y)}`.padEnd(cols, 'x'))

/** Renders the peek's terminal and draws `screen` into it as the child's first chunk. */
function peekWith(grid: { cols: number; rows: number }, screen: string[], room = ROOM) {
  let sink: (text: string) => void = () => undefined
  const attach = (next: (text: string) => void): (() => void) => {
    sink = next
    return () => undefined
  }
  const view = render(<PeekTerminal attach={attach} grid={grid} room={room} windowsBuild={null} />)
  const host = made.terminals.at(-1)!
  host.term.lines.splice(0, screen.length, ...screen)
  host.term.buffer.active.cursorY = Math.max(0, screen.length - 1)
  sink('the screen')
  const box = view.container.querySelector<HTMLElement>('[data-peek-terminal]')!
  return { view, host, box, attach, sink: (text: string) => sink(text) }
}

describe('the peek terminal', () => {
  it('S1: is made at the grid the child has, at most 11px in the terminal’s own line height, never fits its grid to its box, takes no keyboard and no input', () => {
    render(<PeekTerminal attach={() => () => undefined} grid={GRID} room={ROOM} windowsBuild={null} />)
    expect(made.options[0]).toMatchObject({ ...GRID, fit: false, focus: false })
    expect(made.prefs[0]).toMatchObject({ fontSize: 11, scrollback: 0 })
    const options = made.terminals[0]?.term.options
    expect(options).toMatchObject({ disableStdin: true, cursorBlink: false })
    expect(options?.lineHeight).toBeUndefined()
    expect(Number(options?.fontSize)).toBeLessThanOrEqual(11)
  })

  it('S4: whatever the terminal is given to send goes nowhere: no keys and no size', () => {
    render(<PeekTerminal attach={() => () => undefined} grid={GRID} room={ROOM} windowsBuild={null} />)
    const hooks = made.hooks[0] as Hooks
    expect(hooks.onInput?.('echo hi' as never)).toBeUndefined()
    expect(hooks.onResize?.(10 as never, 5 as never)).toBeUndefined()
  })

  it('takes no clipboard write from the program it shows: a peek leaves this client as it is', () => {
    render(<PeekTerminal attach={() => () => undefined} grid={GRID} room={ROOM} windowsBuild={null} />)
    expect(made.options[0]).not.toHaveProperty('programClipboard')
  })

  it('S1: draws what the peek kept and then each chunk as it arrives, and stops with the view', () => {
    let sink: (text: string) => void = () => undefined
    const detach = vi.fn()
    const view = render(
      <PeekTerminal
        attach={(next) => {
          sink = next
          next('screen so far')
          return detach
        }}
        grid={GRID}
        room={ROOM}
        windowsBuild={null}
      />
    )
    const host = made.terminals[0]
    expect(host?.term.write).toHaveBeenCalledWith('screen so far', expect.any(Function))
    sink('more')
    expect(host?.term.write).toHaveBeenCalledWith('more', expect.any(Function))
    view.unmount()
    expect(detach).toHaveBeenCalledTimes(1)
    expect(host?.dispose).toHaveBeenCalledTimes(1)
  })

  it('draws a grid wider and taller than its box whole: every column across the box, and every row under its cap', () => {
    const grid = { cols: 200, rows: 60 }
    const { host, box } = peekWith(grid, full(60, 200))
    // Still the child's grid, never fitted to the box.
    expect(made.options[0]).toMatchObject({ cols: 200, rows: 60, fit: false })
    const fontSize = Number(host.term.options.fontSize)
    expect(fontSize).toBeLessThan(11)
    // Every column fits the box's width ...
    expect(grid.cols * cellAt(fontSize).width).toBeLessThanOrEqual(ROOM.width)
    // ... and every row is in the box, whose height is the rows', under the cap.
    const rowHeight = Math.ceil(cellAt(fontSize).height)
    expect(box.style.height).toBe(`${String(60 * rowHeight)}px`)
    expect(60 * rowHeight).toBeLessThanOrEqual(ROOM.maxHeight)
    // Drawn from its top: no row is pushed out above the box.
    const drawn = box.firstElementChild as HTMLElement
    expect(drawn.className).toContain('top-0')
    expect(drawn.style.bottom).toBe('')
  })

  it('draws a grid that already fits at 11px, in a box as high as its rows', () => {
    const { host, box } = peekWith({ cols: 50, rows: 10 }, full(10, 50))
    expect(host.term.options.fontSize).toBe(11)
    expect(box.style.height).toBe(`${String(10 * Math.ceil(cellAt(11).height))}px`)
  })

  it('S4: ends a shell child’s box on its last row with anything on it', () => {
    const { host, box } = peekWith({ cols: 120, rows: 32 }, ['PS C:\\work> ls', 'a  b  c', 'PS C:\\work> '])
    const rowHeight = Math.ceil(cellAt(Number(host.term.options.fontSize)).height)
    expect(box.style.height).toBe(`${String(3 * rowHeight)}px`)
  })

  it('fits again when the box’s room changes, and when the host says a new grid', () => {
    const grid = { cols: 120, rows: 32 }
    const { view, host, attach } = peekWith(grid, full(32, 120))
    const before = Number(host.term.options.fontSize)

    view.rerender(<PeekTerminal attach={attach} grid={grid} room={{ width: 300, maxHeight: 340 }} windowsBuild={null} />)
    const narrower = Number(host.term.options.fontSize)
    expect(narrower).toBeLessThan(before)
    expect(120 * cellAt(narrower).width).toBeLessThanOrEqual(300)
    // A new room is the same terminal: the child is not drawn again from scratch.
    expect(made.terminals).toHaveLength(1)

    view.rerender(<PeekTerminal attach={attach} grid={{ cols: 240, rows: 32 }} room={{ width: 300, maxHeight: 340 }} windowsBuild={null} />)
    expect(made.terminals).toHaveLength(2)
    expect(made.options[1]).toMatchObject({ cols: 240, rows: 32, fit: false })
    const wider = made.terminals[1]!
    expect(240 * cellAt(Number(wider.term.options.fontSize)).width).toBeLessThanOrEqual(300)
  })
})

describe('where the peek ends the screen of a child', () => {
  const blank = (rows: number): string[] => Array.from({ length: rows }, () => '')

  it('leaves out the empty rows under what a program drew from the top of its grid', () => {
    const lines = blank(32)
    lines[0] = 'Claude Code v2'
    lines[3] = '> hello'
    expect(peekHiddenRows(lines, 3)).toBe(28)
  })

  it('keeps the rows down to the cursor even when it sits on an empty row', () => {
    const lines = blank(32)
    lines[0] = 'ready'
    expect(peekHiddenRows(lines, 5)).toBe(26)
  })

  it('leaves nothing out of a screen drawn to its last row', () => {
    const lines = blank(32)
    lines[31] = 'x'.repeat(100)
    expect(peekHiddenRows(lines, 31)).toBe(0)
  })
})
