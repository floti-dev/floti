import type { JSX } from 'react'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import {
  suggestBrowserAddresses,
  type BrowserHistoryEntry,
  type BrowserSearch,
  type BrowserSuggestions
} from '@floti/core/types'
import { overlayMounted, overlayOpen } from '../lib/overlay'
import { fakeMenuLayer, type FakeMenuLayer } from '../lib/menuLayer.testkit'
import { BrowserEmpty, BrowserPane, browserPageBlank, type BrowserPaneProps, type BrowserPaneState } from './BrowserPane'

const STATE: BrowserPaneState = {
  id: 7,
  url: 'http://localhost:3000/',
  pendingUrl: null,
  refusedUrl: null,
  title: 'Dev server',
  host: 'localhost:3000',
  canGoBack: true,
  canGoForward: false,
  loading: false,
  problem: null,
  retryingUntil: null,
  zoomLevel: 0,
  errors: 0,
  devtoolsOpen: false,
  find: null,
  project: null,
  openedBy: null,
  openerRunning: false,
  sharedWith: null
}

const TARGETS = [
  { session: 1, name: 'alpha' },
  { session: 2, name: 'beta' }
]

/** The browser's history, newest first: what main would rank the suggestions from. */
const HISTORY: BrowserHistoryEntry[] = [
  { url: 'http://localhost:3000/b', title: 'Dev server B', visits: 1, lastVisit: 30 },
  { url: 'http://localhost:5173/', title: 'Vite app', visits: 4, lastVisit: 20 },
  { url: 'https://example.com/docs', title: 'Example docs', visits: 2, lastVisit: 10 }
]
const RECENT = HISTORY.map((entry) => entry.url)

/** Main's answer to `browser:suggest`, ranked by the real ranking over `HISTORY`. */
const suggestFrom =
  (history: readonly BrowserHistoryEntry[], engine: BrowserSearch = 'google') =>
  (text: string): Promise<BrowserSuggestions> =>
    Promise.resolve(suggestBrowserAddresses(history, text, engine))

/**
 * jsdom does no layout, so the placeholder's box is emulated: the space the
 * pane gives it, narrowed by a `max-width` the way CSS narrows a block. That is
 * all a width preset is, and it is enough to follow a preset through to the
 * rectangle the pane reports.
 */
let space = { x: 16, y: 120, width: 1200, height: 600 }
let resized: Array<() => void> = []
/** The menu layer the pane draws its menus and suggestions in. */
let layer: FakeMenuLayer = fakeMenuLayer()

beforeEach(() => {
  space = { x: 16, y: 120, width: 1200, height: 600 }
  resized = []
  layer = fakeMenuLayer()
  // The window holds the keyboard unless a test says it went elsewhere.
  vi.spyOn(document, 'hasFocus').mockReturnValue(true)
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    const max = Number.parseFloat(this.style.maxWidth)
    const width = Number.isFinite(max) ? Math.min(space.width, max) : space.width
    return { ...space, width, top: space.y, left: space.x, right: space.x + width, bottom: space.y + space.height, toJSON: () => ({}) }
  })
  vi.stubGlobal(
    'ResizeObserver',
    class {
      constructor(callback: () => void) {
        resized.push(callback)
      }
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
  )
})

afterEach(() => {
  vi.restoreAllMocks()
  vi.unstubAllGlobals()
})

function renderPane(overrides: Partial<BrowserPaneProps> = {}): BrowserPaneProps {
  const props: BrowserPaneProps = {
    state: STATE,
    entries: [],
    onBounds: vi.fn(),
    onNavigate: vi.fn(),
    onBack: vi.fn(),
    onForward: vi.fn(),
    onReload: vi.fn(),
    onStop: vi.fn(),
    onDevTools: vi.fn(),
    onOpenExternal: vi.fn(),
    onFind: vi.fn(),
    onStopFind: vi.fn(),
    onZoom: vi.fn(),
    onClearStorage: vi.fn(),
    onEvaluate: vi.fn(() => Promise.resolve({ ok: true, value: '', error: null })),
    consoleOpen: false,
    onToggleConsole: vi.fn(),
    layer,
    onSuggest: vi.fn(suggestFrom(HISTORY)),
    onForget: vi.fn(),
    onClearHistory: vi.fn(),
    focusRequest: null,
    onFocusRequestDone: vi.fn(),
    searchEngine: 'Google',
    still: null,
    canShare: true,
    onShareTargets: vi.fn(() => Promise.resolve(TARGETS)),
    onShare: vi.fn(),
    ...overrides
  }
  redraw = render(<BrowserPane {...props} />).rerender
  return props
}

/** Draws the pane rendered last again, as the window does on any change of its own. */
let redraw: (ui: JSX.Element) => void = () => undefined

const address = (): HTMLInputElement => screen.getByRole('combobox', { name: 'Address' })

/** The suggestions as they are listed: a page's address, or the Search row's text. */
const listed = (): string[] =>
  screen
    .getAllByRole('option')
    .map((row) => row.getAttribute('data-browser-suggestion-url') ?? row.textContent ?? '')

/** The part of the bar that is selected: the inline completion, right after typing. */
const selected = (): string => address().value.slice(address().selectionStart ?? 0, address().selectionEnd ?? 0)

/** The overflow menu, opened. */
async function openMenu(user: ReturnType<typeof userEvent.setup>): Promise<HTMLElement> {
  await user.click(screen.getByRole('button', { name: 'More' }))
  return screen.getByRole('menu', { name: 'Browser' })
}

const lastCall = <T,>(fn: unknown): T => (fn as { mock: { calls: T[][] } }).mock.calls.at(-1)![0]!

describe('BrowserPane - the address bar', () => {
  it('opens the pages visited last on a click, newest first, over the page in the menu layer until Escape', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    expect(screen.queryByRole('listbox')).toBeNull()

    await user.click(address())
    await screen.findByRole('listbox')
    expect(props.onSuggest).toHaveBeenLastCalledWith('')
    expect(listed()).toEqual(RECENT)
    expect(layer.suggestions?.content.rows.map((row) => (row.kind === 'page' ? row.url : row.text))).toEqual(RECENT)
    // Hung from the bar, as wide as it: jsdom's boxes are all the emulated space.
    expect(layer.suggestions?.rect).toEqual(space)
    expect(overlayOpen()).toBe(false)

    await user.keyboard('{Escape}')
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(layer.suggestions).toBeNull()
  })

  it('filters the suggestions as you type, best match first', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.clear(address())
    await user.type(address(), 'docs')
    await waitFor(() => expect(listed()).toEqual(['Search Google fordocs', 'https://example.com/docs']))
    expect(props.onSuggest).toHaveBeenLastCalledWith('docs')

    await user.clear(address())
    await user.type(address(), 'vite app')
    await waitFor(() => expect(listed()).toEqual(['Search Google forvite app', 'http://localhost:5173/']))
  })

  it('offers no Search row with searching off', async () => {
    const user = userEvent.setup()
    renderPane({ onSuggest: vi.fn(suggestFrom(HISTORY, 'off')), searchEngine: null })
    await user.clear(address())
    await user.type(address(), 'vite app')
    await waitFor(() => expect(listed()).toEqual(['http://localhost:5173/']))
  })

  it('completes the top match inline, selected, and Backspace takes the completion away', async () => {
    const user = userEvent.setup()
    renderPane()
    await user.clear(address())
    await user.type(address(), 'local')
    // Both dev servers start with it; the one visited more is the top match.
    await waitFor(() => expect(address().value).toBe('localhost:5173'))
    expect(selected()).toBe('host:5173')

    // Typing on along the completion keeps it, one letter shorter.
    await user.keyboard('h')
    expect(address().value).toBe('localhost:5173')
    expect(selected()).toBe('ost:5173')

    await user.keyboard('{Backspace}')
    expect(address().value).toBe('localh')
    // It stays away while the answer for what is left comes back.
    await waitFor(() => expect(listed()[0]).toBe('http://localhost:5173/'))
    expect(address().value).toBe('localh')
  })

  it('opens the completed page as it was visited on Enter, scheme and all', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.clear(address())
    await user.type(address(), 'exa')
    await waitFor(() => expect(address().value).toBe('example.com/docs'))
    await user.keyboard('{Enter}')
    expect(props.onNavigate).toHaveBeenCalledWith('https://example.com/docs')
  })

  it('moves through the rows with the arrows, opens the one Enter is pressed on, and Escape restores what was typed', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.clear(address())
    await user.type(address(), 'o')
    await waitFor(() => expect(screen.getAllByRole('option').length).toBeGreaterThan(1))
    const rows = listed()

    await user.keyboard('{ArrowDown}{ArrowDown}')
    expect(screen.getAllByRole('option')[1]?.getAttribute('aria-selected')).toBe('true')
    expect(address().value).toBe(rows[1])
    expect(address().getAttribute('aria-activedescendant')).toBe(screen.getAllByRole('option')[1]?.id)

    await user.keyboard('{Escape}')
    expect(address().value).toBe('o')
    expect(screen.getByRole('listbox')).toBeTruthy()

    await user.keyboard('{ArrowUp}')
    expect(address().value).toBe(rows.at(-1))
    await user.keyboard('{Enter}')
    expect(props.onNavigate).toHaveBeenCalledWith(rows.at(-1))
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('searches for what was typed from the Search row', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.clear(address())
    await user.type(address(), 'vite app')
    await waitFor(() => expect(listed()[0]).toBe('Search Google forvite app'))
    await user.keyboard('{ArrowDown}{Enter}')
    expect(props.onNavigate).toHaveBeenCalledWith('vite app')
  })

  it('only highlights a row under the pointer, leaving the bar and Enter with what was typed', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.clear(address())
    await user.type(address(), 'vite app')
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(2))
    fireEvent.mouseMove(screen.getAllByRole('option')[1]!)
    expect(screen.getAllByRole('option')[1]?.getAttribute('aria-selected')).toBe('true')
    expect(address().value).toBe('vite app')
    await user.keyboard('{Enter}')
    expect(props.onNavigate).toHaveBeenCalledWith('vite app')
  })

  it('steps on from the row the pointer is on', async () => {
    const user = userEvent.setup()
    renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    fireEvent.mouseMove(screen.getAllByRole('option')[1]!)
    await user.keyboard('{ArrowDown}')
    expect(screen.getAllByRole('option').map((row) => row.getAttribute('aria-selected'))).toEqual(['false', 'false', 'true'])
    expect(address().value).toBe(RECENT[2])
  })

  /** Main's answers: the real ranking, except that the one for '' (the bar opened on the page) never comes. */
  const nothingForEmpty = () =>
    vi.fn((text: string) => (text === '' ? new Promise<BrowserSuggestions>(() => undefined) : suggestFrom(HISTORY)(text)))

  it('shows none of the rows typed for before when the bar is left and opened again', async () => {
    const user = userEvent.setup()
    renderPane({ onSuggest: nothingForEmpty() })
    await user.clear(address())
    await user.type(address(), 'docs')
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(2))

    // Nothing completes `docs`, so one Escape puts the address back and leaves the bar.
    await user.keyboard('{Escape}')
    expect(document.activeElement).not.toBe(address())
    await user.click(address())
    // The bar shows the page and its answer has not come: nothing is listed.
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('shows none of the rows typed for before when the bar is opened again after Enter', async () => {
    const user = userEvent.setup()
    const props = renderPane({ onSuggest: nothingForEmpty() })
    await user.clear(address())
    await user.type(address(), 'docs')
    await waitFor(() => expect(screen.getAllByRole('option')).toHaveLength(2))
    await user.keyboard('{Enter}')
    expect(props.onNavigate).toHaveBeenCalledWith('docs')

    await user.click(address())
    expect(screen.queryByRole('listbox')).toBeNull()
  })

  it('removes the highlighted page from history with Shift+Delete', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    await user.keyboard('{ArrowDown}{ArrowDown}{Shift>}{Delete}{/Shift}')
    expect(props.onForget).toHaveBeenCalledWith('http://localhost:5173/')
    expect(listed()).toEqual(['http://localhost:3000/b', 'https://example.com/docs'])
    // The highlight stays where it was, on the row that took its place.
    expect(address().value).toBe('https://example.com/docs')
  })

  it('removes nothing with Shift+Delete when no page is highlighted', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    await user.keyboard('{Shift>}{Delete}{/Shift}')
    expect(props.onForget).not.toHaveBeenCalled()
  })

  it('clears the browsing history from the More menu', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    const menu = await openMenu(user)
    await user.click(within(menu).getByRole('menuitem', { name: 'Clear browsing history' }))
    expect(props.onClearHistory).toHaveBeenCalledTimes(1)
  })

  it('opens the list in the layer once, however often the pane is drawn, and sends it again only when it changes', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    expect(layer.suggestionsOpened).toBe(1)
    const updates = layer.suggestions!.updates

    // The window draws the pane again with new callbacks, and a still arriving.
    redraw(<BrowserPane {...props} onNavigate={vi.fn()} />)
    redraw(<BrowserPane {...props} onNavigate={vi.fn()} still="data:image/png;base64,cGFnZQ==" />)
    expect(layer.suggestionsOpened).toBe(1)
    expect(layer.suggestions!.updates).toBe(updates)

    // The keys move the highlight: sent, and the list stays the one open.
    await user.keyboard('{ArrowDown}')
    expect(layer.suggestions!.content.highlighted).toBe(0)
    expect(layer.suggestionsOpened).toBe(1)
  })

  it('opens nothing over the page when there is nothing to list', async () => {
    const user = userEvent.setup()
    const props = renderPane({ onSuggest: vi.fn(suggestFrom([])) })
    await user.click(address())
    await waitFor(() => expect(props.onSuggest).toHaveBeenCalledWith(''))
    await act(async () => undefined)
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(layer.suggestionsOpened).toBe(0)
  })

  it('goes to a suggested page when one is chosen in the layer', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    act(() => layer.suggestions!.pick(1))
    expect(props.onNavigate).toHaveBeenCalledWith('http://localhost:5173/')
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(layer.suggestions).toBeNull()
  })

  it('keeps the list and the pick when a press on it takes the keyboard from the window for a moment', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    // The press lands in the layer: the window loses the keyboard, and main hands it straight back.
    vi.spyOn(document, 'hasFocus').mockReturnValue(false)
    act(() => {
      layer.suggestions!.press()
      fireEvent.blur(address())
    })
    expect(layer.blurs).toBe(1)
    expect(screen.getByRole('listbox')).toBeTruthy()
    act(() => layer.suggestions!.pick(2))
    expect(props.onNavigate).toHaveBeenCalledWith('https://example.com/docs')
  })

  it('closes the list when the window loses the keyboard to anything else, as the layer judges', async () => {
    const user = userEvent.setup()
    renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    vi.spyOn(document, 'hasFocus').mockReturnValue(false)
    act(() => address().blur())
    expect(layer.blurs).toBe(1)
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(layer.suggestions).toBeNull()
  })

  it('closes the list itself when focus moves within the window, asking the layer nothing', async () => {
    const user = userEvent.setup()
    renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    vi.spyOn(document, 'hasFocus').mockReturnValue(true)
    await user.click(screen.getByRole('button', { name: 'Find in page' }))
    expect(layer.blurs).toBe(0)
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(layer.suggestions).toBeNull()
  })

  it('closes the list when Floti opens a dialog on its own, which is drawn under the layer', async () => {
    const user = userEvent.setup()
    renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    let release: () => void = () => undefined
    act(() => {
      release = overlayMounted()
    })
    expect(screen.queryByRole('listbox')).toBeNull()
    expect(layer.suggestions).toBeNull()
    act(() => release())
  })

  it("highlights the row the layer's pointer is on, without putting it in the bar", async () => {
    const user = userEvent.setup()
    renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    act(() => layer.suggestions!.hover(1))
    expect(layer.suggestions!.content.highlighted).toBe(1)
    expect(screen.getAllByRole('option')[1]!.getAttribute('aria-selected')).toBe('true')
    expect(address().value).toBe(STATE.url)
    expect(address().getAttribute('aria-activedescendant')).toBeNull()
  })

  it("names the keys' row to assistive technology from the list the window keeps", async () => {
    const user = userEvent.setup()
    renderPane()
    await user.click(address())
    await screen.findByRole('listbox')
    await user.keyboard('{ArrowDown}{ArrowDown}')
    expect(address().getAttribute('aria-activedescendant')).toBe(screen.getAllByRole('option')[1]!.id)
    expect(address().getAttribute('aria-controls')).toBe(screen.getByRole('listbox').id)
    expect(layer.suggestions!.content.highlighted).toBe(1)
  })

  it('stays open when the bar is clicked again just after Enter, however long it is then left', async () => {
    vi.useFakeTimers()
    try {
      const props = renderPane()
      act(() => address().focus())
      fireEvent.keyDown(address(), { key: 'Enter' })
      expect(props.onNavigate).toHaveBeenCalledWith('http://localhost:3000/')
      expect(document.activeElement).not.toBe(address())

      // Back in the bar before anything the Enter set going has run out.
      await act(async () => {
        address().focus()
        fireEvent.click(address())
        await Promise.resolve()
      })
      expect(screen.getByRole('listbox')).toBeTruthy()
      act(() => {
        vi.advanceTimersByTime(60_000)
      })
      expect(screen.getByRole('listbox')).toBeTruthy()
      expect(layer.suggestions).not.toBeNull()
    } finally {
      vi.useRealTimers()
    }
  })

  it("goes where Enter says, and with nothing loading shows the page's own address again", async () => {
    const user = userEvent.setup()
    const props = renderPane()
    expect(address().value).toBe('http://localhost:3000/')
    await user.clear(address())
    // An address the history has never seen, so nothing completes it.
    await user.type(address(), 'localhost:4000{Enter}')
    expect(props.onNavigate).toHaveBeenCalledWith('localhost:4000')
    expect(address().value).toBe('http://localhost:3000/')
  })

  it('shows the address being loaded over the page it replaces, until the page commits', () => {
    const props = renderPane({ state: { ...STATE, loading: true, pendingUrl: 'http://localhost:5173/' } })
    expect(address().value).toBe('http://localhost:5173/')
    redraw(<BrowserPane {...props} state={{ ...STATE, url: 'http://localhost:5173/' }} />)
    expect(address().value).toBe('http://localhost:5173/')
  })

  it('shows the address being loaded on a new tab, which has no address of its own yet', () => {
    renderPane({ state: { ...STATE, url: '', loading: true, pendingUrl: 'http://localhost:5173/' } })
    expect(address().value).toBe('http://localhost:5173/')
  })

  it('never shows the old address between Enter and the address being loaded arriving', async () => {
    const search = 'https://www.google.com/search?q=cats%20and%20dogs'
    let answer: () => void = () => undefined
    const onNavigate = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          answer = resolve
        })
    )
    const user = userEvent.setup()
    const props = renderPane({ onNavigate })
    await user.clear(address())
    await user.type(address(), 'cats and dogs{Enter}')
    expect(onNavigate).toHaveBeenCalledWith('cats and dogs')

    // Main has not answered: whatever redraws the pane, the bar says what was sent.
    redraw(<BrowserPane {...props} />)
    expect(address().value).toBe('cats and dogs')

    // Main's answer: the results page is loading. From then on the bar says that.
    redraw(<BrowserPane {...props} state={{ ...STATE, loading: true, pendingUrl: search }} />)
    await act(async () => answer())
    expect(address().value).toBe(search)

    redraw(<BrowserPane {...props} state={{ ...STATE, url: search }} />)
    expect(address().value).toBe(search)
  })

  it('keeps a suggested page chosen in the bar until the address being loaded arrives', async () => {
    const onNavigate = vi.fn(() => new Promise<void>(() => undefined))
    const user = userEvent.setup()
    renderPane({ onNavigate })
    await user.click(address())
    await screen.findByRole('listbox')
    await user.click(screen.getAllByRole('option')[1]!)
    expect(address().value).toBe('http://localhost:5173/')
  })

  it('stops a load on Escape with nothing typed, and with something typed only puts the address back', async () => {
    const user = userEvent.setup()
    const props = renderPane({ state: { ...STATE, loading: true, pendingUrl: 'http://localhost:5173/' } })

    await user.click(address())
    await user.type(address(), '/x')
    await user.keyboard('{Escape}')
    expect(props.onStop).not.toHaveBeenCalled()
    expect(address().value).toBe('http://localhost:5173/')

    await user.click(address())
    await user.keyboard('{Escape}')
    expect(props.onStop).toHaveBeenCalledTimes(1)
  })

  it('stops nothing on Escape when nothing is loading', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(address())
    await user.keyboard('{Escape}')
    expect(props.onStop).not.toHaveBeenCalled()
  })

  it('shows the address a tab was refused before any page loaded, and opens it in your own browser', async () => {
    const user = userEvent.setup()
    const problem = 'example.com is off this machine, and the browser is set to This machine only.'
    const props = renderPane({
      state: {
        ...STATE,
        url: '',
        refusedUrl: 'https://example.com/docs',
        title: 'example.com',
        host: 'example.com',
        canGoBack: false,
        problem
      }
    })
    expect(address().value).toBe('https://example.com/docs')
    expect(screen.getByRole('status').textContent).toBe(problem)
    await user.click(screen.getByRole('button', { name: 'Open in your own browser' }))
    expect(props.onOpenExternal).toHaveBeenCalledWith('https://example.com/docs')
  })

  it('has nothing to open in your own browser on an empty tab', () => {
    renderPane({ state: { ...STATE, url: '', refusedUrl: null } })
    expect(address().value).toBe('')
    expect(screen.getByRole('button', { name: 'Open in your own browser' })).toHaveProperty('disabled', true)
  })

  it('on an empty tab, says No pages open where the page would be, and Open a page puts the caret in the address bar', async () => {
    renderPane({ state: { ...STATE, url: '', title: '' } })
    const empty = document.querySelector<HTMLElement>('[data-empty-state="browser"]')!
    expect(within(empty).getByText('No pages open')).toBeTruthy()
    expect(within(empty).getByText('Type an address, or let an agent open one.')).toBeTruthy()
    await userEvent.click(within(empty).getByRole('button', { name: 'Open a page' }))
    expect(document.activeElement).toBe(address())
  })

  it('says No pages open only while the tab is going nowhere: not loading, refused or in trouble', () => {
    expect(browserPageBlank({ ...STATE, url: '' })).toBe(true)
    expect(browserPageBlank(STATE)).toBe(false)
    expect(browserPageBlank({ ...STATE, url: '', pendingUrl: 'http://localhost:5173/' })).toBe(false)
    expect(browserPageBlank({ ...STATE, url: '', loading: true })).toBe(false)
    expect(browserPageBlank({ ...STATE, url: '', refusedUrl: 'file:///C:/x' })).toBe(false)
    expect(browserPageBlank({ ...STATE, url: '', problem: 'Nothing is listening.' })).toBe(false)
    renderPane()
    expect(document.querySelector('[data-empty-state="browser"]')).toBeNull()
  })

  it('with no page at all, Open a page asks for one', async () => {
    const onOpen = vi.fn()
    render(<BrowserEmpty onOpen={onOpen} />)
    await userEvent.click(screen.getByRole('button', { name: 'Open a page' }))
    expect(onOpen).toHaveBeenCalledOnce()
  })

  it('paints what is wrong with the tab as a sentence', () => {
    const problem = 'Nothing is listening at http://localhost:3000/.'
    renderPane({ state: { ...STATE, problem } })
    expect(screen.getByRole('status').textContent).toBe(problem)
  })

  it('says in the empty bar whether a phrase will be searched', () => {
    renderPane({ state: { ...STATE, url: '' } })
    expect(address().placeholder).toBe('Search Google or type an address')
  })

  it('says only addresses go there when searching is off', () => {
    renderPane({ state: { ...STATE, url: '' }, searchEngine: null })
    expect(address().placeholder).toBe('Address, or a port number')
  })

  it('takes the caret into the address bar or the find field when asked, once', () => {
    const asked = renderPane({ focusRequest: 'address' })
    expect(document.activeElement).toBe(address())
    expect(asked.onFocusRequestDone).toHaveBeenCalledTimes(1)
  })

  it('opens the find field for Ctrl+F with the caret in it', () => {
    const asked = renderPane({ focusRequest: 'find' })
    expect(document.activeElement).toBe(screen.getByRole('textbox', { name: 'Find in page' }))
    expect(asked.onFocusRequestDone).toHaveBeenCalledTimes(1)
  })

  it('takes nothing when nothing asked, so a page coming to the front leaves the caret alone', () => {
    const asked = renderPane()
    expect(document.activeElement).not.toBe(address())
    expect(asked.onFocusRequestDone).not.toHaveBeenCalled()
  })
})

describe('BrowserPane - the controls', () => {
  it('goes back, and cannot go forward with nowhere to go', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(screen.getByRole('button', { name: 'Back' }))
    expect(props.onBack).toHaveBeenCalledTimes(1)
    expect((screen.getByRole('button', { name: 'Forward' }) as HTMLButtonElement).disabled).toBe(true)
  })

  it('reloads, and ignores the cache with Shift held', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    const reload = screen.getByRole('button', { name: 'Reload (hold Shift to ignore the cache)' })
    await user.click(reload)
    await user.keyboard('{Shift>}')
    await user.click(reload)
    await user.keyboard('{/Shift}')
    expect(vi.mocked(props.onReload).mock.calls).toEqual([[false], [true]])
  })

  it('stops a load from the reload button while the page loads, and reloads once it has', async () => {
    const user = userEvent.setup()
    const props = renderPane({ state: { ...STATE, loading: true, pendingUrl: 'http://localhost:5173/' } })
    expect(screen.queryByRole('button', { name: 'Reload (hold Shift to ignore the cache)' })).toBeNull()
    await user.click(screen.getByRole('button', { name: 'Stop loading' }))
    expect(props.onStop).toHaveBeenCalledTimes(1)
    expect(props.onReload).not.toHaveBeenCalled()

    redraw(<BrowserPane {...props} state={STATE} />)
    expect(screen.queryByRole('button', { name: 'Stop loading' })).toBeNull()
    await user.click(screen.getByRole('button', { name: 'Reload (hold Shift to ignore the cache)' }))
    expect(props.onReload).toHaveBeenCalledWith(false)
    expect(props.onStop).toHaveBeenCalledTimes(1)
  })

  it('keeps a slim bar: zoom, width, DevTools and clearing data are in the menu, not on it', () => {
    renderPane()
    const bar = document.querySelector<HTMLElement>('[data-browser-bar]')!
    expect(within(bar).getAllByRole('button').map((button) => button.getAttribute('aria-label'))).toEqual([
      'Back',
      'Forward',
      'Reload (hold Shift to ignore the cache)',
      'Share with a session',
      'Find in page',
      'Open in your own browser',
      'More'
    ])
  })

  describe('sharing', () => {
    const shareButton = (): HTMLElement | null => document.querySelector('[data-browser="share"]')

    /** The Share menu, opened, once the sessions have been asked for. */
    async function openShare(user: ReturnType<typeof userEvent.setup>): Promise<HTMLElement> {
      await user.click(shareButton()!)
      return screen.findByRole('menu', { name: 'Share' })
    }

    it("offers a page of the user's, and never an agent's page, an empty one, or anything with the tools off", () => {
      for (const [overrides, offered] of [
        [{}, true],
        [{ canShare: false }, false],
        [{ state: { ...STATE, openedBy: 'alpha', openerRunning: true } }, false],
        [{ state: { ...STATE, url: '' } }, false]
      ] as const) {
        cleanup()
        renderPane(overrides)
        expect(shareButton() !== null, JSON.stringify(overrides)).toBe(offered)
      }
    })

    it('asks for the sessions as the menu opens, and shares with the one picked', async () => {
      const user = userEvent.setup()
      const props = renderPane()
      expect(props.onShareTargets).not.toHaveBeenCalled()
      const menu = await openShare(user)
      expect(props.onShareTargets).toHaveBeenCalledTimes(1)
      expect(menu.textContent).toContain('Share with')
      expect(within(menu).queryByRole('menuitem', { name: 'Stop sharing' })).toBeNull()
      await user.click(within(menu).getByRole('menuitemcheckbox', { name: /beta/ }))
      expect(props.onShare).toHaveBeenCalledWith(2)
      expect(screen.queryByRole('menu', { name: 'Share' })).toBeNull()
    })

    it('says who a shared page is shared with, ticks them, and stops sharing from the menu', async () => {
      const user = userEvent.setup()
      const props = renderPane({ state: { ...STATE, sharedWith: { session: 1, name: 'alpha' } } })
      expect(shareButton()!.querySelector('span > span:not([aria-hidden])')?.textContent).toBe('Shared')
      expect(shareButton()!.getAttribute('aria-label')).toBe('Shared with alpha')

      let menu = await openShare(user)
      expect(within(menu).getByRole('menuitemcheckbox', { name: /alpha/ }).getAttribute('aria-checked')).toBe('true')
      expect(within(menu).getByRole('menuitemcheckbox', { name: /beta/ }).getAttribute('aria-checked')).toBe('false')
      // Picking the session it is already shared with changes nothing.
      await user.click(within(menu).getByRole('menuitemcheckbox', { name: /alpha/ }))
      expect(props.onShare).not.toHaveBeenCalled()

      menu = await openShare(user)
      await user.click(within(menu).getByRole('menuitem', { name: 'Stop sharing' }))
      expect(props.onShare).toHaveBeenCalledWith(null)
    })

    it('says there is nobody to share with when no session can take the page', async () => {
      const user = userEvent.setup()
      renderPane({ onShareTargets: vi.fn(() => Promise.resolve([])) })
      const menu = await openShare(user)
      const none = within(menu).getByRole('menuitem', { name: 'No session to share with' })
      expect(none.getAttribute('aria-disabled')).toBe('true')
    })

    it('says above the page which session can drive it, while it can, with a way to stop sharing', async () => {
      const user = userEvent.setup()
      const props = renderPane({ state: { ...STATE, sharedWith: { session: 1, name: 'alpha' } } })
      const note = screen.getByRole('note')
      expect(note.textContent).toBe('Shared with “alpha”: that session can read and drive this page.Stop sharing')
      await user.click(within(note).getByRole('button', { name: 'Stop sharing' }))
      expect(props.onShare).toHaveBeenCalledWith(null)
    })

    it("says an agent's page is the session's while it runs, and nothing once it has ended", () => {
      renderPane({ state: { ...STATE, openedBy: 'alpha', openerRunning: true } })
      expect(screen.getByRole('note').textContent).toBe('“alpha” opened this page and can drive it.')
      cleanup()
      renderPane({ state: { ...STATE, openedBy: 'alpha', openerRunning: false } })
      expect(screen.queryByRole('note')).toBeNull()
      cleanup()
      renderPane()
      expect(screen.queryByRole('note')).toBeNull()
    })
  })

  it('lists zoom, the widths, DevTools and clearing data in the menu, with their keys', async () => {
    const user = userEvent.setup()
    renderPane({ state: { ...STATE, zoomLevel: 1 } })
    const menu = await openMenu(user)
    expect(menu.textContent).toContain('Zoom 120%')
    const rows = [...menu.querySelectorAll('[data-menu-item]')].map((row) => row.textContent)
    expect(rows).toEqual([
      'Zoom inCtrl =',
      'Zoom outCtrl -',
      'Actual sizeCtrl 0',
      'Full',
      'Tablet820 px',
      'Phone390 px',
      'Open consoleCtrl Shift J',
      'Open DevToolsF12',
      'Clear browsing history',
      'Clear cookies and site data'
    ])
    expect(within(menu).getByRole('menuitemcheckbox', { name: /Full/ }).getAttribute('aria-checked')).toBe('true')
    expect(within(menu).getByRole('menuitem', { name: /Zoom in/ }).getAttribute('role')).toBe('menuitem')
  })

  it('zooms by half a step from the menu, and back to actual size', async () => {
    const user = userEvent.setup()
    const props = renderPane({ state: { ...STATE, zoomLevel: 1 } })
    await user.click(within(await openMenu(user)).getByRole('menuitem', { name: /Zoom in/ }))
    await user.click(within(await openMenu(user)).getByRole('menuitem', { name: /Zoom out/ }))
    await user.click(within(await openMenu(user)).getByRole('menuitem', { name: /Actual size/ }))
    expect(vi.mocked(props.onZoom).mock.calls).toEqual([[1.5], [0.5], [0]])
  })

  it('cannot zoom past the ends, nor reset what is already actual size', async () => {
    const user = userEvent.setup()
    renderPane({ state: { ...STATE, zoomLevel: 3 } })
    const menu = await openMenu(user)
    expect(within(menu).getByRole('menuitem', { name: /Zoom in/ }).getAttribute('aria-disabled')).toBe('true')
    await user.keyboard('{Escape}')
  })

  it('says the zoom in the address bar only when it is not 100%, and resets it from there', async () => {
    const user = userEvent.setup()
    const props = renderPane({ state: { ...STATE, zoomLevel: 0.5 } })
    const chip = screen.getByRole('button', { name: 'Zoom 110%, back to actual size' })
    expect(chip.textContent).toBe('110%')
    await user.click(chip)
    expect(props.onZoom).toHaveBeenCalledWith(0)
  })

  it('shows no zoom in the address bar at 100%', () => {
    renderPane()
    expect(screen.queryByRole('button', { name: /back to actual size/ })).toBeNull()
  })

  it('opens DevTools and clears the browsing data from the menu', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(within(await openMenu(user)).getByRole('menuitem', { name: /Open DevTools/ }))
    await user.click(within(await openMenu(user)).getByRole('menuitem', { name: /Clear cookies and site data/ }))
    expect(props.onDevTools).toHaveBeenCalledTimes(1)
    expect(props.onClearStorage).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('menu')).toBeNull()
  })

  it('finds in the page as it is typed, steps with Enter, and stops on Escape', async () => {
    const user = userEvent.setup()
    const props = renderPane({ state: { ...STATE, find: { query: 'tok', matches: 3, active: 1 } } })
    await user.click(screen.getByRole('button', { name: 'Find in page' }))
    const field = screen.getByRole('textbox', { name: 'Find in page' })
    await user.type(field, 'tok')
    expect(lastCall<string>(props.onFind)).toBe('tok')
    expect(screen.getByText('1 / 3')).toBeTruthy()

    await user.keyboard('{Enter}')
    expect(vi.mocked(props.onFind).mock.calls.at(-1)).toEqual(['tok', true])
    await user.keyboard('{Shift>}{Enter}{/Shift}')
    expect(vi.mocked(props.onFind).mock.calls.at(-1)).toEqual(['tok', false])

    await user.keyboard('{Escape}')
    expect(props.onStopFind).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('textbox', { name: 'Find in page' })).toBeNull()
  })

  it('stops finding when the find button is pressed again', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(screen.getByRole('button', { name: 'Find in page' }))
    await user.click(screen.getByRole('button', { name: 'Find in page' }))
    expect(props.onStopFind).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('textbox', { name: 'Find in page' })).toBeNull()
  })

  it('draws no console row, count or filter while the console is closed, errors included', () => {
    renderPane({ state: { ...STATE, errors: 3 }, entries: [{ level: 'error', message: 'boom', source: 'a.js', line: 1 }] })
    expect(screen.queryByRole('button', { name: /^Console/ })).toBeNull()
    expect(screen.queryByRole('textbox', { name: 'Evaluate JavaScript in the page' })).toBeNull()
    expect(screen.queryByText('Console')).toBeNull()
  })

  it('has a console that evaluates in the page, open, with a close button', async () => {
    const user = userEvent.setup()
    const props = renderPane({ consoleOpen: true })
    expect(screen.getByRole('textbox', { name: 'Evaluate JavaScript in the page' })).toBeTruthy()
    await user.click(screen.getByRole('button', { name: /^Console/ }))
    expect(props.onToggleConsole).toHaveBeenCalledTimes(1)
  })

  it('opens and closes the console from the More menu, which names the key', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    const item = within(await openMenu(user)).getByRole('menuitem', { name: /Open console/ })
    expect(item.textContent).toContain('Ctrl Shift J')
    await user.click(item)
    expect(props.onToggleConsole).toHaveBeenCalledTimes(1)
    cleanup()
    renderPane({ consoleOpen: true })
    expect(within(await openMenu(user)).getByRole('menuitem', { name: /Close console/ })).toBeTruthy()
  })
})

describe('BrowserPane - where the view goes', () => {
  it('reports the placeholder on mount, on a resize of it, and on a window resize', () => {
    const props = renderPane()
    expect(lastCall(props.onBounds)).toEqual({ x: 16, y: 120, width: 1200, height: 600 })

    // A split drag narrows the pane.
    space = { ...space, width: 900 }
    act(() => {
      for (const callback of resized) callback()
    })
    expect(lastCall(props.onBounds)).toEqual({ x: 16, y: 120, width: 900, height: 600 })

    // The window moves the pane down without resizing it.
    space = { ...space, y: 160 }
    fireEvent(window, new Event('resize'))
    expect(lastCall(props.onBounds)).toEqual({ x: 16, y: 160, width: 900, height: 600 })
  })

  it('narrows the view to a phone and back to the full pane', async () => {
    const user = userEvent.setup()
    const props = renderPane()
    await user.click(within(await openMenu(user)).getByRole('menuitemcheckbox', { name: /Phone/ }))
    expect(lastCall<{ width: number }>(props.onBounds).width).toBe(390)
    const menu = await openMenu(user)
    expect(within(menu).getByRole('menuitemcheckbox', { name: /Phone/ }).getAttribute('aria-checked')).toBe('true')

    await user.click(within(menu).getByRole('menuitemcheckbox', { name: /Full/ }))
    expect(lastCall<{ width: number }>(props.onBounds).width).toBe(1200)
  })

  it('paints a still of the page in the hole while the view is off the screen, and nothing otherwise', () => {
    renderPane({ still: 'data:image/png;base64,cGFnZQ==' })
    const hole = document.querySelector('[data-browser-hole]')!
    const still = hole.querySelector('img')!
    expect(still.getAttribute('src')).toBe('data:image/png;base64,cGFnZQ==')
    // Decoration, out of the accessibility tree and out of the pointer's way.
    expect(still.getAttribute('alt')).toBe('')
    expect(still.className).toContain('pointer-events-none')
  })

  it('keeps the still at the size it was taken, so a pane resized under it crops it rather than squeezing it', () => {
    vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(640)
    vi.spyOn(HTMLElement.prototype, 'clientHeight', 'get').mockReturnValue(480)
    renderPane({ still: 'data:image/png;base64,cGFnZQ==' })
    const hole = document.querySelector<HTMLElement>('[data-browser-hole]')!
    const still = hole.querySelector('img')!
    expect(still.style).toMatchObject({ width: '640px', height: '480px' })
    expect(still.className).toContain('max-w-none')
    expect(hole.className).toContain('overflow-hidden')
  })
})

describe('BrowserPane - the loading line', () => {
  const line = (): Element | null => document.querySelector('[data-browser-loading]')

  it('shows under the bar while the page loads, and goes when it has loaded', () => {
    const props = renderPane({ state: { ...STATE, loading: true, pendingUrl: 'http://localhost:5173/' } })
    const shown = line()
    expect(shown).not.toBeNull()
    // Directly under the bar, drawn in the accent token, and not read out:
    // the Stop button already says the page is loading.
    expect(shown!.parentElement!.previousElementSibling?.hasAttribute('data-browser-bar')).toBe(true)
    expect(shown!.parentElement!.getAttribute('aria-hidden')).toBe('true')
    expect(shown!.querySelector('.bg-accent')).not.toBeNull()

    redraw(<BrowserPane {...props} state={STATE} />)
    expect(line()).toBeNull()
  })

  it('is not there for a page that is not loading', () => {
    renderPane()
    expect(line()).toBeNull()
  })
})

describe('BrowserPane - the menus in the menu layer', () => {
  it('opens More over the page in the layer, drawn from the button, with nothing standing down', async () => {
    const user = userEvent.setup()
    renderPane()
    await openMenu(user)
    expect(layer.menus).toHaveLength(1)
    expect(layer.menus[0]!.menu.label).toBe('Browser')
    expect(overlayOpen()).toBe(false)
  })

  it('closes on a second press of its button and stays closed', async () => {
    const user = userEvent.setup()
    renderPane()
    await openMenu(user)
    await user.click(screen.getByRole('button', { name: 'More' }))
    await act(async () => undefined)
    expect(screen.queryByRole('menu', { name: 'Browser' })).toBeNull()
    expect(layer.menus.filter((menu) => menu.open)).toHaveLength(0)
    expect(layer.menus).toHaveLength(1)
  })

  it('closes when a press anywhere else takes the keyboard from it, and Escape gives it back to the button', async () => {
    const user = userEvent.setup()
    renderPane()
    await openMenu(user)
    await user.click(address())
    await act(async () => undefined)
    expect(screen.queryByRole('menu', { name: 'Browser' })).toBeNull()

    const more = screen.getByRole('button', { name: 'More' })
    await openMenu(user)
    await user.keyboard('{Escape}')
    await act(async () => undefined)
    expect(screen.queryByRole('menu', { name: 'Browser' })).toBeNull()
    expect(document.activeElement).toBe(more)
  })

  it('closes More when Floti opens a dialog on its own', async () => {
    const user = userEvent.setup()
    renderPane()
    await openMenu(user)
    let release: () => void = () => undefined
    act(() => {
      release = overlayMounted()
    })
    await act(async () => undefined)
    expect(screen.queryByRole('menu', { name: 'Browser' })).toBeNull()
    expect(layer.menus[0]!.open).toBe(false)
    act(() => release())
  })

  it('opens Share in the layer too', async () => {
    const user = userEvent.setup()
    renderPane()
    await user.click(screen.getByRole('button', { name: 'Share with a session' }))
    await screen.findByRole('menu', { name: 'Share' })
    expect(layer.menus.map((menu) => menu.menu.label)).toEqual(['Share'])
  })
})
