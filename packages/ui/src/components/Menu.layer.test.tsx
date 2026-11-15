import { createRef, useRef, type JSX } from 'react'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { overlayOpen } from '../lib/overlay'
import { PANES_MOVED_EVENT } from '../lib/paneGeometry'
import { placePopup, POPUP_MARGIN } from '../lib/popup'
import {
  Menu,
  type LayerMenu,
  type LayerMenuClosed,
  type LayerRect,
  type MenuEntry,
  type MenuLayerPort
} from './Menu'

/**
 * A menu drawn in the browser's menu layer: the window measures and places it,
 * hands the layer data and a rectangle, and acts on what the layer says.
 * The layer itself is played by the test (`port`).
 */

const ENTRIES: MenuEntry[] = [
  { kind: 'heading', id: 'zoom-heading', label: 'Zoom 100%' },
  { kind: 'item', id: 'zoom-in', label: 'Zoom in', hint: 'Ctrl =', icon: <svg data-icon /> },
  { kind: 'separator', id: 'after-zoom' },
  { kind: 'item', id: 'devtools', label: 'Open DevTools', hint: 'F12', checked: false }
]

/** What the measured copy says its size is; jsdom does no layout. */
const SIZE = { width: 220, height: 300 }
const WINDOW = { width: 1280, height: 800 }

interface Opened {
  menu: LayerMenu
  rect: LayerRect
  on: { pick: (id: string) => void; closed: (why: LayerMenuClosed) => void }
  updates: Array<{ menu: LayerMenu; rect: LayerRect }>
  closed: number
}

function fakePort(): { port: MenuLayerPort; opened: Opened[] } {
  const opened: Opened[] = []
  return {
    opened,
    port: {
      open: (menu, rect, on) => {
        const record: Opened = { menu, rect, on, updates: [], closed: 0 }
        opened.push(record)
        return {
          update: (next, at) => record.updates.push({ menu: next, rect: at }),
          close: () => {
            record.closed += 1
          }
        }
      }
    }
  }
}

beforeEach(() => {
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const measured = this.closest('[data-menu-measure]') !== null && this.hasAttribute('data-menu')
    return DOMRect.fromRect(measured ? SIZE : { x: 0, y: 0, width: 0, height: 0 })
  })
  Object.defineProperty(window, 'innerWidth', { configurable: true, value: WINDOW.width })
  Object.defineProperty(window, 'innerHeight', { configurable: true, value: WINDOW.height })
})
afterEach(() => vi.restoreAllMocks())

const BELOW = DOMRect.fromRect({ x: 1100, y: 40, width: 28, height: 28 })

/** A More button and its menu, as the browser bar draws them. */
function Bar({
  port,
  open,
  onSelect,
  onDismiss
}: {
  port: MenuLayerPort
  open: boolean
  onSelect: (id: string) => void
  onDismiss: () => void
}): JSX.Element {
  const anchor = useRef<HTMLButtonElement>(null)
  return (
    <>
      <button ref={anchor} type="button">
        More
      </button>
      <button type="button">Elsewhere</button>
      {open && (
        <Menu
          label="Browser"
          entries={ENTRIES}
          at={{ below: BELOW }}
          anchorRef={anchor}
          minWidth={220}
          layer={port}
          onSelect={onSelect}
          onDismiss={onDismiss}
        />
      )}
    </>
  )
}

function renderBar(): {
  opened: Opened[]
  onSelect: ReturnType<typeof vi.fn>
  onDismiss: ReturnType<typeof vi.fn>
  setOpen: (open: boolean) => void
} {
  const { port, opened } = fakePort()
  const onSelect = vi.fn()
  const onDismiss = vi.fn()
  const view = render(<Bar port={port} open={false} onSelect={onSelect} onDismiss={onDismiss} />)
  const setOpen = (open: boolean): void =>
    view.rerender(<Bar port={port} open={open} onSelect={onSelect} onDismiss={onDismiss} />)
  return { opened, onSelect, onDismiss, setOpen }
}

describe('placePopup - where every popup goes, the layered menus included', () => {
  it('drops from its control and keeps 8px inside the window', () => {
    expect(placePopup({ below: BELOW }, SIZE, WINDOW)).toEqual({
      left: WINDOW.width - SIZE.width - POPUP_MARGIN,
      top: BELOW.bottom + 4
    })
  })

  it('flips above a control with no room under it, and never past the top margin', () => {
    const low = DOMRect.fromRect({ x: 100, y: 700, width: 28, height: 28 })
    expect(placePopup({ below: low }, SIZE, WINDOW)).toEqual({ left: 100, top: 700 - SIZE.height - 4 })
    expect(placePopup({ x: -50, y: -50 }, SIZE, WINDOW)).toEqual({ left: POPUP_MARGIN, top: POPUP_MARGIN })
  })
})

describe('Menu in the menu layer', () => {
  it('opens in the layer at the placed, measured rectangle, with its rows as data and no icons', () => {
    const { opened, setOpen } = renderBar()
    setOpen(true)
    expect(opened).toHaveLength(1)
    const place = placePopup({ below: BELOW }, SIZE, WINDOW)
    expect(opened[0]!.rect).toEqual({ x: place.left, y: place.top, ...SIZE })
    expect(opened[0]!.menu).toEqual({
      kind: 'menu',
      label: 'Browser',
      role: 'menu',
      stayOpen: false,
      entries: [
        { kind: 'heading', id: 'zoom-heading', label: 'Zoom 100%' },
        { kind: 'item', id: 'zoom-in', label: 'Zoom in', hint: 'Ctrl =' },
        { kind: 'separator', id: 'after-zoom' },
        { kind: 'item', id: 'devtools', label: 'Open DevTools', hint: 'F12', checked: false }
      ]
    })
    // Sent as data, so nothing structured-clone refuses can be in it.
    expect(() => structuredClone(opened[0]!.menu)).not.toThrow()
  })

  it('is no overlay, so the page behind it keeps painting, and draws nothing a person can reach in the window', () => {
    const { setOpen } = renderBar()
    setOpen(true)
    expect(overlayOpen()).toBe(false)
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('does what the picked row does, and closes on the pick', () => {
    const { opened, onSelect, onDismiss, setOpen } = renderBar()
    setOpen(true)
    act(() => opened[0]!.on.pick('devtools'))
    expect(onSelect).toHaveBeenCalledWith('devtools')
    act(() => opened[0]!.on.closed('pick'))
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })

  it('closes on Escape in the layer, and the keyboard goes back to the control that opened it', () => {
    const { opened, onDismiss, setOpen } = renderBar()
    const more = screen.getByRole('button', { name: 'More' })
    more.focus()
    setOpen(true)
    // The layer has the keys now; the window's document lost them.
    screen.getByRole('button', { name: 'Elsewhere' }).focus()
    act(() => opened[0]!.on.closed('escape'))
    expect(onDismiss).toHaveBeenCalledTimes(1)
    setOpen(false)
    expect(document.activeElement).toBe(more)
    expect(opened[0]!.closed).toBe(1)
  })

  it('closes when a click elsewhere takes the keys from the layer', () => {
    const { opened, onDismiss, setOpen } = renderBar()
    setOpen(true)
    act(() => opened[0]!.on.closed('blur'))
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })

  it("leaves a press on its own control to that control's click, which is what closes it", () => {
    const { opened, onDismiss, setOpen } = renderBar()
    setOpen(true)
    const more = screen.getByRole('button', { name: 'More' })
    fireEvent.pointerDown(more)
    act(() => opened[0]!.on.closed('blur'))
    fireEvent.pointerUp(more)
    expect(onDismiss).not.toHaveBeenCalled()
  })

  it('closes after all when that press is let go somewhere else, since no click is coming', () => {
    const { opened, onDismiss, setOpen } = renderBar()
    setOpen(true)
    fireEvent.pointerDown(screen.getByRole('button', { name: 'More' }))
    act(() => opened[0]!.on.closed('blur'))
    fireEvent.pointerUp(screen.getByRole('button', { name: 'Elsewhere' }))
    expect(onDismiss).toHaveBeenCalledTimes(1)
  })

  it('closes when the window changes size or the panes move, but not when the window loses the keys to the layer', () => {
    const { onDismiss, setOpen } = renderBar()
    setOpen(true)
    fireEvent.blur(window)
    expect(onDismiss).not.toHaveBeenCalled()
    fireEvent(window, new Event('resize'))
    expect(onDismiss).toHaveBeenCalledTimes(1)
    fireEvent(window, new Event(PANES_MOVED_EVENT))
    expect(onDismiss).toHaveBeenCalledTimes(2)
  })

  it('closes the layer when it goes, and sends new rows to the open one rather than opening again', () => {
    const { port, opened } = fakePort()
    const anchor = createRef<HTMLElement>()
    const draw = (entries: MenuEntry[]): JSX.Element => (
      <Menu
        label="Share"
        entries={entries}
        at={{ below: BELOW }}
        anchorRef={anchor}
        layer={port}
        onSelect={() => undefined}
        onDismiss={() => undefined}
      />
    )
    const view = render(draw(ENTRIES))
    view.rerender(draw(ENTRIES))
    expect(opened[0]!.updates).toEqual([])
    view.rerender(draw([{ kind: 'item', id: 'share-stop', label: 'Stop sharing' }]))
    expect(opened).toHaveLength(1)
    expect(opened[0]!.updates.map((u) => u.menu.entries)).toEqual([
      [{ kind: 'item', id: 'share-stop', label: 'Stop sharing' }]
    ])
    view.unmount()
    expect(opened[0]!.closed).toBe(1)
  })
})
