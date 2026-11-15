import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import type { BrowserLayerMenu, BrowserLayerReport, BrowserLayerShow, EventPayload } from '../../../shared/ipc'
import { LayerRoot } from './LayerRoot'

/**
 * The menu layer's page: it says it is listening, draws what main sends with
 * the window's look, and reports a pick or a close on the turn it happened in.
 * Main is played by the test.
 */

function fakeMain(): {
  bridge: Parameters<typeof LayerRoot>[0]['bridge']
  reports: BrowserLayerReport[]
  show: (next: BrowserLayerShow) => void
} {
  const reports: BrowserLayerReport[] = []
  let listener: ((payload: BrowserLayerShow) => void) | null = null
  const bridge = {
    on: ((channel: string, next: (payload: EventPayload<'browser:layer-show'>) => void) => {
      if (channel !== 'browser:layer-show') throw new Error(`the layer listens to nothing else: ${channel}`)
      listener = next
      return () => {
        listener = null
      }
    }) as Parameters<typeof LayerRoot>[0]['bridge']['on'],
    send: ((channel: string, payload: BrowserLayerReport) => {
      if (channel !== 'browser:layer-report') throw new Error(`the layer sends nothing else: ${channel}`)
      reports.push(payload)
    }) as Parameters<typeof LayerRoot>[0]['bridge']['send']
  }
  return {
    bridge,
    reports,
    show: (next) => act(() => listener?.(next))
  }
}

const MENU: BrowserLayerMenu = {
  kind: 'menu',
  label: 'Browser',
  role: 'menu',
  stayOpen: false,
  entries: [
    { kind: 'heading', id: 'zoom-heading', label: 'Zoom 100%' },
    { kind: 'item', id: 'zoom-in', label: 'Zoom in', hint: 'Ctrl =' },
    { kind: 'item', id: 'zoom-out', label: 'Zoom out', disabled: true },
    { kind: 'item', id: 'devtools', label: 'Open DevTools', hint: 'F12' }
  ]
}
const SHOW: NonNullable<BrowserLayerShow> = {
  turn: 4,
  content: MENU,
  look: { vars: { '--helm-surface-raised': '#202124', '--helm-radius': '6px' }, kind: 'dark', data: { theme: 'night', density: 'compact' } }
}

afterEach(() => {
  const root = document.documentElement
  root.removeAttribute('style')
  root.className = ''
  delete root.dataset['theme']
  delete root.dataset['density']
})

describe('LayerRoot', () => {
  it('says it is listening, and draws nothing until it is told to', () => {
    const main = fakeMain()
    render(<LayerRoot bridge={main.bridge} />)
    expect(main.reports).toEqual([{ ready: true }])
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it("draws the menu in the window's look and takes the keyboard", () => {
    const main = fakeMain()
    render(<LayerRoot bridge={main.bridge} />)
    main.show(SHOW)
    const menu = screen.getByRole('menu', { name: 'Browser' })
    expect(document.activeElement).toBe(menu)
    const root = document.documentElement
    expect(root.style.getPropertyValue('--helm-surface-raised')).toBe('#202124')
    expect(root.style.getPropertyValue('--helm-radius')).toBe('6px')
    expect(root.classList.contains('dark')).toBe(true)
    expect(root.dataset['density']).toBe('compact')
  })

  it('reports the row the keys chose, then the close it makes, skipping a row that is disabled', () => {
    const main = fakeMain()
    render(<LayerRoot bridge={main.bridge} />)
    main.show(SHOW)
    const menu = screen.getByRole('menu')
    fireEvent.keyDown(menu, { key: 'ArrowDown' })
    fireEvent.keyDown(menu, { key: 'Enter' })
    expect(main.reports.slice(1)).toEqual([
      { turn: 4, pick: 'devtools' },
      { turn: 4, closed: 'pick' }
    ])
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('reports a click on a row', () => {
    const main = fakeMain()
    render(<LayerRoot bridge={main.bridge} />)
    main.show(SHOW)
    fireEvent.click(screen.getByRole('menuitem', { name: /Zoom in/ }))
    expect(main.reports.slice(1)).toEqual([
      { turn: 4, pick: 'zoom-in' },
      { turn: 4, closed: 'pick' }
    ])
  })

  it('reports Escape as a close', () => {
    const main = fakeMain()
    render(<LayerRoot bridge={main.bridge} />)
    main.show(SHOW)
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' })
    expect(main.reports.slice(1)).toEqual([{ turn: 4, closed: 'escape' }])
  })

  it('clears when main hides it, so the next showing starts from nothing', () => {
    const main = fakeMain()
    render(<LayerRoot bridge={main.bridge} />)
    main.show(SHOW)
    main.show(null)
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('starts a new menu on its first row, and keeps the row for new content on the same turn', () => {
    const main = fakeMain()
    render(<LayerRoot bridge={main.bridge} />)
    main.show(SHOW)
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'ArrowDown' })
    main.show({ ...SHOW, content: { ...MENU, label: 'Browser' } })
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Enter' })
    expect(main.reports.at(-2)).toEqual({ turn: 4, pick: 'devtools' })

    main.show({ ...SHOW, turn: 5 })
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Enter' })
    expect(main.reports.at(-2)).toEqual({ turn: 5, pick: 'zoom-in' })
  })
})

describe('LayerRoot - the address suggestions', () => {
  const SUGGESTIONS: NonNullable<BrowserLayerShow> = {
    turn: 7,
    content: {
      kind: 'suggestions',
      rows: [
        { kind: 'search', text: 'docs', engine: 'Google' },
        { kind: 'page', url: 'http://localhost:3000/docs', title: 'Docs' }
      ],
      highlighted: 1
    },
    look: SHOW.look
  }

  it('draws the rows with the highlight it is sent, hidden from assistive technology, and never takes the keyboard', () => {
    const main = fakeMain()
    const before = document.activeElement
    render(<LayerRoot bridge={main.bridge} />)
    main.show(SUGGESTIONS)
    // The window's copy of the list is the one announced.
    expect(screen.queryAllByRole('option')).toEqual([])
    const options = screen.getAllByRole('option', { hidden: true })
    expect(options.map((option) => option.getAttribute('aria-selected'))).toEqual(['false', 'true'])
    expect(document.activeElement).toBe(before)
  })

  it("reports the pointer's pick on the press, and its hover, by row", () => {
    const main = fakeMain()
    render(<LayerRoot bridge={main.bridge} />)
    main.show(SUGGESTIONS)
    const [search] = screen.getAllByRole('option', { hidden: true })
    fireEvent.mouseMove(search!)
    fireEvent.mouseDown(search!, { button: 0 })
    expect(main.reports.slice(1)).toEqual([
      { turn: 7, hover: 0 },
      { turn: 7, row: 0 }
    ])
  })
})
