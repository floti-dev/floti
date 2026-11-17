import type { JSX, RefObject } from 'react'
import { useCallback, useEffect, useEffectEvent, useId, useLayoutEffect, useRef, useState } from 'react'
import type { BrowserSuggestion, BrowserSuggestions as Suggestions } from '@floti/core/types'
import { cn } from '../lib/cn'
import { PANES_MOVED_EVENT } from '../lib/paneGeometry'
import { isMac, shortcutLabel } from '../lib/platform'
import { SEGMENT_ON } from '../lib/segmented'
import type { MenuLayerPort, SuggestionsLayerHandle } from '../lib/menuLayer'
import { BrowserSuggestionsLayer } from './BrowserSuggestionsLayer'
import { ConsolePanel, type ConsoleEntry } from './ConsolePanel'
import { EmptyState } from './EmptyState'
import {
  AgentIcon,
  BackIcon,
  CloseIcon,
  ExternalIcon,
  ForwardIcon,
  GlobeIcon,
  MoreIcon,
  RefreshIcon,
  SearchIcon,
  ShareIcon
} from './icons'
import { Menu, type MenuEntry } from './Menu'

/** Everything the pane knows about the view behind it. Mirrors `BrowserState`. */
export interface BrowserPaneState {
  id: number
  /** The committed address. */
  url: string
  /** The address being loaded from the bar or an agent, until it commits, fails or is stopped. */
  pendingUrl: string | null
  /** The address the tab was asked for and refused, while it has loaded nothing. */
  refusedUrl: string | null
  title: string
  host: string
  canGoBack: boolean
  canGoForward: boolean
  loading: boolean
  problem: string | null
  retryingUntil: number | null
  zoomLevel: number
  errors: number
  devtoolsOpen: boolean
  find: { query: string; matches: number; active: number } | null
  project: string | null
  /** The session that opened the page, or null when the user did. */
  openedBy: string | null
  /** Whether that session is still running, and so can still drive the page. */
  openerRunning: boolean
  /** The session the user has shared the page with, or null. */
  sharedWith: BrowserShareTarget | null
}

/** A session a page is shared with, or could be. Its id, and what it is called. */
export interface BrowserShareTarget {
  session: number
  name: string
}

/** The width presets, in CSS pixels. `null` is "whatever the pane is". */
export const BROWSER_WIDTHS: Array<[string, number | null]> = [
  ['Full', null],
  ['Tablet', 820],
  ['Phone', 390]
]

/**
 * Chromium zoom levels: a step is half a level (about 10%), and main holds
 * the page between -3 and 3 (58% to 173%).
 */
export const BROWSER_ZOOM = { step: 0.5, min: -3, max: 3 } as const

export interface BrowserPaneProps {
  state: BrowserPaneState
  entries: readonly ConsoleEntry[]
  /**
   * Where the view should be, in CSS pixels relative to the window, and
   * whether it should paint.
   *
   * The pane measures its own placeholder and hands the rectangle up; the
   * caller adds the reasons to hide that the pane cannot know about - a
   * dialog, the workspace column collapsed, a tab being dragged - and sends it
   * to main. The pane is deliberately not the place those reasons are
   * collected: it is mounted only while it is the active tab, so it cannot see
   * three of the four.
   */
  onBounds: (rect: { x: number; y: number; width: number; height: number }) => void
  /**
   * Send the page somewhere. When it returns a promise, the bar keeps what was
   * entered until it settles - the moment main's answer, with the address being
   * loaded, is in `state` - so no frame shows the old address in between.
   */
  onNavigate: (input: string) => void | Promise<void>
  onBack: () => void
  onForward: () => void
  onReload: (hard: boolean) => void
  /** Stop the page's load. */
  onStop: () => void
  onDevTools: () => void
  onOpenExternal: (url: string) => void
  onFind: (query: string, forward: boolean) => void
  onStopFind: () => void
  onZoom: (level: number) => void
  onClearStorage: () => void
  onEvaluate: (source: string) => Promise<{ ok: boolean; value: string; error: string | null }>
  /**
   * Whether the console is open. Held by the window, so the console key
   * (`shared/browserKeys.ts`) and the More menu's item are one path. Closed, the
   * pane draws no console at all - no row, no count.
   */
  consoleOpen: boolean
  onToggleConsole: () => void
  /**
   * The browser's menu layer: a view above the page where the More and Share
   * menus and the address suggestions are drawn. They hang over the page, and
   * a native view paints above all of the window's DOM, so drawn in the window
   * they would be covered; drawn in the layer, the page keeps running and
   * painting behind them and nothing stands down.
   */
  layer: MenuLayerPort
  /**
   * The suggestions for what is typed, from the browser's history ('' for the
   * pages visited last). Ranked in main; nothing leaves the machine.
   */
  onSuggest: (text: string) => Promise<Suggestions>
  /** Forget one page from the history: Shift+Delete on its row. */
  onForget: (url: string) => void
  /** Forget the whole history: the More menu's Clear browsing history. */
  onClearHistory: () => void
  /**
   * Somewhere the caret should go: the address bar (Ctrl+L, a new page) or
   * the find field (Ctrl+F). A request rather than a counter, answered once
   * with `onFocusRequestDone`, because the pane is rebuilt on every page
   * switch and a counter it read on mounting would pull the caret into the
   * address bar each time a page came to the front.
   */
  focusRequest: 'address' | 'find' | null
  onFocusRequestDone: () => void
  /** The engine a phrase is searched with, for the placeholder, or null when searching is off. */
  searchEngine: string | null
  /**
   * A picture of the page, painted in the hole while the view stands down for
   * something drawn over it (`useBrowsers`), or null.
   */
  still: string | null
  /**
   * Whether a page can be shared with a session at all: the browser tools are
   * on. A page an agent opened, which is its session's already, and an empty
   * one never offer it.
   */
  canShare: boolean
  /** The sessions this page could be shared with, asked for as the Share menu opens. */
  onShareTargets: () => Promise<readonly BrowserShareTarget[]>
  /** Share the page with a session, or (`null`) take it back. */
  onShare: (session: number | null) => void
}

/**
 * A page with nothing in it: a new tab that has not been sent anywhere, is
 * loading nothing, and has no refusal or problem to show. Its view is clear
 * (main grounds it white only on its first navigation), so the window hides
 * it and the pane says the Browser tab's empty state where the page would be.
 */
export function browserPageBlank(
  state: Pick<BrowserPaneState, 'url' | 'pendingUrl' | 'refusedUrl' | 'loading' | 'problem'>
): boolean {
  return (
    state.url === '' &&
    state.pendingUrl === null &&
    state.refusedUrl === null &&
    !state.loading &&
    state.problem === null
  )
}

/**
 * The Browser tab with no page to show (DESIGN.md 5, "Empty states"). Open a
 * page puts the caret in an address bar: the blank page's own, or a new page's.
 */
export function BrowserEmpty({ onOpen }: { onOpen: () => void }): JSX.Element {
  return (
    <EmptyState
      name="browser"
      icon={<GlobeIcon width={18} height={18} />}
      title="No pages open"
      action={{ label: 'Open a page', onClick: onOpen }}
    >
      Type an address, or let an agent open one.
    </EmptyState>
  )
}

/**
 * The browser pane: an address-bar island, a hole for the native view, and a
 * console under it.
 *
 * **The hole is the design.** A `WebContentsView` paints above all renderer
 * DOM, so the middle of this pane is not a component at all - it is a `div`
 * whose only job is to have a rectangle, measured and sent to the main process
 * so the view can be put exactly there. Everything drawn here is drawn *around*
 * it. That is also why the placeholder carries no border, no radius and no
 * ground of its own: the view's ground is the page's, which is foreign ground
 * (DESIGN.md 6), and a rounded corner Floti drew would be a rounded corner the
 * page paints straight over.
 *
 * **It never reaches the top 36px.** The window hides the native title bar and
 * lets Windows draw the min/max/close buttons there, and a native view is above
 * those too. The layout keeps well clear - a tab strip and this island are
 * between - and main clamps the rectangle anyway, because a layout is not a
 * guarantee.
 */
export function BrowserPane({
  state,
  entries,
  onBounds,
  onNavigate,
  onBack,
  onForward,
  onReload,
  onStop,
  onDevTools,
  onOpenExternal,
  onFind,
  onStopFind,
  onZoom,
  onClearStorage,
  onEvaluate,
  consoleOpen,
  onToggleConsole,
  layer,
  onSuggest,
  onForget,
  onClearHistory,
  focusRequest,
  onFocusRequestDone,
  searchEngine,
  still,
  canShare,
  onShareTargets,
  onShare
}: BrowserPaneProps): JSX.Element {
  const holeRef = useRef<HTMLDivElement>(null)
  const addressRef = useRef<HTMLInputElement>(null)
  const findRef = useRef<HTMLInputElement>(null)
  const moreRef = useRef<HTMLButtonElement>(null)
  const shareRef = useRef<HTMLButtonElement>(null)
  /**
   * What is in the address bar, as an override rather than a copy.
   *
   * Null means "whatever the page is", so the bar follows a redirect, a retry
   * that finally connected and a `history.pushState` without anything having to
   * copy the URL into state - and a keystroke is not overwritten mid-word by a
   * page that navigated itself. Typing sets the override; committing, Escape
   * and blurring clear it. Derived, so there is no effect syncing two values
   * that can disagree.
   */
  const [draft, setDraft] = useState<string | null>(null)
  /**
   * What was entered, from Enter until main has answered. Main's answer
   * carries the address being loaded (`pendingUrl`); until it is here, this is
   * the only thing that knows where the page is going, and without it the bar
   * would show the old address - or nothing, on a new tab - in between.
   * `submissions` makes sure only the newest answer clears it.
   */
  const [submitted, setSubmitted] = useState<string | null>(null)
  const submissions = useRef(0)
  /** The page's committed address, or the one refused before any loaded. */
  const address = state.url !== '' ? state.url : (state.refusedUrl ?? '')
  /** What the address bar says when nobody is typing: where the page is going, or where it is. */
  const shown = submitted ?? state.pendingUrl ?? address
  const typed = draft ?? shown
  /** A new tab nothing has been sent to yet: the empty state stands where the page would be. */
  const blank = submitted === null && browserPageBlank(state)
  const [suggesting, setSuggesting] = useState(false)
  /** The newest answer to `onSuggest`, for the text it names. */
  const [suggestions, setSuggestions] = useState<Suggestions | null>(null)
  /**
   * The row the keys or the pointer are on. Only the keys put a row's address
   * in the bar and make it what Enter opens; the pointer only highlights it.
   */
  const [highlight, setHighlight] = useState<Highlight>(NO_HIGHLIGHT)
  /**
   * Whether the inline completion is held back: after a deletion, so
   * Backspace takes the completion away rather than it coming straight back,
   * and after Escape restored what was typed.
   */
  const [holdCompletion, setHoldCompletion] = useState(false)
  const listId = useId()
  /** What the suggestions are asked for: what is typed, or nothing while the bar shows the page. */
  const query = draft ?? ''
  const rows: readonly BrowserSuggestion[] = suggestions?.rows ?? []
  const chosen = highlight.by === 'keys' ? rows[highlight.index] : undefined
  /**
   * The completion shown after the caret, only while the list is up. It is
   * the newest answer's, carried forward while what is typed still runs along
   * it, so typing the next letter of a completed address does not drop the
   * completion for the moment until main answers again.
   */
  const completion = ((): Suggestions['completion'] => {
    if (!suggesting || holdCompletion || chosen !== undefined || draft === null) return null
    if (suggestions === null || suggestions.completion === null) return null
    const { text, completion: answered } = suggestions
    const full = text + answered.rest
    if (draft.length < text.length || !full.toLowerCase().startsWith(draft.toLowerCase())) return null
    return { url: answered.url, rest: full.slice(draft.length) }
  })()
  /** What is typed with its completion after it, or null when none is shown. */
  const completed = completion !== null && draft !== null ? draft + completion.rest : null
  /** What the bar holds: the row the keys are on, what is typed and its completion, or the page. */
  const barValue = chosen !== undefined ? rowText(chosen) : (completed ?? typed)

  /**
   * The list closes and forgets its rows, so the bar opened again shows
   * nothing until the answer for what it then holds arrives, never the rows
   * for what was typed before.
   */
  const closeSuggestions = (): void => {
    setSuggesting(false)
    setSuggestions(null)
    setHighlight(NO_HIGHLIGHT)
  }

  /** Send the page where the bar says, and keep saying it until main has answered. */
  const go = (input: string): void => {
    const turn = ++submissions.current
    setSubmitted(input)
    setDraft(null)
    closeSuggestions()
    const answered = (): void => {
      if (submissions.current === turn) setSubmitted(null)
    }
    void Promise.resolve(onNavigate(input)).then(answered, answered)
  }
  const [finding, setFinding] = useState(false)
  const [findQuery, setFindQuery] = useState('')
  const [width, setWidth] = useState<number | null>(null)
  /** The size the page was when its still was taken, in CSS pixels. */
  const [stillSize, setStillSize] = useState<{ width: number; height: number } | null>(null)
  const [menuAt, setMenuAt] = useState<DOMRect | null>(null)
  const [shareMenu, setShareMenu] = useState<{ at: DOMRect; targets: readonly BrowserShareTarget[] } | null>(null)

  // Ctrl+F opens the find field before the effect below can put the caret in
  // it: adjusted while rendering, as React has state follow a prop.
  if (focusRequest === 'find' && !finding) setFinding(true)

  // The caret, wherever it was asked for. The effect touches the DOM and
  // answers the request, which is what an effect is for.
  useEffect(() => {
    if (focusRequest === null) return
    const field = focusRequest === 'find' ? findRef.current : addressRef.current
    field?.focus()
    field?.select()
    onFocusRequestDone()
  }, [focusRequest, onFocusRequestDone])

  const showingList = suggesting && rows.length > 0
  /** The suggestions open in the menu layer, for the bar to say it lost the keyboard. */
  const suggestionsLayer = useRef<SuggestionsLayerHandle | null>(null)

  /*
   * The suggestions for what is typed, asked whenever it changes while the
   * bar has the caret. An answer that arrives after the text moved on is
   * dropped; the rows on screen stay until the next one lands, so the list
   * does not blink on every keystroke.
   */
  const suggest = useEffectEvent((text: string) => onSuggest(text))
  useEffect(() => {
    if (!suggesting) return
    let current = true
    suggest(query).then(
      (answer) => {
        if (current && answer.text === query) setSuggestions(answer)
      },
      () => undefined
    )
    return () => {
      current = false
    }
  }, [suggesting, query])

  /*
   * The completion is selected, so the next character typed replaces it and
   * Backspace removes it. Keyed on what is shown, so it is selected once as
   * it appears and a caret the user has moved since is left where it is.
   */
  const completedFrom = draft?.length ?? 0
  useLayoutEffect(() => {
    if (completed === null) return
    addressRef.current?.setSelectionRange(completedFrom, completed.length)
  }, [completed, completedFrom])

  /** Open what a row offers: its page, or a search for what was typed. */
  const pick = (row: BrowserSuggestion): void => {
    addressRef.current?.blur()
    go(row.kind === 'page' ? row.url : row.text)
  }

  /**
   * Move the keys' highlight through the rows, from the row the keys or the
   * pointer are on, and back to what was typed past either end.
   */
  const step = (by: 1 | -1): void => {
    if (rows.length === 0) return
    const next = highlight.index + by
    setHighlight({ index: next < -1 ? rows.length - 1 : next >= rows.length ? -1 : next, by: 'keys' })
  }

  /** Shift+Delete on a page's row: it leaves the history and the list at once. */
  const forgetChosen = (): boolean => {
    if (chosen === undefined || chosen.kind !== 'page' || suggestions === null) return false
    onForget(chosen.url)
    const left = suggestions.rows.filter((row) => row !== chosen)
    setSuggestions({
      ...suggestions,
      rows: left,
      completion: suggestions.completion?.url === chosen.url ? null : suggestions.completion
    })
    setHighlight(left.length === 0 ? NO_HIGHLIGHT : { index: Math.min(highlight.index, left.length - 1), by: 'keys' })
    return true
  }

  /**
   * The rectangle, measured and reported.
   *
   * `useLayoutEffect` for the first one so the view is placed in the same frame
   * the pane appears in - an effect would put the view one frame behind, which
   * on a tab switch is a visible flash of the page in the wrong place. After
   * that a `ResizeObserver` on the placeholder catches a divider drag, the
   * window resize and the console opening; a scroll or a layout change that
   * moves the pane without resizing it is caught by the window listeners, since
   * `ResizeObserver` fires on size and not on position. `PANES_MOVED_EVENT` is
   * the pane grid saying it has just placed every pane, which is the one way a
   * pane moves without the window or anything in it scrolling.
   */
  const report = useCallback(() => {
    const box = holeRef.current?.getBoundingClientRect()
    if (box === undefined) return
    onBounds({ x: box.x, y: box.y, width: box.width, height: box.height })
  }, [onBounds])

  useLayoutEffect(() => {
    report()
    const hole = holeRef.current
    if (hole === null) return
    const observer = new ResizeObserver(() => report())
    observer.observe(hole)
    window.addEventListener('resize', report)
    window.addEventListener('scroll', report, true)
    window.addEventListener(PANES_MOVED_EVENT, report)
    return () => {
      observer.disconnect()
      window.removeEventListener('resize', report)
      window.removeEventListener('scroll', report, true)
      window.removeEventListener(PANES_MOVED_EVENT, report)
    }
  }, [report])

  // A width preset changes the placeholder's box, which the observer above
  // turns into a `setBounds`. Reported here as well so the first frame after a
  // preset is picked is already right.
  useLayoutEffect(() => {
    report()
  }, [width, consoleOpen, report])

  // The still keeps the size it was taken at, so a pane resized while the view
  // is down - a divider being dragged - crops the picture or shows the canvas
  // beside it, as a page does before it reflows, rather than squeezing it.
  useLayoutEffect(() => {
    const hole = holeRef.current
    setStillSize(still === null || hole === null ? null : { width: hole.clientWidth, height: hole.clientHeight })
  }, [still])

  const retrying = state.retryingUntil !== null
  const zoomed = state.zoomLevel !== 0

  /**
   * The overflow menu: what a browser keeps out of its bar. Zoom, the width
   * presets, DevTools and clearing the profile were each a control in the bar
   * once, and the bar read as a dashboard rather than a browser. The keys each
   * row names work from the page too (`shared/browserKeys.ts`).
   */
  const menu: MenuEntry[] = [
    { kind: 'heading', id: 'zoom-heading', label: `Zoom ${zoomPercent(state.zoomLevel)}` },
    { kind: 'item', id: 'zoom-in', label: 'Zoom in', hint: shortcutLabel('=', { separator: ' ' }), disabled: state.zoomLevel >= BROWSER_ZOOM.max },
    { kind: 'item', id: 'zoom-out', label: 'Zoom out', hint: shortcutLabel('-', { separator: ' ' }), disabled: state.zoomLevel <= BROWSER_ZOOM.min },
    { kind: 'item', id: 'zoom-reset', label: 'Actual size', hint: shortcutLabel('0', { separator: ' ' }), disabled: !zoomed },
    { kind: 'separator', id: 'after-zoom' },
    { kind: 'heading', id: 'width-heading', label: 'Width' },
    ...BROWSER_WIDTHS.map(
      ([label, px]): MenuEntry => ({
        kind: 'item',
        id: `width:${label}`,
        label,
        hint: px === null ? undefined : `${String(px)} px`,
        checked: width === px
      })
    ),
    { kind: 'separator', id: 'after-width' },
    {
      kind: 'item',
      id: 'console',
      label: consoleOpen ? 'Close console' : 'Open console',
      hint: shortcutLabel('J', { alt: isMac(), shift: !isMac(), separator: ' ' })
    },
    {
      kind: 'item',
      id: 'devtools',
      label: state.devtoolsOpen ? 'Close DevTools' : 'Open DevTools',
      hint: 'F12'
    },
    {
      kind: 'item',
      id: 'clear-history',
      label: 'Clear browsing history',
      title: 'Every page the address bar suggests'
    },
    {
      kind: 'item',
      id: 'clear-storage',
      label: 'Clear cookies and site data',
      title: "Every site's, in Floti's browser: you will be signed out of all of them"
    }
  ]

  /**
   * Sharing: one session at a time may read and drive a page of the user's.
   * The sessions are asked for as the menu opens rather than kept, because
   * which ones can take a page - running, and started with the tools - is main's
   * to know and changes under the window.
   */
  const shared = state.sharedWith
  const sharable = canShare && state.openedBy === null && state.url !== ''
  const openShare = (): void => {
    if (shareMenu !== null) {
      setShareMenu(null)
      return
    }
    const at = shareRef.current?.getBoundingClientRect()
    if (at === undefined) return
    void onShareTargets().then(
      (targets) => setShareMenu({ at, targets }),
      () => setShareMenu({ at, targets: [] })
    )
  }
  const shareEntries: MenuEntry[] =
    shareMenu === null
      ? []
      : [
          { kind: 'heading', id: 'share-heading', label: 'Share with' },
          ...(shareMenu.targets.length === 0
            ? [
                {
                  kind: 'item' as const,
                  id: 'share-none',
                  label: 'No session to share with',
                  disabled: true,
                  title: 'A Claude session Floti started while its browser tools were on can be given this page.'
                }
              ]
            : shareMenu.targets.map(
                (target): MenuEntry => ({
                  kind: 'item',
                  id: `share:${String(target.session)}`,
                  label: target.name,
                  checked: shared?.session === target.session
                })
              )),
          ...(shared === null
            ? []
            : [
                { kind: 'separator' as const, id: 'share-separator' },
                { kind: 'item' as const, id: 'share-stop', label: 'Stop sharing' }
              ])
        ]
  const chooseShare = (id: string): void => {
    if (id === 'share-stop') onShare(null)
    else if (id.startsWith('share:')) {
      const session = Number(id.slice('share:'.length))
      if (session !== shared?.session) onShare(session)
    }
  }

  /** Who besides the user can drive this page, said above it. */
  const driver =
    shared !== null
      ? { text: `Shared with “${shared.name}”: that session can read and drive this page.`, stop: true }
      : state.openedBy !== null && state.openerRunning
        ? { text: `“${state.openedBy}” opened this page and can drive it.`, stop: false }
        : null

  const choose = (id: string): void => {
    if (id === 'zoom-in') onZoom(state.zoomLevel + BROWSER_ZOOM.step)
    else if (id === 'zoom-out') onZoom(state.zoomLevel - BROWSER_ZOOM.step)
    else if (id === 'zoom-reset') onZoom(0)
    else if (id === 'console') onToggleConsole()
    else if (id === 'devtools') onDevTools()
    else if (id === 'clear-storage') onClearStorage()
    else if (id === 'clear-history') {
      onClearHistory()
      setSuggestions(null)
    }
    else if (id.startsWith('width:')) setWidth(BROWSER_WIDTHS.find(([label]) => `width:${label}` === id)?.[1] ?? null)
  }

  return (
    <div data-pane="browser" className="flex h-full min-h-0 flex-col">
      <div
        data-browser-bar
        data-browser-suggestion-count={showingList ? rows.length : 0}
        className="flex min-h-strip shrink-0 items-center gap-2 border-b border-border px-2 py-1"
      >
        <div className="flex shrink-0 items-center gap-0.5">
          <BarButton label="Back" disabled={!state.canGoBack} onClick={onBack} data-browser="back">
            <BackIcon width={14} height={14} />
          </BarButton>
          <BarButton
            label="Forward"
            disabled={!state.canGoForward}
            onClick={onForward}
            data-browser="forward"
          >
            <ForwardIcon width={14} height={14} />
          </BarButton>
          {/* Stop while the page loads, reload once it has, in one place, as
              every browser has it. Shift-click is the hard reload, which is the
              binding every browser already has - and the title says so, because
              a modifier nobody is told about is a feature nobody has. */}
          {state.loading ? (
            <BarButton label="Stop loading" onClick={onStop} data-browser="stop">
              <CloseIcon width={14} height={14} />
            </BarButton>
          ) : (
            <BarButton
              label="Reload (hold Shift to ignore the cache)"
              onClick={(event) => onReload(event.shiftKey)}
              data-browser="reload"
            >
              <RefreshIcon width={14} height={14} />
            </BarButton>
          )}
        </div>

        <div className="relative flex min-w-0 flex-1 items-center">
          <input
            ref={addressRef}
            data-browser-address
            value={barValue}
            spellCheck={false}
            placeholder={
              searchEngine === null ? 'Address, or a port number' : `Search ${searchEngine} or type an address`
            }
            aria-label="Address"
            role="combobox"
            aria-autocomplete="both"
            aria-expanded={showingList}
            aria-controls={showingList ? listId : undefined}
            aria-activedescendant={chosen === undefined ? undefined : `${listId}-${String(highlight.index)}`}
            onChange={(event) => {
              setDraft(event.target.value)
              setSuggesting(true)
              setHighlight(NO_HIGHLIGHT)
              // A deletion holds the completion back, so Backspace takes it
              // away; typing on brings it back.
              const input = (event.nativeEvent as Partial<InputEvent>).inputType
              setHoldCompletion(input?.startsWith('delete') === true)
            }}
            onFocus={() => setSuggesting(true)}
            // And on the click itself, not only on the focus it causes.
            // Clicking a bar that already has the caret fires no focus event, so
            // focus alone would leave the list shut for exactly the gesture
            // somebody makes when they want it. A probe that drove the click and
            // nothing else is what caught this.
            onClick={() => setSuggesting(true)}
            // Not after a delay: a delayed close outlived the blur Enter makes:
            // back in the bar within it, the list opened, then shut under the
            // click. Focus moving within the window closes the list here. The
            // window losing the keyboard while the list is up is the menu
            // layer's to judge: a press on the list takes it for a moment and
            // hands it straight back, which is no close; anywhere else closes
            // it (`onClosed`).
            onBlur={() => {
              if (suggestionsLayer.current !== null && !document.hasFocus()) suggestionsLayer.current.blurred()
              else closeSuggestions()
            }}
            onKeyDown={(event) => {
              if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
                event.preventDefault()
                if (!showingList) setSuggesting(true)
                else step(event.key === 'ArrowDown' ? 1 : -1)
                return
              }
              if (event.key === 'Delete' && event.shiftKey) {
                if (forgetChosen()) event.preventDefault()
                return
              }
              if (event.key === 'Escape') {
                // First what was typed comes back, from a row the keys are on
                // or from a completion. Then, with something typed, Escape puts
                // the address back; with nothing typed, it stops a load, as it
                // does in the page.
                if (chosen !== undefined || completion !== null) {
                  setHighlight(NO_HIGHLIGHT)
                  setHoldCompletion(true)
                  return
                }
                if (draft === null && state.loading) onStop()
                setDraft(null)
                setSuggesting(false)
                addressRef.current?.blur()
                return
              }
              if (event.key !== 'Enter') return
              event.preventDefault()
              if (chosen !== undefined) {
                pick(chosen)
                return
              }
              addressRef.current?.blur()
              // A completed address opens as the page was visited, scheme and all.
              go(completion !== null ? completion.url : typed)
            }}
            className={cn(
              'min-w-0 flex-1 rounded-well border border-border bg-surface-sunken py-1 pl-2',
              zoomed ? 'pr-14' : 'pr-2',
              'font-mono text-[12px] text-fg outline-none transition-colors',
              'placeholder:font-sans placeholder:text-fg-subtle',
              'focus:border-border-strong'
            )}
          />
          {/* The zoom, said where a browser says it - in the address bar, only
              when it is not 100% - and a click away from 100% again. */}
          {zoomed && (
            <button
              type="button"
              data-browser="zoom-reset"
              data-browser-zoom={String(state.zoomLevel)}
              title={`Zoomed - back to actual size (${shortcutLabel('0', { separator: ' ' })})`}
              aria-label={`Zoom ${zoomPercent(state.zoomLevel)}, back to actual size`}
              onClick={() => onZoom(0)}
              className="absolute top-1/2 right-1 -translate-y-1/2 rounded-raised px-1.5 py-0.5 font-mono text-[11px] tabular-nums text-fg-muted transition-colors hover:bg-hover hover:text-fg"
            >
              {zoomPercent(state.zoomLevel)}
            </button>
          )}
          {showingList && (
            <BrowserSuggestionsLayer
              layer={layer}
              handleRef={suggestionsLayer}
              id={listId}
              rows={rows}
              highlighted={highlight.index}
              onPick={(index) => {
                const row = rows[index]
                if (row !== undefined) pick(row)
              }}
              onHover={(index) => setHighlight({ index, by: 'pointer' })}
              onClosed={closeSuggestions}
            />
          )}
        </div>

        <div className="flex shrink-0 items-center gap-0.5">
          {sharable && (
            <button
              type="button"
              ref={shareRef}
              data-browser="share"
              data-browser-shared={shared === null ? undefined : String(shared.session)}
              title={
                shared === null
                  ? 'Let a session read and drive this page'
                  : `Shared with “${shared.name}”`
              }
              aria-label={shared === null ? 'Share with a session' : `Shared with ${shared.name}`}
              aria-haspopup="menu"
              aria-expanded={shareMenu !== null}
              onClick={openShare}
              className={cn(
                'mr-0.5 flex h-7 shrink-0 items-center gap-1.5 rounded-well px-2 text-[12px] transition-colors',
                shareMenu !== null
                  ? cn(SEGMENT_ON, shared === null ? 'text-fg' : 'text-accent-text')
                  : shared === null
                    ? 'text-fg-muted hover:bg-hover hover:text-fg'
                    : 'text-accent-text hover:bg-hover'
              )}
            >
              <ShareIcon width={13} height={13} />
              {/* As wide as its longer word either way, so sharing does not
                  nudge the address bar. */}
              <span className="grid">
                <span aria-hidden="true" className="invisible col-start-1 row-start-1">
                  Shared
                </span>
                <span className="col-start-1 row-start-1">{shared === null ? 'Share' : 'Shared'}</span>
              </span>
            </button>
          )}
          <BarButton
            label="Find in page"
            onClick={() => {
              setFinding((current) => {
                if (current) onStopFind()
                return !current
              })
            }}
            data-browser="find"
          >
            <SearchIcon width={14} height={14} />
          </BarButton>
          <BarButton
            label="Open in your own browser"
            disabled={address === ''}
            onClick={() => onOpenExternal(address)}
            data-browser="external"
          >
            <ExternalIcon width={14} height={14} />
          </BarButton>
          <BarButton
            label="More"
            buttonRef={moreRef}
            on={menuAt !== null}
            onClick={() => setMenuAt((open) => (open === null ? (moreRef.current?.getBoundingClientRect() ?? null) : null))}
            data-browser="more"
            aria-haspopup="menu"
            aria-expanded={menuAt !== null}
          >
            <MoreIcon width={14} height={14} />
          </BarButton>
        </div>
      </div>

      {/* The loading line: 2px of accent over the bar's bottom edge, a mark
          and not a fill (DESIGN.md "Browser pane"). Indeterminate, because
          Electron says a page is loading and not how far it has got. It takes
          no height, so the page below never moves when a load starts or ends. */}
      {state.loading && (
        <div aria-hidden="true" className="relative h-0 shrink-0">
          <div
            data-browser-loading
            className="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 overflow-hidden"
          >
            <div className="h-full w-1/3 animate-load bg-accent motion-reduce:w-full motion-reduce:animate-none" />
          </div>
        </div>
      )}

      {shareMenu !== null && (
        <Menu
          label="Share"
          entries={shareEntries}
          at={{ below: shareMenu.at }}
          anchorRef={shareRef}
          minWidth={200}
          layer={layer}
          onSelect={chooseShare}
          onDismiss={() => setShareMenu(null)}
        />
      )}

      {menuAt !== null && (
        <Menu
          label="Browser"
          entries={menu}
          at={{ below: menuAt }}
          anchorRef={moreRef}
          minWidth={220}
          layer={layer}
          onSelect={choose}
          onDismiss={() => setMenuAt(null)}
        />
      )}

      {finding && (
        <div
          data-browser-find
          className="flex shrink-0 items-center gap-2 border-b border-border px-2 py-1.5"
        >
          <input
            ref={findRef}
            data-browser-find-input
            autoFocus
            value={findQuery}
            aria-label="Find in page"
            placeholder="Find in page"
            spellCheck={false}
            onChange={(event) => {
              setFindQuery(event.target.value)
              onFind(event.target.value, true)
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault()
                onFind(findQuery, !event.shiftKey)
              }
              if (event.key === 'Escape') {
                event.preventDefault()
                setFinding(false)
                onStopFind()
              }
            }}
            className={cn(
              'min-w-0 flex-1 rounded-well border border-border bg-surface-sunken px-2 py-1',
              'text-[12px] text-fg outline-none focus:border-border-strong'
            )}
          />
          <span data-browser-find-count className="shrink-0 text-[11px] tabular-nums text-fg-subtle">
            {state.find === null || state.find.matches === 0
              ? findQuery === ''
                ? ''
                : 'no matches'
              : `${String(state.find.active)} / ${String(state.find.matches)}`}
          </span>
        </div>
      )}

      {/* A page a session can drive says so, above the page, for as long as
          it can: shared by the user, or opened by a session still running. */}
      {driver !== null && (
        <div
          data-browser-driver
          role="note"
          className="flex shrink-0 items-center gap-2 border-b border-border bg-accent-soft py-1 pr-1.5 pl-3 text-[12px] text-accent-text"
        >
          <AgentIcon width={13} height={13} className="shrink-0" />
          <span className="min-w-0 flex-1 truncate leading-[22px]">{driver.text}</span>
          {driver.stop && (
            <button
              type="button"
              data-browser="share-stop"
              onClick={() => onShare(null)}
              className="h-[22px] shrink-0 rounded-well border border-border-strong px-2 text-[11.5px] text-fg transition-colors hover:bg-hover"
            >
              Stop sharing
            </button>
          )}
        </div>
      )}

      {(state.problem !== null || retrying) && (
        <div
          data-browser-problem
          role="status"
          className={cn(
            'shrink-0 border-b px-3 py-2 text-[12px]',
            retrying
              ? 'border-border text-fg-muted'
              : 'border-danger/30 bg-danger/10 text-danger'
          )}
        >
          {state.problem}
        </div>
      )}

      {/*
        The hole. No ground, no border, no radius: the page paints here and
        Floti has no business tinting foreign ground. `min-h-0` so the console
        below can take its own height rather than pushing this off the pane.
      */}
      <div className="flex min-h-0 flex-1 justify-center">
        <div
          ref={holeRef}
          data-browser-hole={String(state.id)}
          style={width === null ? undefined : { maxWidth: `${String(width)}px` }}
          className="relative min-h-0 w-full flex-1 overflow-hidden"
        >
          {blank && (
            <div className="absolute inset-0">
              <BrowserEmpty onOpen={() => addressRef.current?.focus()} />
            </div>
          )}
          {/* The page, as a picture, while the view is off the screen for
              something drawn over it - so a menu opens over the page rather
              than over a hole. Exactly the view's rectangle, which is this one. */}
          {still !== null && (
            <img
              data-browser-still
              src={still}
              alt=""
              draggable={false}
              style={stillSize === null ? undefined : { width: `${String(stillSize.width)}px`, height: `${String(stillSize.height)}px` }}
              className={cn(
                'pointer-events-none absolute top-0 left-0 max-w-none select-none',
                stillSize === null && 'size-full'
              )}
            />
          )}
        </div>
      </div>

      {consoleOpen && (
        <ConsolePanel name="browser" entries={entries} open onToggle={onToggleConsole} onEvaluate={onEvaluate} />
      )}
    </div>
  )
}

/** A ghost icon button in the address-bar island, at the size the strip wants. */
function BarButton({
  label,
  disabled = false,
  on = false,
  onClick,
  buttonRef,
  children,
  ...hooks
}: {
  label: string
  disabled?: boolean
  on?: boolean
  onClick: (event: { shiftKey: boolean }) => void
  buttonRef?: RefObject<HTMLButtonElement | null>
  children: JSX.Element
  'aria-haspopup'?: 'menu'
  'aria-expanded'?: boolean
} & Record<`data-${string}`, string>): JSX.Element {
  return (
    <button
      type="button"
      ref={buttonRef}
      {...hooks}
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={(event) => onClick(event)}
      className={cn(
        'grid size-7 shrink-0 place-items-center rounded-well transition-colors',
        disabled
          ? 'cursor-default text-fg-subtle/40'
          : on
            ? cn(SEGMENT_ON, 'text-fg')
            : 'text-fg-subtle hover:bg-hover hover:text-fg'
      )}
    >
      {children}
    </button>
  )
}

/** Chromium's zoom *level* is logarithmic; what a person reads is a percentage. */
const zoomPercent = (level: number): string => `${String(Math.round(1.2 ** level * 100))}%`

/** A suggestion row the keys or the pointer are on; index -1 is none. */
interface Highlight {
  index: number
  by: 'keys' | 'pointer'
}

const NO_HIGHLIGHT: Highlight = { index: -1, by: 'keys' }

/** What the bar holds while the keys are on a row: the page's address, or the text a search is for. */
const rowText = (row: BrowserSuggestion): string => (row.kind === 'page' ? row.url : row.text)
