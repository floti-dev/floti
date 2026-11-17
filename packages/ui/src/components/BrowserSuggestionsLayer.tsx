import { useLayoutEffect, useRef, type JSX, type RefObject } from 'react'
import type { BrowserSuggestion } from '@floti/core/types'
import type { LayerRect, LayerSuggestions, MenuLayerPort, SuggestionsLayerHandle } from '../lib/menuLayer'
import { PANES_MOVED_EVENT } from '../lib/paneGeometry'
import { BrowserSuggestions } from './BrowserSuggestions'
import { useClosedByOverlay } from './Menu'

export interface BrowserSuggestionsLayerProps {
  layer: MenuLayerPort
  /** The listbox's id, which the address field's `aria-activedescendant` names rows by. */
  id: string
  rows: readonly BrowserSuggestion[]
  highlighted: number
  onPick: (index: number) => void
  onHover: (index: number) => void
  /** The layer closed the list: the bar's keyboard went somewhere other than the list. */
  onClosed: () => void
  /** Where the bar finds the open list, to say it lost the keyboard (`blurred`). */
  handleRef: RefObject<SuggestionsLayerHandle | null>
}

/**
 * The address suggestions, drawn over the live page in the browser's menu
 * layer, and kept in the window as the list assistive technology reads.
 *
 * The page's native view paints over the window's DOM, so the list a person
 * sees is the layer's. The window keeps the same `BrowserSuggestions` where the
 * list hangs from the bar, transparent and taking no pointer: it is what the
 * layer's rectangle is measured from, and it is the listbox the address
 * field's `aria-controls` and `aria-activedescendant` point into, so a screen
 * reader follows the keys' highlight through rows with the ids it always had.
 *
 * Opened as it mounts and closed as it unmounts; new rows, a new highlight,
 * or the list moving (its size, the window's, the panes) send it again. An
 * overlay opening closes it (`onClosed`), as it is drawn under the layer. The
 * bar keeps the keyboard throughout.
 */
export function BrowserSuggestionsLayer({
  layer,
  id,
  rows,
  highlighted,
  onPick,
  onHover,
  onClosed,
  handleRef
}: BrowserSuggestionsLayerProps): JSX.Element {
  const listRef = useRef<HTMLDivElement>(null)
  // What the layer's callbacks and the sends read, as of this render. First
  // of the layout effects, so the ones below see it.
  const latest = useRef({ rows, highlighted, onPick, onHover, onClosed })
  useLayoutEffect(() => {
    latest.current = { rows, highlighted, onPick, onHover, onClosed }
  })
  // A dialog Floti opens on its own is drawn under the layer.
  useClosedByOverlay(onClosed)

  const content = (): LayerSuggestions => ({
    kind: 'suggestions',
    rows: [...latest.current.rows],
    highlighted: latest.current.highlighted
  })
  const measure = (): LayerRect | null => {
    const node = listRef.current
    if (node === null) return null
    const { left, top, width, height } = node.getBoundingClientRect()
    return { x: left, y: top, width, height }
  }

  useLayoutEffect(() => {
    const rect = measure()
    if (rect === null) return
    const opened = layer.openSuggestions(content(), rect, {
      pick: (index) => latest.current.onPick(index),
      hover: (index) => latest.current.onHover(index),
      closed: () => latest.current.onClosed()
    })
    handleRef.current = opened
    return () => {
      if (handleRef.current === opened) handleRef.current = null
      opened.close()
    }
  }, [layer, handleRef])

  const shown = JSON.stringify([rows, highlighted])
  const sent = useRef<string | null>(null)
  useLayoutEffect(() => {
    const node = listRef.current
    if (node === null) return
    const send = (): void => {
      const rect = measure()
      const handle = handleRef.current
      if (rect === null || handle === null) return
      const key = `${shown}|${JSON.stringify(rect)}`
      if (sent.current === null) {
        // The open above already sent this.
        sent.current = key
        return
      }
      if (key === sent.current) return
      sent.current = key
      handle.update(content(), rect)
    }
    send()
    window.addEventListener('resize', send)
    window.addEventListener(PANES_MOVED_EVENT, send)
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(send)
    observer?.observe(node)
    return () => {
      window.removeEventListener('resize', send)
      window.removeEventListener(PANES_MOVED_EVENT, send)
      observer?.disconnect()
    }
  }, [shown, handleRef])

  return (
    <div
      ref={listRef}
      data-browser-suggestions-window
      className="pointer-events-none absolute top-full right-0 left-0 z-30 mt-1 opacity-0"
    >
      <BrowserSuggestions id={id} rows={rows} highlighted={highlighted} onPick={onPick} onHover={onHover} />
    </div>
  )
}
