import type { JSX, ReactNode, RefObject } from 'react'
import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { cn } from '../lib/cn'
import { POPUP_MARGIN, usePopup, type PopupAnchor } from '../lib/popup'
import { ChildMarkIcon, type ChildMarkView } from './ChildMark'
import { CloseIcon } from './icons'

/** How a state chip is toned: the same tones the state dots use. */
export type PeekTone = 'working' | 'needs' | 'idle' | 'danger' | 'neutral'

const TONE: Record<PeekTone, string> = {
  working: 'border-accent/40 text-accent-text',
  needs: 'border-warn/40 text-warn',
  idle: 'border-success/40 text-success',
  danger: 'border-danger/40 text-danger',
  // An ended child: DESIGN.md's state table gives `ended` fg-subtle.
  neutral: 'border-border-strong text-fg-subtle'
}

/** The room the peek's screen has inside its box, in CSS px. */
export interface PeekScreenRoom {
  /** The box's inner width: the whole width the screen is drawn across. */
  width: number
  /** The most the screen may be high: the box's cap less its border and padding. */
  maxHeight: number
}

/** The screen box is never capped lower than this, in px, however short the window. */
export const PEEK_SCREEN_MIN = 150
/** Nor higher than this, however tall the window. */
export const PEEK_SCREEN_MAX = 360

/**
 * The most the screen box may be high, border and padding included: the
 * larger of {@link PEEK_SCREEN_MIN} and what fits in the window under the
 * peek's other parts (`others`, their height together), at most
 * {@link PEEK_SCREEN_MAX}. The popover stays inside the window by
 * `POPUP_MARGIN` at its top and its bottom.
 */
export function peekScreenCap(windowHeight: number, others: number): number {
  return Math.min(PEEK_SCREEN_MAX, Math.max(PEEK_SCREEN_MIN, windowHeight - 2 * POPUP_MARGIN - others))
}

/** A length in px from a computed style, 0 where it has none. */
const px = (value: string): number => Number.parseFloat(value) || 0

export interface ChildPeekProps {
  /** The child, as every surface names it. */
  name: string
  mark: ChildMarkView
  /** What it is doing, in a word or two ("working", "needs you"). */
  state: { label: string; tone: PeekTone }
  /** The agent an agent child was started as, or null for one started as none. Ignored for a command. */
  agent: string | null
  /** A command child's command line; undefined for an agent child, which has an agent and a mode. */
  command?: string | undefined
  /** The folder it runs in, as named on screen, and the whole path for its hover text. */
  folder: string
  folderPath?: string | undefined
  /** The branch the folder was on, or null. */
  branch: string | null
  /** The permission mode it was launched in, or null. Ignored for a command. */
  mode: string | null
  /** Where a command child runs, when the window knows ("your shell, pwsh"). */
  runsIn?: string | undefined
  /** What it last told its parent: its state ("done") and its words; null when it has told nothing. A command child has no report. */
  report: { state: string; text: string } | null
  /** It has ended: the peek says so, and has no Stop. Its screen is the last one the window kept. */
  ended?: boolean | undefined
  /** Whether any of its screen reached the window. An ended child that sent none has nothing for Open to show but its tab. */
  screenSeen?: boolean | undefined
  /**
   * The child's screen, live and read-only, drawn in the room its box has:
   * across the box's width, and as high as it needs up to the box's cap, which
   * the box's height then follows. Null while there is no screen to draw yet,
   * when the box keeps the cap's floor.
   */
  screen: ((room: PeekScreenRoom) => ReactNode) | null
  /** Where the popover goes: beside a row (a point), or under a chip. */
  at: PopupAnchor
  /** The row or chip it opened from, whose press is not outside the peek. */
  anchorRef?: RefObject<HTMLElement | null> | undefined
  onClose: () => void
  /** Stop: the window closes the peek and asks before it ends the child. */
  onStop: () => Promise<void> | void
  /** Open as tab: the window gives the child a tab of its own. */
  onOpenAsTab: () => void
  /**
   * Open, for a running child the user closed out of its parent's family
   * view: the window puts it back there. Absent for any other child, which
   * has no place in a family view to go back to and only Open as tab.
   */
  onOpen?: (() => void) | undefined
}

const BUTTON = 'rounded-well border px-[9px] py-[3px] text-[11.5px] font-medium whitespace-nowrap transition-colors'

/**
 * The peek: a popover beside the row, or under the crumb chip, it was opened
 * from, for a child that has nowhere on screen here. It shows the
 * child's screen live and read-only, who it is (its agent, or its command), its
 * folder, branch and mode, what it last reported, and Stop and Open as tab,
 * with Open after them for a running child closed out of its parent's family
 * view. Open never makes a tab: it puts the child back in that view. A child
 * that has ended gets a peek too, which says so and has no Stop.
 *
 * The screen box spans the peek's width, and its height follows the screen
 * drawn in it up to its cap ({@link peekScreenCap}), which it measures from the
 * window and the peek's other parts and hands the screen as its room.
 *
 * It is an overlay (`usePopup`), so a browser tab's native view stands down for
 * it; Escape and its close button close it, and so does a press outside it. Stop
 * is the window's: it closes the peek as its question opens. Focus goes to the close
 * button when it opens and back to what had it when it closes, which is the row
 * or chip the user pressed.
 *
 * Drawing only: the screen is a slot, and every button is the window's.
 */
export function ChildPeek({
  name,
  mark,
  state,
  agent,
  command,
  folder,
  folderPath,
  branch,
  mode,
  runsIn,
  report,
  ended = false,
  screenSeen = true,
  screen,
  at,
  anchorRef,
  onClose,
  onStop,
  onOpenAsTab,
  onOpen
}: ChildPeekProps): JSX.Element {
  const ref = useRef<HTMLDivElement>(null)
  const boxRef = useRef<HTMLDivElement>(null)
  const closeRef = useRef<HTMLButtonElement>(null)
  const titleId = useId()
  const place = usePopup({ ref, at, anchorRef, onDismiss: onClose })
  const [room, setRoom] = useState<PeekScreenRoom | null>(null)

  // The screen's room: the box's inner width, and its cap less its edges. Read
  // again whenever the peek or the box changes size, as when a report arrives;
  // the parts other than the box do not change with it, so the cap holds still.
  useLayoutEffect(() => {
    const peek = ref.current
    const box = boxRef.current
    if (peek === null || box === null) return
    const measure = (): void => {
      const style = getComputedStyle(box)
      const width = Math.max(0, box.clientWidth - px(style.paddingLeft) - px(style.paddingRight))
      const edges = box.offsetHeight - box.clientHeight + px(style.paddingTop) + px(style.paddingBottom)
      const cap = peekScreenCap(window.innerHeight, peek.offsetHeight - box.offsetHeight)
      const maxHeight = Math.max(0, cap - edges)
      setRoom((last) => (last?.width === width && last.maxHeight === maxHeight ? last : { width, maxHeight }))
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const observer = new ResizeObserver(measure)
    observer.observe(peek)
    observer.observe(box)
    return () => observer.disconnect()
  }, [])

  // Focus: the close button while it is open, and back where it came from.
  useEffect(() => {
    const before = document.activeElement
    closeRef.current?.focus({ preventScroll: true })
    return () => {
      if (!(before instanceof HTMLElement) || !before.isConnected) return
      const now = document.activeElement
      // The peek is gone, so focus it held fell to the body.
      if (now === null || now === document.body) before.focus({ preventScroll: true })
    }
  }, [])

  // Escape closes it.
  useEffect(() => {
    const onKey = (event: KeyboardEvent): void => {
      if (event.key !== 'Escape' || event.defaultPrevented) return
      event.preventDefault()
      onClose()
    }
    document.addEventListener('keydown', onKey)
    return () => document.removeEventListener('keydown', onKey)
  }, [onClose])

  const rows: Array<{ label: string; value: string; title?: string | undefined; strong?: boolean }> =
    command === undefined
      ? [
          { label: 'Agent', value: agent ?? 'none' },
          { label: 'Folder', value: folder, title: folderPath },
          { label: 'Branch', value: branch ?? 'none' },
          { label: 'Mode', value: mode ?? 'default' }
        ]
      : [
          { label: 'Command', value: command, title: command, strong: true },
          { label: 'Folder', value: folder, title: folderPath },
          { label: 'Branch', value: branch ?? 'none' },
          ...(runsIn === undefined ? [] : [{ label: 'Runs in', value: runsIn }])
        ]

  return (
    <div
      ref={ref}
      role="dialog"
      aria-modal="false"
      aria-labelledby={titleId}
      data-child-peek={name}
      style={{
        position: 'fixed',
        left: place?.left ?? 0,
        top: place?.top ?? 0,
        visibility: place === null ? 'hidden' : 'visible'
      }}
      className="z-50 flex w-[470px] max-w-[calc(100vw-16px)] flex-col rounded-well border border-border-strong bg-surface-raised"
    >
      <div className="flex items-center gap-2 px-3 pt-2.5 pb-2">
        <ChildMarkIcon mark={mark} />
        <span id={titleId} className="min-w-0 truncate text-[13px] font-medium text-fg">
          {name}
        </span>
        <span
          data-peek-state={state.tone}
          className={cn('shrink-0 rounded-full border px-[7px] py-px text-[10.5px] font-medium', TONE[state.tone])}
        >
          {state.label}
        </span>
        <button
          ref={closeRef}
          type="button"
          aria-label="Close peek"
          onClick={onClose}
          className="ml-auto grid size-[22px] shrink-0 place-items-center rounded-well text-fg-subtle transition-colors hover:bg-hover hover:text-fg"
        >
          <CloseIcon width={11} height={11} />
        </button>
      </div>

      <dl className="m-0 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5 px-3 pb-2 text-[11.5px]">
        {rows.map((row) => (
          <div key={row.label} className="contents">
            <dt className="text-fg-subtle">{row.label}</dt>
            <dd
              title={row.title}
              className={cn(
                'm-0 truncate font-mono text-[11px]',
                row.strong === true ? 'text-fg' : 'text-fg-muted'
              )}
            >
              {row.value}
            </dd>
          </div>
        ))}
      </dl>

      <div
        ref={boxRef}
        data-peek-screen
        className={cn(
          'relative mx-3 overflow-hidden rounded-raised border border-border bg-terminal px-2.5 py-[9px]',
          screen === null && 'h-[150px]'
        )}
      >
        {screen !== null && room !== null && screen(room)}
      </div>

      {report !== null && command === undefined && (
        <div
          data-peek-report
          className="mx-3 mt-2 rounded-r-raised bg-accent-soft px-2.5 py-2 text-[11.5px] leading-[1.45] text-fg shadow-[inset_2px_0_0_var(--color-accent)]"
        >
          <span className="mb-0.5 block text-[10px] leading-[1.4] font-semibold tracking-[0.07em] text-fg-subtle uppercase">
            Last report · {report.state}
          </span>
          <span>{report.text}</span>
        </div>
      )}

      <div className="flex items-center gap-1.5 px-3 py-2.5">
        <span className="mr-auto text-[11px] text-fg-subtle">
          {ended
            ? screenSeen
              ? 'Ended. Open as tab shows its last screen.'
              : 'Ended. This window saw none of its screen.'
            : onOpen === undefined
              ? 'Watching only. Open it as a tab to type.'
              : 'Watching only. Open it to type.'}
        </span>
        {!ended && (
          <button
            type="button"
            data-peek-stop
            onClick={() => void onStop()}
            className={cn(BUTTON, 'border-danger/45 text-danger hover:bg-danger/10')}
          >
            Stop
          </button>
        )}
        {/* The accent outline marks the press the peek is for: Open where there is one, Open as tab where it is alone. */}
        <button
          type="button"
          data-peek-open-tab
          onClick={onOpenAsTab}
          className={cn(
            BUTTON,
            onOpen === undefined
              ? 'border-accent text-accent-text hover:bg-accent-soft'
              : 'border-border-strong text-fg hover:bg-hover'
          )}
        >
          Open as tab
        </button>
        {onOpen !== undefined && (
          <button
            type="button"
            data-peek-open
            onClick={onOpen}
            className={cn(BUTTON, 'border-accent text-accent-text hover:bg-accent-soft')}
          >
            Open
          </button>
        )}
      </div>
    </div>
  )
}
