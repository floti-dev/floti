import { describe, expect, it } from 'vitest'
import { PEEK_FONT_SIZE, fitPeekScreen, type PeekCell } from './peekFit'

/** A monospace font whose cell grows in step with its size: 0.6 of it wide, 1.2 of it high. */
const linear = (fontSize: number): PeekCell => ({ width: 0.6 * fontSize, height: 1.2 * fontSize })

/** The room a 470px peek's screen has: its inner width, and the cap less its edges. */
const ROOM = { width: 424, maxHeight: 340 }

describe('fitting a child’s grid to the peek', () => {
  it('draws a grid that already fits at the largest size, and the box is as tall as its rows', () => {
    const fit = fitPeekScreen({ grid: { cols: 40, rows: 8 }, shownRows: 8, cellAt: linear, room: ROOM, dpr: 1 })
    expect(fit.fontSize).toBe(PEEK_FONT_SIZE)
    expect(fit.scale).toBe(1)
    // 13.2px rounds up to 14 device pixels.
    expect(fit.rowHeight).toBe(14)
    expect(fit.height).toBe(8 * 14)
  })

  it('scales a grid wider than the box down until every column fits its width', () => {
    const grid = { cols: 120, rows: 32 }
    const fit = fitPeekScreen({ grid, shownRows: 32, cellAt: linear, room: ROOM, dpr: 1.25 })
    expect(fit.scale).toBeCloseTo(424 / (120 * 6.6), 4)
    expect(fit.fontSize).toBeCloseTo(PEEK_FONT_SIZE * fit.scale, 9)
    expect(grid.cols * linear(fit.fontSize).width).toBeLessThanOrEqual(ROOM.width)
    // Each row is its height rounded up to whole device pixels: 8.83 of them make 9.
    expect(fit.rowHeight).toBeCloseTo(9 / 1.25, 9)
    expect(fit.height).toBeCloseTo(32 * (9 / 1.25), 9)
    expect(fit.height).toBeLessThanOrEqual(ROOM.maxHeight)
  })

  it('scales a grid taller than the cap down further, until all of its rows fit under it', () => {
    const grid = { cols: 80, rows: 100 }
    const fit = fitPeekScreen({ grid, shownRows: 100, cellAt: linear, room: ROOM, dpr: 1 })
    // At the width alone it would be 0.8 of the largest size, and 100 rows of that pass the cap.
    expect(fit.scale).toBeLessThan((424 / (80 * 6.6)) * 0.5)
    // 340px holds 100 rows of 3 device pixels.
    expect(fit.rowHeight).toBe(3)
    expect(fit.height).toBe(300)
    expect(fit.height).toBeLessThanOrEqual(ROOM.maxHeight)
    expect(grid.cols * linear(fit.fontSize).width).toBeLessThanOrEqual(ROOM.width)
  })

  it('fits only the shown rows, so a shell that drew three rows has a box three rows high', () => {
    const fit = fitPeekScreen({ grid: { cols: 120, rows: 32 }, shownRows: 3, cellAt: linear, room: ROOM, dpr: 1 })
    expect(fit.scale).toBeCloseTo(424 / (120 * 6.6), 4)
    expect(fit.height).toBe(3 * fit.rowHeight)
  })

  it('counts rows in whole device pixels, so 60 rows fit a 340px cap at 11 device pixels each on a 2x screen', () => {
    const fit = fitPeekScreen({ grid: { cols: 40, rows: 60 }, shownRows: 60, cellAt: linear, room: ROOM, dpr: 2 })
    // 340px at 2 device pixels a px is 680 of them: 11 a row for 60 rows.
    expect(Math.ceil(linear(fit.fontSize).height * 2)).toBeLessThanOrEqual(11)
    expect(fit.height).toBeCloseTo((60 * 11) / 2, 9)
  })

  it('brings the size down until the measured cell fits where the font’s metrics are rounded and do not grow in step', () => {
    // Heights rounded to whole px, and widths that run a little wide at small sizes.
    const rounded = (fontSize: number): PeekCell => ({
      width: 0.6 * fontSize + (fontSize < 6 ? 0.08 : 0),
      height: Math.round(1.25 * fontSize)
    })
    const grid = { cols: 120, rows: 40 }
    const fit = fitPeekScreen({ grid, shownRows: 40, cellAt: rounded, room: ROOM, dpr: 1 })
    const cell = rounded(fit.fontSize)
    expect(grid.cols * cell.width).toBeLessThanOrEqual(ROOM.width)
    expect(40 * Math.ceil(cell.height)).toBeLessThanOrEqual(ROOM.maxHeight)
    expect(fit.height).toBe(40 * Math.ceil(cell.height))
  })

  it('never draws above the largest size, however much room there is', () => {
    const fit = fitPeekScreen({
      grid: { cols: 10, rows: 2 },
      shownRows: 2,
      cellAt: linear,
      room: { width: 2000, maxHeight: 2000 },
      dpr: 1
    })
    expect(fit.fontSize).toBe(PEEK_FONT_SIZE)
  })
})
