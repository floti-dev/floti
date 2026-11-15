import { describe, expect, it, vi } from 'vitest'
import type { BrowserWindow, WebContentsView } from 'electron'
import { DEFAULT_SETTINGS, openBrowserHistory, openStore, type AppSettings } from '@helm/core'
import { FakeBrowserWindow, FakeWebContentsView, type FakeWebContents } from '../../test/browser-electron'
import { createBrowserHost } from './browser'
import { TITLEBAR_HEIGHT } from './chrome'
import { createMenuLayer, layerBounds, type MenuLayer } from './menuLayer'
import type { BrowserLayerEvent, BrowserLayerLook, BrowserLayerMenu, BrowserLayerRequest } from '../shared/ipc'

vi.mock('electron', async () => ({
  ...(await import('../../test/electron')).electronFake(),
  ...(await import('../../test/browser-electron')).browserElectronFake()
}))

/**
 * The browser's menu layer, against a fake Electron: where it goes, what it
 * draws, that it stays above the pages, and how it closes. The layer page's
 * side (`renderer/src/layer/LayerRoot.tsx`) is played by the test through
 * `report`, as the window's is through `request`.
 */

const MENU: BrowserLayerMenu = {
  kind: 'menu',
  label: 'Browser',
  role: 'menu',
  stayOpen: false,
  entries: [
    { kind: 'item', id: 'zoom-in', label: 'Zoom in' },
    { kind: 'item', id: 'devtools', label: 'Open DevTools' }
  ]
}
const LOOK: BrowserLayerLook = { vars: { '--helm-bg': '#101010' }, kind: 'dark', data: { theme: 'night' } }
const RECT = { x: 600, y: 80, width: 220, height: 300 }

interface Rig {
  layer: MenuLayer
  window: FakeBrowserWindow
  /** The layer's view, once it has been made. */
  view: () => FakeWebContentsView
  contents: () => FakeWebContents
  events: BrowserLayerEvent[]
  loaded: number
}

function rig(): Rig {
  const window = new FakeBrowserWindow()
  const events: BrowserLayerEvent[] = []
  let made: FakeWebContentsView | null = null
  const state = { loaded: 0 }
  const layer = createMenuLayer({
    window: () => window as unknown as BrowserWindow,
    createView: () => {
      made = new FakeWebContentsView()
      return made as unknown as WebContentsView
    },
    load: () => {
      state.loaded += 1
    },
    onEvent: (event) => events.push(event)
  })
  return {
    layer,
    window,
    view: () => {
      if (made === null) throw new Error('no layer view yet')
      return made
    },
    contents: () => rigContents(made),
    events,
    get loaded() {
      return state.loaded
    }
  }
}

function rigContents(view: FakeWebContentsView | null): FakeWebContents {
  const contents = view?.webContents
  if (contents === undefined) throw new Error('no layer contents')
  return contents
}

/** After the callbacks already queued for the loop's next turn (`setImmediate`) have run. */
const nextTurn = (): Promise<void> => new Promise((resolve) => setImmediate(resolve))

const open = (turn: number, rect = RECT, content: BrowserLayerMenu = MENU): BrowserLayerRequest => ({
  open: { turn, rect, content, look: LOOK }
})

/** What the layer page has been told to draw, in order. */
const shown = (r: Rig): unknown[] =>
  r.contents().sent.filter((m) => m.channel === 'browser:layer-show').map((m) => m.payload)

describe('layerBounds - the window measures in CSS pixels; the layer is placed in DIPs', () => {
  it('converts each edge with the zoom factor', () => {
    expect(layerBounds({ x: 100, y: 100, width: 200, height: 100 }, 1.25, { width: 2000, height: 1200 })).toEqual({
      x: 125,
      y: 125,
      width: 250,
      height: 125
    })
  })

  it('lands its edges where the window would have, rounding the edges rather than the size', () => {
    // 10.4 -> 10.4*1.5 = 15.6 -> 16; right edge 10.4+20.4 = 30.8 -> 46.2 -> 46: 30 wide, not round(30.6) = 31.
    expect(layerBounds({ x: 10.4, y: 50, width: 20.4, height: 10 }, 1.5, { width: 500, height: 500 })).toMatchObject({
      x: 16,
      width: 30
    })
  })

  it('stays inside the window and below the title bar, where the window controls are', () => {
    expect(layerBounds({ x: -20, y: 0, width: 300, height: 100 }, 1, { width: 250, height: 80 })).toEqual({
      x: 0,
      y: TITLEBAR_HEIGHT,
      width: 250,
      height: 80 - TITLEBAR_HEIGHT
    })
  })

  it('is nothing when none of it is left', () => {
    expect(layerBounds({ x: 10, y: 0, width: 50, height: 20 }, 1, { width: 500, height: 500 })).toBeNull()
  })
})

describe('the menu layer - above every page', () => {
  it('is made with the first page, hidden and clear, and loads its own page', () => {
    const r = rig()
    r.layer.raise()
    expect(r.view().visible).toBe(false)
    expect(r.view().background).toBe('#00000000')
    expect(r.loaded).toBe(1)
    expect(r.window.children.at(-1)).toBe(r.view())
  })

  it('is raised above each page the browser adds, and above a page that goes fullscreen', () => {
    const r = rig()
    let settings: AppSettings = { ...DEFAULT_SETTINGS, browserProjectUrls: {} }
    const store = openStore({ file: ':memory:' })
    const browsers = createBrowserHost({
      history: openBrowserHistory(store),
      onFavicon: () => undefined,
      platform: 'win32',
      window: () => r.window as unknown as BrowserWindow,
      settings: () => settings,
      writeSettings: (next) => {
        settings = { ...settings, ...next }
      },
      onChanged: () => undefined,
      onOpened: () => undefined,
      onClosed: () => undefined,
      onCommand: () => undefined,
      onLogged: () => undefined,
      sessionOpener: () => null,
      shareTargets: () => [],
      onViewRaised: () => r.layer.raise()
    })
    const first = browsers.open({ url: 'http://localhost:3000/' }).state!
    const firstView = FakeWebContentsView.created.find((view) => view !== r.view() && r.window.children.includes(view))
    expect(r.window.children.at(-1)).toBe(r.view())
    browsers.open({ url: 'http://localhost:3001/' })
    expect(r.window.children.at(-1)).toBe(r.view())
    expect(r.window.children).toHaveLength(3)

    // The first page asks for the whole screen: it is raised over the second, and the layer over it.
    browsers.bounds({ id: first.id, ...RECT, visible: true })
    firstView!.webContents!.enterFullscreen()
    expect(r.window.children.at(-2)).toBe(firstView)
    expect(r.window.children.at(-1)).toBe(r.view())
    browsers.shutdown()
    store.close()
  })

  it('makes one layer per window, and a new one for a new window', () => {
    let current = new FakeBrowserWindow()
    const made: FakeWebContentsView[] = []
    const layer = createMenuLayer({
      window: () => current as unknown as BrowserWindow,
      createView: () => {
        const view = new FakeWebContentsView()
        made.push(view)
        return view as unknown as WebContentsView
      },
      load: () => undefined,
      onEvent: () => undefined
    })
    layer.raise()
    layer.raise()
    expect(made).toHaveLength(1)
    // The window closed (macOS keeps the app), and a new one opened.
    current.destroy()
    expect(made[0]!.webContents!.isDestroyed()).toBe(true)
    current = new FakeBrowserWindow()
    layer.raise()
    expect(made).toHaveLength(2)
    expect(current.children).toEqual([made[1]])
  })
})

describe('the menu layer - opening', () => {
  it('draws the menu in its rectangle, at the window zoom, takes the keys, and holds what to draw until its page listens', () => {
    const r = rig()
    r.window.webContents.zoomFactor = 1.25
    r.layer.request(open(1))
    expect(shown(r)).toEqual([])
    expect(r.view().getBounds()).toEqual({ x: 750, y: 100, width: 275, height: 375 })
    expect(r.view().visible).toBe(true)
    expect(r.contents().zoomFactor).toBe(1.25)
    expect(r.contents().focused).toBe(1)

    r.layer.report({ ready: true })
    expect(shown(r)).toEqual([{ turn: 1, content: MENU, look: LOOK }])
  })

  it('takes new content or a new place for the open menu without taking the keys again', () => {
    const r = rig()
    r.layer.report({ ready: true })
    r.layer.raise()
    r.layer.report({ ready: true })
    r.layer.request(open(1))
    const moved = { ...RECT, x: 500 }
    r.layer.request(open(1, moved))
    expect(r.view().getBounds().x).toBe(500)
    expect(r.contents().focused).toBe(1)
    expect(shown(r)).toHaveLength(2)
  })

  it('is raised to the top as it opens', () => {
    const r = rig()
    r.layer.raise()
    const page = new FakeWebContentsView()
    r.window.contentView.addChildView(page)
    r.layer.request(open(1))
    expect(r.window.children.at(-1)).toBe(r.view())
  })

  it('shows nothing for a rectangle with nothing left of it', () => {
    const r = rig()
    r.layer.request(open(1, { x: 10, y: 0, width: 40, height: 20 }))
    expect(r.view().visible).toBe(false)
  })
})

describe('the menu layer - closing', () => {
  const opened = (): Rig => {
    const r = rig()
    r.layer.raise()
    r.layer.report({ ready: true })
    r.layer.request(open(1))
    // Chromium gave the layer the keys main asked for.
    r.contents().hasFocus = true
    return r
  }

  it('passes a pick on, then closes on the pick: hidden, cleared, and the keys back to the window', () => {
    const r = opened()
    r.layer.report({ turn: 1, pick: 'devtools' })
    r.layer.report({ turn: 1, closed: 'pick' })
    expect(r.events).toEqual([
      { turn: 1, pick: 'devtools' },
      { turn: 1, closed: 'pick' }
    ])
    expect(r.view().visible).toBe(false)
    expect(shown(r).at(-1)).toBeNull()
    expect(r.window.webContents.focused).toBe(1)
  })

  it('closes on Escape, giving the keys back to the window', () => {
    const r = opened()
    r.layer.report({ turn: 1, closed: 'escape' })
    expect(r.events).toEqual([{ turn: 1, closed: 'escape' }])
    expect(r.view().visible).toBe(false)
    expect(r.window.webContents.focused).toBe(1)
  })

  it('closes when a click anywhere else takes the keys, and leaves them where the click put them', () => {
    const r = opened()
    r.contents().hasFocus = false
    r.contents().emit('blur')
    expect(r.events).toEqual([{ turn: 1, closed: 'blur' }])
    expect(r.view().visible).toBe(false)
    expect(r.window.webContents.focused).toBe(0)
  })

  it('closes when the window asks, without telling it, and ignores a close for a turn that is over', () => {
    const r = opened()
    r.layer.request({ close: { turn: 0 } })
    expect(r.view().visible).toBe(true)
    r.layer.request({ close: { turn: 1 } })
    expect(r.view().visible).toBe(false)
    expect(r.events).toEqual([])
    // Hidden, the blur that follows is no close.
    r.contents().emit('blur')
    expect(r.events).toEqual([])
  })

  it("ignores what the page says about a turn that is not the open one's", () => {
    const r = opened()
    r.layer.request(open(2))
    r.layer.report({ turn: 1, pick: 'zoom-in' })
    r.layer.report({ turn: 1, closed: 'escape' })
    expect(r.events).toEqual([])
    expect(r.view().visible).toBe(true)
  })

  it('closes when its page dies, and holds what to draw until the page listens again', () => {
    const r = opened()
    r.contents().emit('render-process-gone', {}, { reason: 'crashed' })
    expect(r.events).toEqual([{ turn: 1, closed: 'blur' }])
    const before = shown(r).length
    r.layer.request(open(2))
    expect(shown(r)).toHaveLength(before)
    r.layer.report({ ready: true })
    expect(shown(r)).toHaveLength(before + 1)
  })

  it('knows its own page from any other', () => {
    const r = opened()
    expect(r.layer.isLayer(r.contents().id)).toBe(true)
    expect(r.layer.isLayer(r.window.webContents.id)).toBe(false)
    r.layer.destroy()
    expect(r.layer.isLayer(r.contents().id)).toBe(false)
  })
})

describe('the menu layer - the address suggestions', () => {
  const SUGGESTIONS = {
    kind: 'suggestions' as const,
    rows: [
      { kind: 'search' as const, text: 'docs', engine: 'Google' },
      { kind: 'page' as const, url: 'http://localhost:3000/docs', title: 'Docs' }
    ],
    highlighted: -1
  }
  const suggesting = (): Rig => {
    const r = rig()
    r.layer.raise()
    r.layer.report({ ready: true })
    r.layer.request({ open: { turn: 3, rect: RECT, content: SUGGESTIONS, look: LOOK } })
    return r
  }

  it('draws them over the page and leaves the keyboard in the bar', () => {
    const r = suggesting()
    expect(r.view().visible).toBe(true)
    expect(r.contents().focused).toBe(0)
    expect(shown(r).at(-1)).toEqual({ turn: 3, content: SUGGESTIONS, look: LOOK })
  })

  it("passes on the pointer's pick and hover, by row", () => {
    const r = suggesting()
    r.layer.report({ turn: 3, hover: 1 })
    r.layer.report({ turn: 3, row: 1 })
    expect(r.events).toEqual([
      { turn: 3, hover: 1 },
      { turn: 3, row: 1 }
    ])
  })

  it('hands the keyboard straight back to the window when a press on the list takes it, and that blur is no close', async () => {
    const r = suggesting()
    r.contents().emit('focus')
    // On the next turn of the loop, once Chromium has placed the press's focus.
    expect(r.window.webContents.focused).toBe(0)
    await nextTurn()
    // The layer losing the keys again is nothing to the suggestions.
    r.contents().emit('blur')
    // The bar's blur is reported before the window has the keyboard again.
    r.window.webContents.hasFocus = false
    r.layer.request({ blurred: { turn: 3 } })
    expect(r.view().visible).toBe(true)
    // Then it has, and a blur reported now is answered by where the keyboard is.
    r.window.webContents.hasFocus = true
    r.window.webContents.emit('focus')
    r.layer.request({ blurred: { turn: 3 } })
    expect(r.view().visible).toBe(true)
    expect(r.events).toEqual([])

    // The keyboard went to the page.
    r.window.webContents.hasFocus = false
    r.layer.request({ blurred: { turn: 3 } })
    expect(r.events).toEqual([{ turn: 3, closed: 'blur' }])
  })

  it('closes them when the bar loses the keyboard to anything but the list, and ignores a blur for a turn that is over', () => {
    const r = suggesting()
    r.window.webContents.hasFocus = false
    r.layer.request({ blurred: { turn: 2 } })
    expect(r.view().visible).toBe(true)
    r.layer.request({ blurred: { turn: 3 } })
    expect(r.view().visible).toBe(false)
    expect(r.events).toEqual([{ turn: 3, closed: 'blur' }])
  })

  it('takes no blur report for a menu, which closes on its own blur', async () => {
    const r = rig()
    r.layer.raise()
    r.layer.request(open(1))
    r.layer.request({ blurred: { turn: 1 } })
    expect(r.view().visible).toBe(true)
    r.contents().emit('focus')
    await nextTurn()
    expect(r.window.webContents.focused).toBe(0)
  })
})
