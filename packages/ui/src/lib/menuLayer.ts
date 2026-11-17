import type { BrowserSuggestion } from '@floti/core/types'
import type { MenuEntry } from '../components/Menu'

/**
 * The browser's menu layer, as the window sees it: a view of its own above
 * the browser's pages (`main/menuLayer.ts` in the desktop), where the bar's
 * menus and the address suggestions are drawn over a live page. The window
 * keeps what each row does; the layer draws data and says what happened.
 */

/** A row the menu layer can draw: a `MenuEntry` with nothing that cannot cross a process. */
export type LayerMenuEntry =
  | Omit<Extract<MenuEntry, { kind: 'item' }>, 'icon'>
  | Extract<MenuEntry, { kind: 'separator' | 'heading' }>

/** A menu, as data the menu layer draws. */
export interface LayerMenu {
  kind: 'menu'
  label: string
  role: 'menu' | 'listbox'
  stayOpen: boolean
  entries: LayerMenuEntry[]
}

/**
 * Why the menu layer closed: a key, a pick, or a click anywhere else - in
 * the window's own chrome (`window`, whose press reaches the window after
 * this does) or outside it (`blur`: the page, another program).
 */
export type LayerMenuClosed = 'escape' | 'blur' | 'window' | 'pick'

/** A rectangle in the window's CSS pixels. */
export interface LayerRect {
  x: number
  y: number
  width: number
  height: number
}

/** One menu open in the layer: new content or a new place for it, or gone. */
export interface MenuLayerHandle {
  update: (menu: LayerMenu, rect: LayerRect) => void
  close: () => void
}

/**
 * Where a menu is drawn when it is not in the window: the browser's menu
 * layer, a view above the page (`main/menuLayer.ts`). The window keeps what
 * each row does; the layer says which row was picked, by its id.
 */
export interface MenuLayerPort {
  open: (
    menu: LayerMenu,
    rect: LayerRect,
    on: { pick: (id: string) => void; closed: (why: LayerMenuClosed) => void }
  ) => MenuLayerHandle
  /**
   * The address suggestions. The bar keeps the keyboard throughout; the layer
   * says which row the pointer picked or is on, by index.
   */
  openSuggestions: (
    content: LayerSuggestions,
    rect: LayerRect,
    on: { pick: (index: number) => void; hover: (index: number) => void; closed: (why: 'blur') => void }
  ) => SuggestionsLayerHandle
}

/**
 * The address suggestions, as data the layer draws: `BrowserSuggestions`'
 * rows and the row the bar's keys or the pointer are on (-1 for none).
 */
export interface LayerSuggestions {
  kind: 'suggestions'
  rows: BrowserSuggestion[]
  highlighted: number
}

/** The suggestions open in the layer. */
export interface SuggestionsLayerHandle {
  update: (content: LayerSuggestions, rect: LayerRect) => void
  /**
   * The window lost the keyboard while the bar had it. The layer closes the
   * list (`closed('blur')`) unless the keyboard is back in the window: a press
   * on the list takes it for a moment and main hands it straight back, and
   * the pick that press makes follows.
   */
  blurred: () => void
  close: () => void
}
