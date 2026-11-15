import type { BrowserWindow, WebContents, WebContentsView } from 'electron'
import type {
  BrowserLayerClosed,
  BrowserLayerContent,
  BrowserLayerEvent,
  BrowserLayerLook,
  BrowserLayerRect,
  BrowserLayerReport,
  BrowserLayerRequest
} from '../shared/ipc'
import { TITLEBAR_HEIGHT } from './chrome'

/**
 * The browser's menu layer: one transparent `WebContentsView` per window,
 * above the page views, that draws the More menu, the Share menu and the
 * address suggestions over a live page.
 *
 * A page's native view paints over all of the window's DOM, so a menu drawn
 * in the window used to need the page to stand down behind a still of itself
 * (`useBrowsers`, `cover`). Drawn in a view of its own above the page, the
 * menu opens in the next frame and the page keeps running behind it.
 *
 * **A Floti page, not a browser view.** It loads `layer.html` with the
 * window's preload and CSP, is never in the browser exemption registry, and
 * so `will-navigate` and `setWindowOpenHandler` stay denied for it.
 *
 * **Sized to the menu.** A transparent part of a view still takes the clicks
 * that land on it, so the layer's bounds are always the open menu's rectangle,
 * and idle it is hidden. The window measures and places the menu with the
 * popup placement it always used, and sends the rectangle in its CSS pixels;
 * this converts it with the window's zoom factor, as `bounds()` in
 * `browser.ts` does for a page.
 *
 * **Above every page.** Re-adding a view raises it, so whenever a browser view
 * is added or raised (`raise`, called from `browser.ts`), the layer is added
 * again after it.
 *
 * **The suggestions** are drawn the same way but never hold the keyboard:
 * the address bar keeps it and sends each change, and the layer reports the
 * row the pointer picks or is on. A press on the list hands the keyboard
 * straight back (see the `focus` listener), and a blur the bar reports closes
 * the list only when the keyboard is not back in the window (`blurred`).
 *
 * **Closing.** A pick or Escape in a menu closes it from the layer; a click
 * anywhere else - the page or the window's chrome - takes the keyboard from
 * the layer, and that blur closes it. Each is reported to the window on
 * `browser:layer-event`, with the turn it happened in. A close the window
 * asked for is not reported back. Closing hides the layer and, if the layer
 * held the keyboard, gives it back to the window, which returns it to the
 * control that opened the menu.
 */

export interface MenuLayerOptions {
  window: () => BrowserWindow | null
  /** Makes the layer's view: the window's preload, sandboxed, never a browser view. */
  createView: () => WebContentsView
  /** Loads the layer page into the view. */
  load: (contents: WebContents) => void
  /** What happened in the layer, for the window. */
  onEvent: (event: BrowserLayerEvent) => void
}

export interface MenuLayer {
  /** The window asked for a menu, a new rectangle or content for the one open, or a close. */
  request(payload: BrowserLayerRequest): void
  /** The layer page reported. */
  report(payload: BrowserLayerReport): void
  /**
   * A browser view was added or raised: put the layer back above it. The
   * layer is made here for a window that has none yet, so a window that never
   * shows a page runs no layer page, and the first menu over a page does not
   * wait for one to load.
   */
  raise(): void
  /** Whether these web contents are the layer page's. */
  isLayer(contentsId: number): boolean
  /** Closes the layer's page. Called from `before-quit`, beside the browser views. */
  destroy(): void
}

/** The window's client area, in DIPs. */
interface Area {
  width: number
  height: number
}

/**
 * A rectangle in the window's CSS pixels as the layer's bounds in DIPs.
 *
 * Each edge is converted on its own and the size taken between them, so the
 * layer's edges land on the same device pixels as the menu's would have in the
 * window. Kept inside the client area, and below the title bar, where Windows
 * draws the window controls: a view there would sit on the close button. Null
 * when nothing of it is left.
 */
export function layerBounds(rect: BrowserLayerRect, zoom: number, area: Area): BrowserLayerRect | null {
  const left = Math.max(0, Math.round(rect.x * zoom))
  const top = Math.max(TITLEBAR_HEIGHT, Math.round(rect.y * zoom))
  const right = Math.min(area.width, Math.round((rect.x + rect.width) * zoom))
  const bottom = Math.min(area.height, Math.round((rect.y + rect.height) * zoom))
  if (right <= left || bottom <= top) return null
  return { x: left, y: top, width: right - left, height: bottom - top }
}

interface Layer {
  win: BrowserWindow
  view: WebContentsView
  /**
   * Whether the page has said it is listening. Until then the layer is never
   * shown, and what it should draw waits here.
   */
  ready: boolean
  /** Its renderer died. The next open makes a new layer in its place. */
  gone: boolean
  /**
   * A press on the suggestions took the keyboard and it is on its way back
   * to the window: from the layer's `focus` until the window's.
   */
  handingBack: boolean
}

interface Open {
  turn: number
  content: BrowserLayerContent
  look: BrowserLayerLook
  zoom: number
  /** A menu has been given the keyboard, once, as it was first shown. */
  focused: boolean
  /** A mouse press reached the window's own page while this was open. */
  windowPressed: boolean
}

export function createMenuLayer(options: MenuLayerOptions): MenuLayer {
  let layer: Layer | null = null
  let open: Open | null = null

  const alive = (candidate: Layer | null): candidate is Layer =>
    candidate !== null && !candidate.win.isDestroyed() && !candidate.view.webContents.isDestroyed()

  /**
   * Draws what is open and shows it - only once the page is listening, so a
   * layer whose page has not loaded, or is loading again, never shows an
   * empty view that takes clicks. A menu takes the keyboard as it is first
   * shown, so its own arrows, type-ahead and Escape work.
   */
  const reveal = (): void => {
    if (!alive(layer) || !layer.ready || open === null) return
    layer.view.webContents.send('browser:layer-show', {
      turn: open.turn,
      content: open.content,
      look: open.look
    })
    layer.view.setVisible(true)
    if (open.content.kind === 'menu' && !open.focused) {
      open.focused = true
      layer.view.webContents.focus()
    }
  }

  /** Hides the layer. The keyboard goes back to the window if the layer had it, and nowhere if it had already gone. */
  const hide = (): void => {
    open = null
    if (!alive(layer)) return
    if (layer.ready) layer.view.webContents.send('browser:layer-show', null)
    const hadKeys = layer.view.webContents.isFocused()
    layer.view.setVisible(false)
    if (hadKeys) layer.win.webContents.focus()
  }

  /** The layer closed of its own accord: hidden, and the window told why. */
  const closed = (why: BrowserLayerClosed): void => {
    if (open === null) return
    const { turn } = open
    hide()
    options.onEvent({ turn, closed: why })
  }

  const make = (win: BrowserWindow): Layer => {
    const view = options.createView()
    // Clear where the menu does not draw: the corners of its rounded edge.
    view.setBackgroundColor('#00000000')
    view.setVisible(false)
    const made: Layer = { win, view, ready: false, gone: false, handingBack: false }
    const contents = view.webContents
    /*
     * A click anywhere else takes the keyboard from a menu, and closes it.
     * Where the keyboard went is read on the next turn, once Chromium has
     * placed it: to Floti's own window by a press on its page (`window`), which
     * reaches the page after this report does, or anywhere else (`blur`) -
     * the browser's page, another program, or the window by a way that sends
     * its page no press, such as the title bar, which nothing would follow.
     */
    contents.on('blur', () => {
      if (layer !== made || open?.content.kind !== 'menu') return
      const { turn } = open
      setImmediate(() => {
        if (layer !== made || open?.turn !== turn) return
        const toWindow = !win.isDestroyed() && win.webContents.isFocused() && open.windowPressed
        closed(toWindow ? 'window' : 'blur')
      })
    })
    win.webContents.on('before-mouse-event', (_event: unknown, mouse: { type: string }) => {
      if (layer === made && open !== null && mouse.type === 'mouseDown') open.windowPressed = true
    })
    /*
     * The suggestions never hold the keyboard: the bar does. A press on the
     * list still gives the layer focus (Chromium focuses the view a press
     * lands in), so it goes straight back to the window. Measured on 43.3.0
     * in the dev app, all within 4ms: the window's `blur`, the layer's
     * `focus`, the bar's blur, the window's `focus`, the bar's focus, and the
     * press's pick after. The bar reports its blur (`blurred`) before the
     * window has the keyboard again, so a blur that arrives while it is on
     * its way back is that press's, and keeps the list.
     */
    contents.on('focus', () => {
      if (layer !== made || open?.content.kind !== 'suggestions') return
      made.handingBack = true
      // Not inside this event: Chromium is still placing the press's focus,
      // and a focus asked for here was measured to be lost about half the time.
      setImmediate(() => {
        if (!win.isDestroyed() && !win.webContents.isDestroyed()) win.webContents.focus()
      })
    })
    win.webContents.on('focus', () => {
      made.handingBack = false
    })
    // A page that loads again (a reload in development) is not listening until it says so.
    contents.on('did-start-navigation', (details: { isMainFrame: boolean; isSameDocument: boolean }) => {
      if (!details.isMainFrame || details.isSameDocument) return
      made.ready = false
      view.setVisible(false)
    })
    // A page whose renderer died never listens again: the next open makes a new layer.
    contents.on('render-process-gone', () => {
      made.ready = false
      made.gone = true
      view.setVisible(false)
      if (layer === made) closed('blur')
    })
    win.once('closed', () => {
      if (!contents.isDestroyed()) contents.close()
      if (layer === made) {
        layer = null
        open = null
      }
    })
    win.contentView.addChildView(view)
    options.load(contents)
    return made
  }

  /** The layer of the window there is now, made if it has none or its page died. */
  const ensure = (): Layer | null => {
    const win = options.window()
    if (win === null || win.isDestroyed()) return null
    if (alive(layer) && layer.win === win && !layer.gone) return layer
    if (layer !== null) {
      if (!layer.win.isDestroyed()) layer.win.contentView.removeChildView(layer.view)
      if (!layer.view.webContents.isDestroyed()) layer.view.webContents.close()
    }
    layer = make(win)
    return layer
  }

  return {
    request(payload) {
      if ('close' in payload) {
        if (open?.turn === payload.close.turn) hide()
        return
      }
      if ('blurred' in payload) {
        if (open?.turn !== payload.blurred.turn || open.content.kind !== 'suggestions') return
        // A press on the list, the keyboard on its way back or back already.
        // Anywhere else closes the list.
        if (!alive(layer) || (!layer.handingBack && !layer.win.webContents.isFocused())) closed('blur')
        return
      }
      const current = ensure()
      if (current === null) return
      const { win, view } = current
      const { turn, rect, content, look } = payload.open
      const zoom = win.webContents.getZoomFactor()
      const area = win.getContentBounds()
      const bounds = layerBounds(rect, zoom, area)
      if (bounds === null) {
        if (open?.turn === turn) hide()
        return
      }
      const same = open?.turn === turn
      open = {
        turn,
        content,
        look,
        zoom,
        focused: same && open !== null && open.focused,
        windowPressed: same && open !== null && open.windowPressed
      }
      // The page draws at the window's scale, so the menu is the size the window measured.
      view.webContents.setZoomFactor(zoom)
      win.contentView.addChildView(view)
      view.setBounds(bounds)
      reveal()
    },

    report(payload) {
      if ('ready' in payload) {
        if (layer === null || layer.gone) return
        layer.ready = true
        reveal()
        return
      }
      if (open === null || payload.turn !== open.turn) return
      if ('pick' in payload) options.onEvent({ turn: payload.turn, pick: payload.pick })
      else if ('row' in payload) options.onEvent({ turn: payload.turn, row: payload.row })
      else if ('hover' in payload) options.onEvent({ turn: payload.turn, hover: payload.hover })
      else closed(payload.closed)
    },

    raise() {
      const current = ensure()
      if (current === null) return
      current.win.contentView.addChildView(current.view)
    },

    isLayer(contentsId) {
      return alive(layer) && layer.view.webContents.id === contentsId
    },

    destroy() {
      open = null
      if (layer !== null && !layer.view.webContents.isDestroyed()) layer.view.webContents.close()
      layer = null
    }
  }
}
