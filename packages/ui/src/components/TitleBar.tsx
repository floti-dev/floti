import type { JSX } from 'react'
import { cn } from '../lib/cn'
import { isMac } from '../lib/platform'
import { FlotiMarkIcon } from './icons'

/**
 * The mark's size in the strip. Larger than the rail's 20px icons: at 20 the
 * longship's sail, dragon and waves crowd into each other. 24 leaves 6px of
 * the 36px strip above and below, and keeps the mark's ink under what a
 * checked checkbox fills (DESIGN.md, the mark).
 */
export const TITLE_MARK_SIZE = 24

/**
 * The strip that replaces the native title bar, and the window's drag region.
 *
 * Each OS draws its window controls into it (main/chrome.ts, DESIGN.md 5b).
 * On Windows the Window Controls Overlay draws min/max/close at the right,
 * which `pr-36` keeps clear, and the mark sits centred over the rail below it,
 * so the two read as one column down the window's left edge. On macOS the traffic lights sit at the left instead: the first
 * 80px are left to them - nothing interactive under them, the mark included -
 * the mark moves right of them, and there is nothing at the right to keep
 * clear.
 *
 * Nothing else lives here any more. The theme switch went to Settings with the
 * rest of Appearance, and Settings went to the rail.
 */
export function TitleBar(): JSX.Element {
  const mac = isMac()
  return (
    <div className={cn('app-drag flex h-9 shrink-0 items-center', !mac && 'pr-36')}>
      {mac && <span data-traffic-lights aria-hidden className="w-20 shrink-0 self-stretch" />}
      <span className="grid w-12 shrink-0 place-items-center">
        {/* The mark alone - a wordmark beside it would say "Floti" to someone
            already looking at Floti. The accessible name stays, because the
            mark is the only thing identifying the window. */}
        <FlotiMarkIcon
          width={TITLE_MARK_SIZE}
          height={TITLE_MARK_SIZE}
          role="img"
          aria-label="Floti"
          // The icons default to `aria-hidden`, which is right when a label sits
          // beside them. Nothing does here, so this one has to be announced.
          aria-hidden={false}
          className="text-accent"
        />
      </span>
    </div>
  )
}
