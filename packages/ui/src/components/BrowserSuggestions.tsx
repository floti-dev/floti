import type { JSX } from 'react'
import type { BrowserSuggestion } from '@helm/core/types'
import { cn } from '../lib/cn'
import { GlobeIcon, SearchIcon } from './icons'

export interface BrowserSuggestionsProps {
  /** What to offer, best first: `suggestBrowserAddresses`' rows. */
  rows: readonly BrowserSuggestion[]
  /** The row the keys or the pointer are on, or -1 for none. */
  highlighted: number
  /** A row was chosen with the pointer. */
  onPick: (index: number) => void
  /** The pointer moved onto a row. */
  onHover: (index: number) => void
  /**
   * The listbox's id. Each row's is `<id>-<index>`, for the address field's
   * `aria-activedescendant`.
   */
  id: string
}

/**
 * The address bar's suggestions: a Search row and pages from history.
 *
 * Presentational and stateless on purpose. The bar owns the keys and the
 * highlight; this draws what it is given and says what the pointer did, by
 * row index. Its props are data plus callbacks, and it measures nothing
 * outside itself, so it can be drawn away from the bar - in a layer of its
 * own over the page - as well as under it.
 *
 * The popup recipe from DESIGN.md (`Menu`): `surface-raised` behind a
 * `border-strong` edge, `rounded-well`, 4px of padding, 28px rows at 12px,
 * `hover` under the pointer or the keys, no shadow.
 */
export function BrowserSuggestions({ rows, highlighted, onPick, onHover, id }: BrowserSuggestionsProps): JSX.Element | null {
  if (rows.length === 0) return null
  return (
    <ul
      id={id}
      role="listbox"
      aria-label="Suggestions"
      data-browser-suggestions
      className="w-full rounded-well border border-border-strong bg-surface-raised p-1 text-fg"
    >
      {rows.map((row, index) => {
        const on = index === highlighted
        return (
          <li
            key={row.kind === 'search' ? 'search' : row.url}
            id={`${id}-${String(index)}`}
            role="option"
            aria-selected={on}
            data-browser-suggestion={row.kind}
            data-browser-suggestion-url={row.kind === 'page' ? row.url : undefined}
            title={row.kind === 'page' ? `${row.url}\nShift+Delete removes it from history` : undefined}
            // On the press, with its default prevented: the address field keeps
            // the caret, so its blur never closes the list before the choice.
            onMouseDown={(event) => {
              if (event.button !== 0) return
              event.preventDefault()
              onPick(index)
            }}
            onMouseMove={on ? undefined : () => onHover(index)}
            className={cn(
              'flex h-7 cursor-default items-center gap-2 rounded-raised px-2 text-[12px]',
              on ? 'bg-hover text-fg' : 'text-fg'
            )}
          >
            <span aria-hidden className="grid shrink-0 place-items-center text-fg-muted">
              {row.kind === 'search' ? <SearchIcon width={13} height={13} /> : <GlobeIcon width={13} height={13} />}
            </span>
            {row.kind === 'search' ? (
              <span className="flex min-w-0 flex-1 gap-1">
                <span className="shrink-0 text-fg-muted">Search {row.engine} for</span>
                <span className="truncate">{row.text}</span>
              </span>
            ) : row.title === '' ? (
              <span className="min-w-0 flex-1 truncate font-mono text-[11px]">{row.url}</span>
            ) : (
              <span className="flex min-w-0 flex-1 items-baseline gap-2">
                <span className="min-w-0 max-w-[60%] shrink-0 truncate">{row.title}</span>
                <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-fg-subtle">{row.url}</span>
              </span>
            )}
          </li>
        )
      })}
    </ul>
  )
}
