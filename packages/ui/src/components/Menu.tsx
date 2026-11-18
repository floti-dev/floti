import type { CSSProperties, JSX, KeyboardEvent, ReactNode, RefObject } from 'react'
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { cn } from '../lib/cn'
import { overlayOpen, subscribeOverlay } from '../lib/overlay'
import { PANES_MOVED_EVENT } from '../lib/paneGeometry'
import { placePopup, usePopup, type PopupAnchor } from '../lib/popup'
import { CheckIcon } from './icons'
import type {
  LayerMenu,
  LayerMenuEntry,
  LayerRect,
  MenuLayerHandle,
  MenuLayerPort
} from '../lib/menuLayer'

/**
 * A list that drops from a control or opens at the pointer: a choice of one
 * (the Files view's project), or a set of ticks (which destinations the rail
 * shows).
 *
 * Drawn by Floti rather than left to the platform, which is what the native
 * `<select>` it replaces did - Chromium painted that list white, with the
 * app's light text on it, in every theme. This one is the popup recipe
 * (DESIGN.md "Menus"): `surface-raised` behind a `border-strong` edge, the
 * well's corner, no shadow, rows that answer the pointer.
 *
 * **It counts as an overlay while it is open.** A browser tab is a native view
 * that paints over every bit of renderer DOM, and a menu opened from the rail
 * or the sidebar can reach across a pane holding one - so it registers in
 * `lib/overlay.ts` exactly as a dialog does, and the view gets out of the way.
 *
 * **Or it is drawn in the browser's menu layer** (`layer`), a view of its own
 * above the page, which is what the browser bar's own menus do: the page keeps
 * running and painting behind them. The window still measures and places the
 * menu - drawn hidden, once, with the same recipe - and the layer draws it in
 * exactly that rectangle. It is no overlay then, so nothing stands down.
 *
 * Portalled to `document.body` and positioned in the viewport, because the
 * sidebar and the rail both clip their contents and a list cut off at the
 * sidebar's edge is the failure this is here to avoid.
 */

export type MenuEntry =
  | {
      kind: 'item'
      id: string
      label: string
      icon?: ReactNode
      /** Quieter text on the right - a path, a shortcut, a state. */
      hint?: string | undefined
      /** The hint in `danger`: a state that is a fault, such as a host out of reach. */
      hintTone?: 'danger' | undefined
      /** The hint is words, not machine data, so it is set in sans: a state such as "connected". Mono otherwise. */
      hintSans?: boolean | undefined
      /** The label is machine data, set in mono: a remote host's name. */
      mono?: boolean | undefined
      /** A tick menu's state, or a list's current choice. Undefined draws no tick column. */
      checked?: boolean | undefined
      disabled?: boolean | undefined
      /** Said on hover - why a disabled row is disabled. */
      title?: string | undefined
      /** A second, quieter line under the label: what it is, or why it is disabled. */
      detail?: string | undefined
    }
  | { kind: 'separator'; id: string }
  | { kind: 'heading'; id: string; label: string }

/** Where the menu opens: at a point (a right-click), or under a control. */
export type MenuAnchor = PopupAnchor

export interface MenuProps {
  /** The menu's accessible name. */
  label: string
  entries: readonly MenuEntry[]
  at: MenuAnchor
  /**
   * `listbox` for a choice of one (the row picked closes it), `menu` for
   * commands and ticks.
   */
  role?: 'menu' | 'listbox'
  /** A tick menu stays open so several can be changed in one visit. */
  stayOpen?: boolean
  /** Never narrower than this - the control it drops from, usually. */
  minWidth?: number
  /**
   * The control that opened it. A press on it is not a press outside: the
   * control's own click is what closes the menu, and treating the press as
   * outside too would close it and open it again in one click.
   */
  anchorRef?: RefObject<HTMLElement | null> | undefined
  /** Draw it in the browser's menu layer, over the page, rather than in the window. Icons are not drawn there. */
  layer?: MenuLayerPort | undefined
  onSelect: (id: string) => void
  onDismiss: () => void
}

/** Letters typed within this of each other are one prefix. */
const TYPEAHEAD_MS = 600

/** The popup recipe's frame, wherever the list is drawn: the window, or the menu layer's page. */
export const MENU_FRAME = 'rounded-well border border-border-strong bg-surface-raised p-1 text-fg outline-none'
/** As tall and wide as a menu in the window may be. */
const POPUP_LIMITS = 'max-h-[min(420px,calc(100vh-16px))] max-w-[360px] overflow-y-auto'

type Item = Extract<MenuEntry, { kind: 'item' }>
type ListEntry = MenuEntry | LayerMenuEntry
type ListItem = Extract<ListEntry, { kind: 'item' }>

const isItem = (entry: ListEntry): entry is ListItem => entry.kind === 'item'

export function Menu(props: MenuProps): JSX.Element {
  return props.layer === undefined ? <PopupMenu {...props} /> : <LayeredMenu {...props} layer={props.layer} />
}

/** A menu in the window: an overlay, placed once it is measured, dismissed by a press outside it. */
function PopupMenu({
  label,
  entries,
  at,
  role = 'menu',
  stayOpen = false,
  minWidth = 180,
  anchorRef,
  onSelect,
  onDismiss
}: MenuProps): JSX.Element {
  const menuRef = useRef<HTMLDivElement>(null)
  const place = usePopup({ ref: menuRef, at, anchorRef, onDismiss })
  useFocusReturns()
  return createPortal(
    <MenuList
      listRef={menuRef}
      label={label}
      entries={entries}
      role={role}
      stayOpen={stayOpen}
      // Focus moves in once the menu is placed. Not before: it is drawn hidden
      // for the frame it is measured in, and a hidden element refuses focus -
      // so focusing it on mount left the keys going to the control that opened it.
      focus={place !== null}
      onSelect={onSelect}
      onDismiss={onDismiss}
      style={{
        left: place?.left ?? 0,
        top: place?.top ?? 0,
        minWidth,
        // Measured where it lands, then shown - one frame at 0,0 would flash
        // in the corner of the window.
        visibility: place === null ? 'hidden' : undefined
      }}
      className={cn('fixed z-50', POPUP_LIMITS, MENU_FRAME)}
    />,
    document.body
  )
}

/**
 * Focus goes back to whatever had it when the menu opened, once it closes.
 * `unlessMoved`: only if nothing else in the window has taken it since - a
 * menu in the layer leaves the window's focus where it was, and a dialog that
 * opened over it keeps the focus it took.
 */
function useFocusReturns(unlessMoved = false): void {
  useEffect(() => {
    const before = document.activeElement
    return () => {
      if (!(before instanceof HTMLElement) || !before.isConnected) return
      const now = document.activeElement
      if (unlessMoved && now !== null && now !== document.body && now !== before) return
      before.focus({ preventScroll: true })
    }
  }, [unlessMoved])
}

/**
 * Something on the menu layer goes when an overlay opens: a dialog Floti
 * opens on its own is drawn in the window, under the layer, so whatever the
 * layer shows would sit on top of it.
 */
export function useClosedByOverlay(close: () => void): void {
  const latest = useRef(close)
  useLayoutEffect(() => {
    latest.current = close
  })
  useEffect(() => {
    const check = (): void => {
      if (overlayOpen()) latest.current()
    }
    check()
    return subscribeOverlay(check)
  }, [])
}

/** The rows the layer can draw: everything but the icons. */
export function layerEntries(entries: readonly MenuEntry[]): LayerMenuEntry[] {
  return entries.map((entry) => {
    if (entry.kind !== 'item') return entry
    const { icon: _icon, ...rest } = entry
    return rest
  })
}

/**
 * A menu drawn in the browser's menu layer.
 *
 * The window draws the list once, hidden and inert, with the same recipe and
 * the same limits a menu in the window has, measures it, places it with the
 * popup placement, and hands the layer the rectangle. Its content changing, or
 * its size, sends it again.
 *
 * Dismissed when the layer says it closed (a pick, Escape, or a click anywhere
 * else taking the keyboard from it), when the window changes size, or when the
 * panes move: a menu left floating over a layout that moved points at nothing.
 * And when an overlay opens, which is drawn under the layer.
 * Not when the window loses focus - the layer taking the keys is exactly that.
 * A click on the control that opened it blurs the layer too, and that control's
 * own click is what closes the menu, so that blur is not a dismissal. The
 * close reaches the window before the press that caused it, so a close for a
 * press in the window (`window`) waits for that press to say where it was.
 */
function LayeredMenu({
  label,
  entries,
  at,
  role = 'menu',
  stayOpen = false,
  minWidth = 180,
  anchorRef,
  layer,
  onSelect,
  onDismiss
}: MenuProps & { layer: MenuLayerPort }): JSX.Element {
  const measureRef = useRef<HTMLDivElement>(null)
  const handle = useRef<MenuLayerHandle | null>(null)
  const menu: LayerMenu = { kind: 'menu', label, role, stayOpen, entries: layerEntries(entries) }
  const content = JSON.stringify(menu)
  // What the layer's callbacks and the measuring read, as of this render.
  // First of the layout effects, so the ones below see it.
  const latest = useRef({ menu, at, onSelect, onDismiss })
  useLayoutEffect(() => {
    latest.current = { menu, at, onSelect, onDismiss }
  })
  useFocusReturns(true)
  useClosedByOverlay(onDismiss)

  /**
   * The press in the window that took the keyboard from the layer, which can
   * reach the window before or after main's word of the close it caused:
   * whether one on the opening control is down, whether the layer closed
   * during it, whether a press elsewhere came first and its close has not
   * (`elsewhere`), and whether the close came first and its press has not
   * (`awaiting`).
   */
  const press = useRef({ down: false, blurred: false, elsewhere: false, awaiting: false })

  /** The rectangle the layer draws the menu in, measured from the hidden copy. */
  const measure = (): LayerRect | null => {
    const node = measureRef.current
    if (node === null) return null
    const { width, height } = node.getBoundingClientRect()
    const place = placePopup(latest.current.at, { width, height }, { width: window.innerWidth, height: window.innerHeight })
    return { x: place.left, y: place.top, width, height }
  }

  // Opened once, closed when the menu goes.
  useLayoutEffect(() => {
    const rect = measure()
    if (rect === null) return
    const opened = layer.open(latest.current.menu, rect, {
      pick: (id) => latest.current.onSelect(id),
      closed: (why) => {
        if (why === 'blur' || why === 'window') {
          if (press.current.down) {
            press.current.blurred = true
            return
          }
          // A press in the window took the keyboard. If it has not reached the
          // window yet, whether it is on the opening control decides.
          if (why === 'window' && !press.current.elsewhere) {
            press.current.awaiting = true
            return
          }
        }
        press.current.elsewhere = false
        latest.current.onDismiss()
      }
    })
    handle.current = opened
    return () => {
      handle.current = null
      opened.close()
    }
  }, [layer])

  // New content or a new anchor: measured and sent again. Its size changing
  // after the fonts arrive, too.
  const sent = useRef<string | null>(null)
  useLayoutEffect(() => {
    const node = measureRef.current
    if (node === null) return
    const send = (): void => {
      const rect = measure()
      if (rect === null || handle.current === null) return
      const key = `${content}|${JSON.stringify(rect)}`
      if (sent.current === null) {
        // The open above already sent this.
        sent.current = key
        return
      }
      if (key === sent.current) return
      sent.current = key
      handle.current.update(latest.current.menu, rect)
    }
    send()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(send)
    observer.observe(node)
    return () => observer.disconnect()
  }, [content, at])

  useEffect(() => {
    const away = (): void => latest.current.onDismiss()
    const onAnchor = (event: Event): boolean =>
      event.target instanceof Node && anchorRef?.current?.contains(event.target) === true
    const down = (event: PointerEvent): void => {
      const { awaiting } = press.current
      const anchor = onAnchor(event)
      press.current = { down: anchor, blurred: awaiting, elsewhere: !awaiting && !anchor, awaiting: false }
      // The press the close was waiting for, somewhere other than the control.
      if (awaiting && !anchor) latest.current.onDismiss()
    }
    // Released off the control that was pressed: no click is coming to close
    // the menu, so a blur the press caused closes it now.
    const up = (event: PointerEvent): void => {
      const { down: wasDown, blurred, elsewhere } = press.current
      press.current = { down: false, blurred: false, elsewhere, awaiting: false }
      if (wasDown && blurred && !onAnchor(event)) latest.current.onDismiss()
    }
    // The keyboard came back to the window without a press: a key closes it.
    const key = (): void => {
      if (press.current.awaiting) latest.current.onDismiss()
    }
    window.addEventListener('resize', away)
    window.addEventListener(PANES_MOVED_EVENT, away)
    document.addEventListener('pointerdown', down, true)
    document.addEventListener('pointerup', up, true)
    document.addEventListener('keydown', key, true)
    return () => {
      window.removeEventListener('resize', away)
      window.removeEventListener(PANES_MOVED_EVENT, away)
      document.removeEventListener('pointerdown', down, true)
      document.removeEventListener('pointerup', up, true)
      document.removeEventListener('keydown', key, true)
    }
  }, [anchorRef])

  return createPortal(
    <div aria-hidden inert data-menu-measure={label} className="pointer-events-none fixed top-0 left-0 invisible">
      <MenuList
        listRef={measureRef}
        label={label}
        entries={entries}
        role={role}
        stayOpen={stayOpen}
        focus={false}
        onSelect={() => undefined}
        onDismiss={() => undefined}
        style={{ minWidth }}
        className={cn(POPUP_LIMITS, MENU_FRAME)}
      />
    </div>,
    document.body
  )
}

export interface MenuListProps {
  label: string
  entries: readonly ListEntry[]
  role: 'menu' | 'listbox'
  stayOpen: boolean
  /** Take the keyboard: once the list is placed, or as soon as it is drawn in the layer. */
  focus: boolean
  onSelect: (id: string) => void
  /** Escape or Tab, or a row picked in a menu that does not stay open. */
  onDismiss: (why: 'escape' | 'pick') => void
  className?: string
  style?: CSSProperties
  listRef?: RefObject<HTMLDivElement | null>
}

/**
 * The list itself: its rows and its keys. Drawn by a menu in the window, and
 * by the browser's menu layer filling its own page.
 */
export function MenuList({
  label,
  entries,
  role,
  stayOpen,
  focus,
  onSelect,
  onDismiss,
  className,
  style,
  listRef
}: MenuListProps): JSX.Element {
  const ids = useId()
  const ownRef = useRef<HTMLDivElement>(null)
  const menuRef = listRef ?? ownRef
  const items = entries.filter(isItem)
  const ticks = items.some((item) => item.checked !== undefined)

  // A list opens on its current choice; a menu on its first row.
  const [active, setActive] = useState(() => {
    const current = role === 'listbox' ? items.findIndex((item) => item.checked === true && !item.disabled) : -1
    return current >= 0 ? current : items.findIndex((item) => !item.disabled)
  })
  const typed = useRef({ text: '', at: 0 })

  useEffect(() => {
    if (focus) menuRef.current?.focus({ preventScroll: true })
  }, [focus, menuRef])

  // The row the keyboard is on stays in view in a list long enough to scroll.
  useEffect(() => {
    menuRef.current?.querySelector(`[data-menu-index="${String(active)}"]`)?.scrollIntoView({ block: 'nearest' })
  }, [active, menuRef])

  const choose = (item: ListItem | undefined): void => {
    if (item === undefined || item.disabled === true) return
    onSelect(item.id)
    if (!stayOpen) onDismiss('pick')
  }

  /** The next row that can be chosen, `step` at a time from `from`, wrapping. */
  const next = (from: number, step: 1 | -1): number => {
    for (let i = 1; i <= items.length; i += 1) {
      const at = (from + step * i + items.length * 2) % items.length
      if (items[at]?.disabled !== true) return at
    }
    return from
  }

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const key = event.key
    if (key === 'Escape' || key === 'Tab') {
      event.preventDefault()
      event.stopPropagation()
      onDismiss('escape')
      return
    }
    if (key === 'ArrowDown' || key === 'ArrowUp') {
      event.preventDefault()
      setActive((current) => next(current, key === 'ArrowDown' ? 1 : -1))
      return
    }
    if (key === 'Home' || key === 'End') {
      event.preventDefault()
      setActive(key === 'Home' ? next(-1, 1) : next(items.length, -1))
      return
    }
    if (key === 'Enter' || key === ' ') {
      event.preventDefault()
      choose(items[active])
      return
    }
    if (key.length === 1 && !event.ctrlKey && !event.altKey && !event.metaKey) {
      // Type-ahead: the letters typed so far, matched against the start of
      // each label from the row after this one.
      const now = Date.now()
      const text = (now - typed.current.at < TYPEAHEAD_MS ? typed.current.text : '') + key.toLowerCase()
      typed.current = { text, at: now }
      const start = text.length === 1 ? active + 1 : active
      for (let i = 0; i < items.length; i += 1) {
        const at = (start + i) % items.length
        const item = items[at]
        if (item !== undefined && item.disabled !== true && item.label.toLowerCase().startsWith(text)) {
          setActive(at)
          break
        }
      }
    }
  }

  // Which row the keyboard counts each item as, worked out before drawing:
  // separators and headings are in `entries` but are not stops.
  const stop = new Map(items.map((item, at) => [item.id, at]))
  return (
    <div
      ref={menuRef}
      role={role}
      aria-label={label}
      aria-activedescendant={active >= 0 ? `${ids}-${String(active)}` : undefined}
      tabIndex={-1}
      onKeyDown={onKeyDown}
      onContextMenu={(event) => event.preventDefault()}
      data-menu={label}
      style={style}
      className={className}
    >
      {entries.map((entry) => {
        if (entry.kind === 'separator') {
          return <div key={entry.id} role="separator" className="mx-1 my-1 h-px bg-border" />
        }
        if (entry.kind === 'heading') {
          return (
            <div
              key={entry.id}
              role="presentation"
              className="px-2 pt-1.5 pb-1 text-[10px] font-semibold tracking-[.07em] text-fg-subtle uppercase"
            >
              {entry.label}
            </div>
          )
        }
        const at = stop.get(entry.id) ?? -1
        const on = at === active
        const icon = 'icon' in entry ? (entry as Item).icon : undefined
        return (
          <div
            key={entry.id}
            id={`${ids}-${String(at)}`}
            data-menu-index={at}
            data-menu-item={entry.id}
            // A tick is said on the rows that have one: a menu that mixes
            // commands with a choice (the browser's) keeps its commands menu items.
            role={role === 'listbox' ? 'option' : entry.checked !== undefined ? 'menuitemcheckbox' : 'menuitem'}
            aria-checked={role === 'menu' && entry.checked !== undefined ? entry.checked : undefined}
            aria-selected={role === 'listbox' ? entry.checked === true : undefined}
            aria-disabled={entry.disabled === true ? true : undefined}
            title={entry.title}
            // The pointer never takes focus from the menu, so the arrows go on
            // from wherever it left the highlight.
            onMouseDown={(event) => event.preventDefault()}
            onMouseMove={entry.disabled === true || on ? undefined : () => setActive(at)}
            onClick={() => choose(entry)}
            className={cn(
              'flex cursor-default items-center gap-2 rounded-raised px-2 text-[12px] whitespace-nowrap',
              entry.detail === undefined ? 'h-7' : 'py-[5px]',
              entry.disabled === true ? 'text-fg-subtle' : on ? 'bg-hover text-fg' : 'text-fg'
            )}
          >
            {ticks && (
              <span aria-hidden className="grid w-3.5 shrink-0 place-items-center text-accent-text">
                {entry.checked === true && <CheckIcon width={12} height={12} />}
              </span>
            )}
            {icon !== undefined && (
              <span aria-hidden className="grid shrink-0 place-items-center text-fg-muted">
                {icon}
              </span>
            )}
            {entry.detail === undefined ? (
              <span className={cn('min-w-0 flex-1 truncate', entry.mono === true && 'font-mono text-[11.5px]')}>
                {entry.label}
              </span>
            ) : (
              <span className="flex min-w-0 flex-1 flex-col gap-px">
                <span className={cn('truncate', entry.mono === true && 'font-mono text-[11.5px]')}>{entry.label}</span>
                <span data-menu-detail className="truncate text-[10.5px] leading-[1.35] text-fg-subtle">
                  {entry.detail}
                </span>
              </span>
            )}
            {entry.hint !== undefined && (
              <span
                className={cn(
                  'ml-3 shrink-0',
                  entry.hintSans === true ? 'text-[10.5px]' : 'font-mono text-[11px]',
                  entry.hintTone === 'danger' ? 'text-danger' : 'text-fg-subtle'
                )}
              >
                {entry.hint}
              </span>
            )}
          </div>
        )
      })}
    </div>
  )
}
