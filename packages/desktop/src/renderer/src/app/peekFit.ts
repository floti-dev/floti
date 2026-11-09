import type { PeekScreenRoom } from '@helm/ui'
import type { PeekGrid } from './peek'

/**
 * How a child's screen fits the peek: its whole grid, scaled down to the room
 * the screen box has.
 *
 * The peek draws every column of the child's grid and every row down to the
 * last with anything on it, at the largest type size that fits the box's width
 * and its height cap, and never larger than {@link PEEK_FONT_SIZE}. It is drawn
 * smaller rather than stretched: the terminal is given the smaller size and
 * draws its glyphs at it, so the text stays sharp at any scale. The grid stays
 * the child's, so nothing wraps differently than it does for the child.
 *
 * The terminal sizes its cells in the screen's device pixels, and the fit
 * reckons them the same way: a row is its character's height rounded up to a
 * whole device pixel, which both of xterm's renderers do, and a column is at
 * most its character's width, which the WebGL renderer rounds down and the DOM
 * renderer keeps. So the width fits when the characters' own widths do, and
 * the height fits when the rows do in whole device pixels. The line height is
 * the terminal's own, 1, so a row is a character high.
 */

/** The largest size the peek's screen is drawn at, in px: a grid that fits at it is not drawn larger. */
export const PEEK_FONT_SIZE = 11

/** The smallest, in px: a grid too large for even this is drawn at it. */
const SMALLEST_FONT_SIZE = 1

/** One character cell at a type size, in CSS px, unrounded, as the terminal measures it. */
export interface PeekCell {
  width: number
  height: number
}

export interface PeekFitInput {
  /** The child's grid, which the screen is drawn at. */
  grid: PeekGrid
  /** How many of its rows, from the top, the screen shows: down to the last with anything on it. */
  shownRows: number
  /** The cell at a type size, measured as the terminal measures it. */
  cellAt: (fontSize: number) => PeekCell
  /** The room the screen has inside its box. */
  room: PeekScreenRoom
  /** The screen's device pixels to a CSS px. */
  dpr: number
}

export interface PeekFit {
  /** The type size the grid is drawn at, in px. */
  fontSize: number
  /** That size against {@link PEEK_FONT_SIZE}: 1 for a grid that fits at it, less for one scaled down. */
  scale: number
  /** How tall a drawn row is, in CSS px. */
  rowHeight: number
  /** The screen's height: the shown rows, whole, in CSS px. */
  height: number
}

/**
 * The type size the shown rows of `grid` fit `room` at, and the height they
 * take there. The size is the one where the widths and the device-pixel rows
 * fit; where the measured cell does not grow in step with the size, it is
 * brought down until they do.
 */
export function fitPeekScreen({ grid, shownRows, cellAt, room, dpr }: PeekFitInput): PeekFit {
  const cols = Math.max(1, grid.cols)
  const rows = Math.min(Math.max(1, Math.round(shownRows)), Math.max(1, grid.rows))
  const ratio = dpr > 0 ? dpr : 1
  // The most device pixels a row may take for the rows to fit the cap.
  const rowPixels = Math.max(1, Math.floor((Math.max(0, room.maxHeight) * ratio) / rows))
  const measure = (fontSize: number): { width: number; rowPixels: number } => {
    const cell = cellAt(fontSize)
    return { width: cols * cell.width, rowPixels: Math.ceil(cell.height * ratio) }
  }
  const fits = (size: { width: number; rowPixels: number }): boolean =>
    size.width <= room.width && size.rowPixels <= rowPixels

  let fontSize = PEEK_FONT_SIZE
  let size = measure(fontSize)
  for (let step = 0; !fits(size) && fontSize > SMALLEST_FONT_SIZE && step < 32; step++) {
    const cell = cellAt(fontSize)
    // Where the cell grows in step with the size, this is the size that fits;
    // the margin keeps a rounding error from landing a row on the pixel above.
    const byWidth = cell.width > 0 ? (room.width - 0.001) / (cols * cell.width) : 1
    const byHeight = cell.height > 0 ? (rowPixels - 0.001) / (cell.height * ratio) : 1
    // Every step lowers the size, so it ends where the measured cell fits.
    const next = fontSize * Math.min(byWidth, byHeight, 0.995)
    fontSize = Math.max(SMALLEST_FONT_SIZE, next)
    size = measure(fontSize)
  }
  const rowHeight = size.rowPixels / ratio
  return { fontSize, scale: fontSize / PEEK_FONT_SIZE, rowHeight, height: rows * rowHeight }
}
