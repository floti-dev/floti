import type { PeekScreenRoom } from '@helm/ui'
import type { JSX } from 'react'
import { useLayoutEffect, useRef } from 'react'
import { createTerminal } from '../terminal'
import { helm } from './bridge'
import type { PeekGrid } from './peek'
import { PEEK_FONT_SIZE, fitPeekScreen, type PeekCell } from './peekFit'
import { terminalPrefs } from './termprefs'

const HIDE_CURSOR = '\x1b[?25l'

/**
 * How many rows under the last one with something on it (or the cursor) the
 * peek leaves out, so the box ends on what the child drew.
 *
 * A program that draws from the top, a shell for one, leaves the bottom of its
 * grid empty, so a view of the whole grid would show that emptiness under it.
 */
export function peekHiddenRows(lines: readonly string[], cursorRow: number): number {
  let last = Math.min(Math.max(cursorRow, 0), lines.length - 1)
  for (let y = lines.length - 1; y > last; y--) {
    if ((lines[y] ?? '').trim() !== '') {
      last = y
      break
    }
  }
  return Math.max(0, lines.length - 1 - last)
}

/**
 * One character cell of `fontFamily` at a size, measured as xterm measures it
 * (`TextMetricsMeasureStrategy`): the advance of a W and the font's ascent and
 * descent together, from a canvas's text metrics, so the fit reckons with the
 * numbers the terminal draws by. Null where there is no canvas to measure with.
 */
export function cellMeasure(fontFamily: string): ((fontSize: number) => PeekCell) | null {
  if (typeof OffscreenCanvas === 'undefined') return null
  const context = new OffscreenCanvas(1, 1).getContext('2d')
  if (context === null) return null
  return (fontSize) => {
    context.font = `${String(fontSize)}px ${fontFamily}`
    const metrics = context.measureText('W')
    return { width: metrics.width, height: metrics.fontBoundingBoxAscent + metrics.fontBoundingBoxDescent }
  }
}

export interface PeekTerminalProps {
  /** Draws the peek's output into `sink`: what has arrived, then each chunk. Returns the way to stop (`PeekStore.attach`). */
  attach: (sink: (text: string) => void) => () => void
  /** The child's own grid, which the screen is drawn at so nothing wraps differently than it does for the child. */
  grid: PeekGrid
  /** The room the peek's screen box has (`ChildPeek`). */
  room: PeekScreenRoom
  windowsBuild: number | null
}

/**
 * A child's screen in the peek: read-only, its whole grid drawn at the child's
 * own cols and rows, scaled down to fit the room the box has. Every column is
 * drawn, and every row down to the last with anything on it, at the largest
 * type size up to 11px at which they fit the box's width and its height cap
 * (`fitPeekScreen`); the box is as high as those rows. The size is the
 * terminal's own, so its glyphs are drawn at it and stay sharp. It fits again
 * whenever a chunk changes how many rows have anything on them, when the room
 * changes, and at a new grid, which makes a new terminal.
 *
 * It has no way to type or to size the child. Input is off, nothing it is given
 * goes anywhere, it never fits its grid to its box (the grid is the child's),
 * and it is never given the keyboard. The only thing it reaches outside itself
 * is the clipboard, to copy text the user selected.
 */
export function PeekTerminal({ attach, grid, room, windowsBuild }: PeekTerminalProps): JSX.Element {
  const { cols, rows } = grid
  const boxRef = useRef<HTMLDivElement>(null)
  const termRef = useRef<HTMLDivElement>(null)
  const roomRef = useRef(room)
  const refitRef = useRef<(() => void) | null>(null)

  // The room first, so a terminal made in the same commit fits the room it has now.
  useLayoutEffect(() => {
    roomRef.current = { width: room.width, maxHeight: room.maxHeight }
    refitRef.current?.()
  }, [room.width, room.maxHeight])

  useLayoutEffect(() => {
    const box = boxRef.current
    const wrap = termRef.current
    if (box === null || wrap === null) return
    const element = document.createElement('div')
    wrap.appendChild(element)
    const prefs = terminalPrefs()
    const host = createTerminal(
      element,
      {
        cols,
        rows,
        fit: false,
        focus: false,
        ...(windowsBuild !== null ? { windowsBuild } : {})
      },
      {
        onInput: () => undefined,
        onResize: () => undefined,
        readClipboard: () => Promise.resolve({ refused: true }),
        writeClipboard: async (text) => (await helm.invoke('clipboard:write', text)).outcome,
        openLink: () => undefined
      },
      // The screen only: what scrolled off is not this view's to keep.
      { ...prefs, fontSize: PEEK_FONT_SIZE, scrollback: 0 }
    )
    host.term.options.disableStdin = true
    host.term.options.cursorBlink = false
    // Nothing is typed here, so no cursor is drawn: the program's own cursor
    // position is not a place the user can act.
    host.term.options.cursorInactiveStyle = 'none'

    const cellAt = cellMeasure(prefs.fontFamily)
    // The rows the screen shows: the whole grid until what the peek kept has
    // been drawn, so a full screen arriving does not first show a sliver.
    let shownRows = rows
    const fit = (): void => {
      if (cellAt === null) {
        // Nothing to measure with: the screen at the largest size, as high as the cap.
        box.style.height = `${String(roomRef.current.maxHeight)}px`
        return
      }
      const next = fitPeekScreen({
        grid: { cols, rows },
        shownRows,
        cellAt,
        room: roomRef.current,
        dpr: window.devicePixelRatio > 0 ? window.devicePixelRatio : 1
      })
      if (host.term.options.fontSize !== next.fontSize) host.term.options.fontSize = next.fontSize
      box.style.height = `${String(next.height)}px`
    }
    // The box ends on the last row that has anything on it.
    const settle = (): void => {
      const buffer = host.term.buffer.active
      const lines: string[] = []
      for (let y = buffer.viewportY; y < buffer.viewportY + rows; y++) {
        lines.push(buffer.getLine(y)?.translateToString(true) ?? '')
      }
      shownRows = rows - peekHiddenRows(lines, buffer.cursorY)
      fit()
    }
    refitRef.current = fit

    // Hidden again after every chunk, because the program shows its cursor
    // whenever it likes and this view has none to show.
    let written = false
    const detach = attach((text) => {
      written = true
      host.term.write(text, () => {
        host.term.write(HIDE_CURSOR, settle)
      })
    })
    if (written) fit()
    else settle()
    return () => {
      refitRef.current = null
      detach()
      host.dispose()
      element.remove()
    }
  }, [attach, cols, rows, windowsBuild])

  return (
    <div ref={boxRef} className="relative w-full overflow-hidden" data-peek-terminal>
      <div ref={termRef} className="absolute top-0 left-0" />
    </div>
  )
}
