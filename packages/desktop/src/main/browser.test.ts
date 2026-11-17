import { afterEach, describe, expect, it, vi } from 'vitest'
import type { BrowserWindow, BrowserWindowConstructorOptions, WebContents } from 'electron'
import { DEFAULT_SETTINGS, openBrowserHistory, openStore, type AppSettings, type BrowserHistory, type Store } from '@helm/core'
import {
  FakeBrowserWindow,
  FakeIncomingMessage,
  FakeWebContents,
  FakeWebContentsView,
  fakeBrowser,
  type FakeRoute,
  type FakeSession
} from '../../test/browser-electron'
import {
  browserWillNavigate,
  browserWindowOpen,
  createBrowserHost,
  exemptedWebContents,
  flushBrowserCookies,
  type BrowserHost,
  type BrowserOpener
} from './browser'
import type {
  BrowserCommandEvent,
  BrowserConsoleEntry,
  BrowserFavicon,
  BrowserOpened,
  BrowserState
} from '../shared/ipc'

vi.mock('electron', async () => ({
  ...(await import('../../test/electron')).electronFake(),
  ...(await import('../../test/browser-electron')).browserElectronFake()
}))

/**
 * The browser pane's main-process half, against a fake Electron.
 *
 * The host is real; Chromium is played by the test (`test/browser-electron.ts`).
 * So every claim here is about what **Helm** does with what Chromium reports -
 * which is exactly the part a real page cannot isolate. A refused connection
 * that later loads proves nothing about Helm's retry when Chromium may have held
 * the socket open itself; here nothing but Helm can ask for the page again.
 */

/** The sessions a workspace can share a page with, by id: what `sessions.browserOpener` would answer. */
const SESSIONS = new Map<number, BrowserOpener>([
  [7, { key: 'token-alpha', name: 'alpha' }],
  [8, { key: 'token-beta', name: 'beta' }]
])

interface Workspace {
  host: BrowserHost
  window: FakeBrowserWindow
  settings: () => AppSettings
  patch: (next: Partial<AppSettings>) => void
  opened: BrowserOpened[]
  closed: number[]
  commands: BrowserCommandEvent[]
  /** Every `browser:favicon` the host sent, in order. */
  favicons: BrowserFavicon[]
  /** The history the host records into, on a database in memory. */
  history: BrowserHistory
  store: Store
}

/** The history's addresses, newest first. */
const historyOf = (h: Workspace): string[] =>
  [...h.history.entries()].sort((a, b) => b.lastVisit - a.lastVisit || b.visits - a.visits).map((entry) => entry.url)

let current: Workspace | null = null

/**
 * A host over a fake Electron. It reads a page's keys as the OS named here
 * does - Windows' table unless a test asks for the Mac's - never as the OS the
 * tests happen to run on.
 */
function workspace(patch: Partial<AppSettings> = {}, platform: NodeJS.Platform = 'win32'): Workspace {
  let settings: AppSettings = { ...DEFAULT_SETTINGS, browserProjectUrls: {}, ...patch }
  const window = new FakeBrowserWindow()
  const store = openStore({ file: ':memory:' })
  // A clock that moves a millisecond per visit, so newest-first is the order the visits were made.
  let clock = 0
  const history = openBrowserHistory(store, { now: () => ++clock })
  const opened: BrowserOpened[] = []
  const closed: number[] = []
  const commands: BrowserCommandEvent[] = []
  const favicons: BrowserFavicon[] = []
  const host = createBrowserHost({
    platform,
    window: () => window as unknown as BrowserWindow,
    settings: () => settings,
    writeSettings: (next) => {
      settings = { ...settings, ...next }
    },
    history,
    onChanged: () => undefined,
    onOpened: (page) => opened.push(page),
    onClosed: (id) => closed.push(id),
    onCommand: (command) => commands.push(command),
    onLogged: (_id: number, _entry: BrowserConsoleEntry) => undefined,
    onFavicon: (favicon) => favicons.push(favicon),
    sessionOpener: (session) => SESSIONS.get(session) ?? null,
    shareTargets: () => [...SESSIONS].map(([session, opener]) => ({ session, name: opener.name }))
  })
  current = {
    host,
    window,
    settings: () => settings,
    patch: (next) => {
      settings = { ...settings, ...next }
    },
    opened,
    closed,
    commands,
    favicons,
    history,
    store
  }
  return current
}

afterEach(() => {
  current?.host.shutdown()
  current?.store.close()
  current = null
  vi.useRealTimers()
})

/** A tab opened from the strip, and the fake view and page behind it. */
function openTab(h: Workspace, url?: string): { id: number; view: FakeWebContentsView; wc: FakeWebContents } {
  const { state, problem } = h.host.open(url === undefined ? {} : { url })
  if (state === null) throw new Error(`open refused: ${String(problem)}`)
  const view = FakeWebContentsView.created.at(-1)!
  return { id: state.id, view, wc: view.webContents! }
}

const stateOf = (h: Workspace, id: number): BrowserState => {
  const found = h.host.states(id)[0]
  if (found === undefined) throw new Error(`no browser tab ${String(id)}`)
  return found
}

const messages = (h: Workspace, id: number): string[] => h.host.entries(id).map((entry) => entry.message)

/**
 * What Electron hands `createWindow`: the window options, with the web contents
 * Chromium made for the page - or none, for a middle click.
 */
const handedOver = (guest?: FakeWebContents): BrowserWindowConstructorOptions =>
  (guest === undefined ? {} : { webContents: guest as unknown as WebContents }) as BrowserWindowConstructorOptions

const partition = (): FakeSession => {
  const found = fakeBrowser.partitions.get('persist:floti-browser')
  if (found === undefined) throw new Error('the browser partition was never configured')
  return found
}

describe('browser host - navigation and the address bar', () => {
  it('records a page in the history only once it has arrived there, counting visits, newest first', () => {
    const h = workspace()
    const { id, wc } = openTab(h)
    expect(wc.loads).toEqual([])

    h.host.navigate(id, 'localhost:3000')
    expect(wc.loads).toEqual(['http://localhost:3000/'])
    // Asked for is not arrived.
    expect(historyOf(h)).toEqual([])
    // Nor is an error page, which Chromium commits like any other navigation.
    wc.failLoad(-105, 'ERR_NAME_NOT_RESOLVED', 'http://localhost:3000/')
    expect(historyOf(h)).toEqual([])

    wc.commit('http://localhost:3000/', { title: 'Dev server' })
    expect(h.history.entries()).toEqual([
      { url: 'http://localhost:3000/', title: 'Dev server', visits: 1, lastVisit: expect.any(Number) }
    ])

    wc.commit('http://localhost:3000/docs')
    wc.commit('http://localhost:3000/')
    expect(historyOf(h)).toEqual(['http://localhost:3000/', 'http://localhost:3000/docs'])
    expect(h.history.entries().find((entry) => entry.url === 'http://localhost:3000/')?.visits).toBe(2)
  })

  it('takes the title a page says after it arrives, and a move within the page with the title it has', () => {
    const h = workspace()
    const { wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    wc.sayTitle('Inbox')
    wc.navigateInPage('http://localhost:3000/#mail')
    const titles = Object.fromEntries(h.history.entries().map((entry) => [entry.url, entry.title]))
    expect(titles).toEqual({ 'http://localhost:3000/': 'Inbox', 'http://localhost:3000/#mail': 'Inbox' })
  })

  it('never records a page an agent opened, nor its title', async () => {
    const h = workspace()
    const { wc } = await openAgentTab(h, SESSIONS.get(7)!, 'http://localhost:4000/')
    wc.commit('http://localhost:4000/next', { title: 'The agent’s page' })
    wc.navigateInPage('http://localhost:4000/next#more')
    expect(h.history.entries()).toEqual([])
  })

  it('suggests from the history, and forgets one page or all of them', () => {
    const h = workspace({ browserSearch: 'bing' })
    const { wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/', { title: 'Dev server' })
    wc.commit('https://example.com/', { title: 'Example Domain' })

    expect(h.host.suggest('local')).toEqual({
      text: 'local',
      // A word would search, so its Search row follows the page Enter opens.
      rows: [
        { kind: 'page', url: 'http://localhost:3000/', title: 'Dev server' },
        { kind: 'search', text: 'local', engine: 'Bing' }
      ],
      completion: { url: 'http://localhost:3000/', rest: 'host:3000' }
    })
    expect(h.host.suggest('example domain').rows).toEqual([
      { kind: 'search', text: 'example domain', engine: 'Bing' },
      { kind: 'page', url: 'https://example.com/', title: 'Example Domain' }
    ])

    h.host.forget('http://localhost:3000/')
    expect(h.host.suggest('local').rows).toEqual([{ kind: 'search', text: 'local', engine: 'Bing' }])
    expect(historyOf(h)).toEqual(['https://example.com/'])

    h.host.clearHistory()
    expect(h.host.suggest('').rows).toEqual([])
  })

  it("opens a tab beside a project on that project's last address, and one beside none empty", () => {
    const h = workspace()
    const project = 'C:\\Projects\\A Project'
    const first = h.host.open({ url: 'http://localhost:5173/', project })
    const firstPage = FakeWebContentsView.created.at(-1)!.webContents!
    expect(first.state?.project).toBe(project)
    firstPage.commit('http://localhost:5173/app')
    expect(h.settings().browserProjectUrls).toEqual({ 'c:\\projects\\a project': 'http://localhost:5173/app' })

    // The same project, however it is cased.
    const again = openTabFor(h, 'c:\\PROJECTS\\a project')
    expect(again.wc.loads).toEqual(['http://localhost:5173/app'])

    const elsewhere = openTabFor(h, 'C:\\Projects\\Another')
    expect(elsewhere.wc.loads).toEqual([])
    const none = openTabFor(h, null)
    expect(none.wc.loads).toEqual([])
    expect(stateOf(h, none.id).url).toBe('')
  })

  it('searches a phrase with the engine in Settings, and with searching off says so and loads nothing', () => {
    const h = workspace()
    const { id, wc } = openTab(h)
    h.host.navigate(id, 'what is a webcontentsview')
    expect(wc.loads).toEqual(['https://www.google.com/search?q=what%20is%20a%20webcontentsview'])
    // An address is still an address.
    h.host.navigate(id, 'localhost:3000')
    expect(wc.loads.at(-1)).toBe('http://localhost:3000/')

    h.patch({ browserSearch: 'duckduckgo' })
    h.host.navigate(id, 'helm')
    expect(wc.loads.at(-1)).toBe('https://duckduckgo.com/?q=helm')

    h.patch({ browserSearch: 'off' })
    const loaded = wc.loads.length
    const state = h.host.navigate(id, 'what is a webcontentsview')
    expect(state?.problem).toContain('searching from the address bar is off')
    expect(wc.loads).toHaveLength(loaded)
  })

  it('holds a search to the reach rule: on "This machine only" the results page is refused', () => {
    const h = workspace({ browserReach: 'local' })
    const { id, wc } = openTab(h)
    const state = h.host.navigate(id, 'helm desktop')
    expect(state?.problem).toContain('This machine only')
    expect(wc.loads).toEqual([])
  })

  it('takes a picture of the page for the window to show while the view stands down', async () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    expect(await h.host.snapshot(id)).toBe('data:image/png;base64,cGFnZQ==')
    wc.picture = ''
    expect(await h.host.snapshot(id)).toBeNull()
    expect(await h.host.snapshot(999)).toBeNull()
  })

  it('reloads ignoring the cache when asked to, and ordinarily otherwise', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    h.host.reload(id, true)
    h.host.reload(id, false)
    expect(wc.reloads).toEqual(['ignoring-cache', 'ordinary'])
  })

  it('zooms the page, three steps either way at most', () => {
    const h = workspace()
    const { id } = openTab(h, 'http://localhost:3000/')
    expect(h.host.zoom(id, 0.5)?.zoomLevel).toBe(0.5)
    expect(h.host.zoom(id, 9)?.zoomLevel).toBe(3)
    expect(h.host.zoom(id, -9)?.zoomLevel).toBe(-3)
    expect(h.host.zoom(id, 0)?.zoomLevel).toBe(0)
  })

  it('clears the shared browser profile and says so in the console', async () => {
    const h = workspace()
    const { id } = openTab(h, 'http://localhost:3000/')
    const before = partition().cleared
    await h.host.clearStorage(id)
    expect(partition().cleared).toBe(before + 1)
    expect(messages(h, id).at(-1)).toContain('Cleared cookies and storage')
  })

  it('finds in the page: a new query starts a search, the same query steps, and stopping clears the find state', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')

    // Electron's `findNext` is true for the request that *starts* a search.
    h.host.find(id, 'tok', true)
    h.host.find(id, 'token', true)
    expect(wc.finds).toEqual([
      { query: 'tok', forward: true, findNext: true },
      { query: 'token', forward: true, findNext: true }
    ])
    // The answer to the search the next keystroke typed over changes nothing.
    wc.emit('found-in-page', {}, { requestId: 1, matches: 9, activeMatchOrdinal: 1 })
    expect(stateOf(h, id).find).toEqual({ query: 'token', matches: 0, active: 0 })
    wc.emit('found-in-page', {}, { requestId: 2, matches: 3, activeMatchOrdinal: 1 })
    expect(stateOf(h, id).find).toEqual({ query: 'token', matches: 3, active: 1 })

    // The same query again steps through that search, backwards here.
    h.host.find(id, 'token', false)
    expect(wc.finds.at(-1)).toEqual({ query: 'token', forward: false, findNext: false })
    wc.emit('found-in-page', {}, { requestId: 3, matches: 3, activeMatchOrdinal: 3 })
    expect(stateOf(h, id).find).toEqual({ query: 'token', matches: 3, active: 3 })

    // The caret stays in the pane's find field: the page is never focused.
    expect(wc.focused).toBe(0)

    h.host.stopFind(id)
    expect(stateOf(h, id).find).toBeNull()
    expect(wc.stoppedFinding).toEqual(['clearSelection'])
    // Closing the field gives the page the caret back, and only then.
    expect(wc.focused).toBe(1)

    // An emptied find field is a stop too.
    h.host.find(id, 'token', true)
    h.host.find(id, '', true)
    expect(stateOf(h, id).find).toBeNull()
  })
})

/** A tab opened beside a project: `project: null` is "beside nothing". */
function openTabFor(h: Workspace, project: string | null): { id: number; wc: FakeWebContents } {
  const { state } = h.host.open({ project })
  if (state === null) throw new Error('open refused')
  return { id: state.id, wc: FakeWebContentsView.created.at(-1)!.webContents! }
}

describe('browser host - the address being loaded', () => {
  it('says what a new tab is loading until the page commits, while its committed address stays empty', () => {
    const h = workspace()
    const { id, wc } = openTab(h)

    const asked = h.host.navigate(id, 'localhost:3000')
    expect(asked?.pendingUrl).toBe('http://localhost:3000/')
    expect(asked?.url).toBe('')
    expect(stateOf(h, id).pendingUrl).toBe('http://localhost:3000/')

    wc.commit('http://localhost:3000/')
    expect(stateOf(h, id)).toMatchObject({ url: 'http://localhost:3000/', pendingUrl: null })
  })

  it("says the search results address for a phrase, and the new address over an open page's", () => {
    const h = workspace({ browserSearch: 'duckduckgo' })
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')

    h.host.navigate(id, 'cats and dogs')
    const searching = stateOf(h, id)
    expect(searching.pendingUrl).toBe(wc.loads.at(-1))
    expect(searching.pendingUrl).toContain('cats%20and%20dogs')
    expect(searching.url).toBe('http://localhost:3000/')
  })

  it('says what a tab opened at an address is loading, from the strip and from an agent', () => {
    const h = workspace()
    expect(openTab(h, 'http://localhost:3000/').id).toBeGreaterThan(0)
    expect(h.host.states().at(-1)?.pendingUrl).toBe('http://localhost:3000/')

    void h.host.openFor(SESSIONS.get(7)!, 'http://localhost:4000/')
    const agents = h.host.states().at(-1)!
    expect(agents.pendingUrl).toBe('http://localhost:4000/')
    // An agent's tools read the committed address, which is still nothing.
    expect(agents.url).toBe('')
  })

  it('follows a redirect of the address being loaded', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/login')
    wc.redirect('http://localhost:3000/home')
    expect(stateOf(h, id).pendingUrl).toBe('http://localhost:3000/home')
    wc.commit('http://localhost:3000/home')
    expect(stateOf(h, id).pendingUrl).toBeNull()
  })

  it("never moves the pending address to a redirect of a page's own navigation", () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/typed')
    wc.redirect('http://evil.example/', { initiator: 'http://localhost:3000/' })
    expect(stateOf(h, id).pendingUrl).toBe('http://localhost:3000/typed')
  })

  it("keeps the address being loaded while the old page moves within itself, and drops it when the load lands in-page", () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    h.host.navigate(id, 'http://localhost:4000/next')

    // A router's pushState or a scroll-spy's hash on the page being left.
    wc.navigateInPage('http://localhost:3000/#section-2')
    expect(stateOf(h, id)).toMatchObject({ url: 'http://localhost:3000/#section-2', pendingUrl: 'http://localhost:4000/next' })

    // A load that is itself an in-page move ends when it lands.
    h.host.navigate(id, 'http://localhost:3000/#top')
    wc.navigateInPage('http://localhost:3000/#top')
    expect(stateOf(h, id).pendingUrl).toBeNull()
  })

  it('keeps the address being loaded when back or forward have nowhere to go', () => {
    const h = workspace()
    const { id } = openTab(h, 'http://localhost:3000/first')
    expect(h.host.back(id)?.pendingUrl).toBe('http://localhost:3000/first')
    expect(h.host.forward(id)?.pendingUrl).toBe('http://localhost:3000/first')
  })

  it('leaves a load that failed with its address in the bar and nothing pending', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://nowhere.invalid/')
    wc.failLoad(-105, 'ERR_NAME_NOT_RESOLVED', 'http://nowhere.invalid/')
    expect(stateOf(h, id)).toMatchObject({ url: 'http://nowhere.invalid/', pendingUrl: null })
  })

  it('drops the pending address when a load ends with nothing committed, as a 204 does', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    h.host.navigate(id, 'http://localhost:3000/empty')
    wc.endWithoutCommit()
    expect(stateOf(h, id)).toMatchObject({ url: 'http://localhost:3000/', pendingUrl: null })
  })

  it('shows the newer address when a second load replaces the first', () => {
    const h = workspace()
    const { id } = openTab(h, 'http://localhost:3000/a')
    h.host.navigate(id, 'http://localhost:3000/b')
    expect(stateOf(h, id).pendingUrl).toBe('http://localhost:3000/b')
  })

  it("never shows where a link or the page's own script is taking it before it commits", () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')

    // A link clicked on a page that is not loading anything Floti asked for.
    wc.startNavigation('http://localhost:3000/linked', { initiator: 'http://localhost:3000/' })
    expect(stateOf(h, id).pendingUrl).toBeNull()
    wc.commit('http://localhost:3000/linked')

    // And a page navigating itself while an address typed in the bar loads:
    // the typed address is superseded, and the page's is not shown in its place.
    h.host.navigate(id, 'http://localhost:3000/typed')
    expect(stateOf(h, id).pendingUrl).toBe('http://localhost:3000/typed')
    wc.startNavigation('http://evil.example/', { initiator: 'http://localhost:3000/linked' })
    expect(stateOf(h, id)).toMatchObject({ url: 'http://localhost:3000/linked', pendingUrl: null })
  })

  it('drops the pending address for a navigation to somewhere else, and ignores frames and in-page moves', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    h.host.navigate(id, 'http://localhost:3000/next')

    wc.startNavigation('http://ads.example/frame', { mainFrame: false })
    wc.startNavigation('http://localhost:3000/#top', { sameDocument: true })
    expect(stateOf(h, id).pendingUrl).toBe('http://localhost:3000/next')

    // A navigation the browser started for somewhere else replaces the one asked for.
    wc.startNavigation('http://localhost:2000/')
    expect(stateOf(h, id).pendingUrl).toBeNull()
  })

  it('drops the pending address on back, forward and reload, which load something else', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/a')
    wc.commit('http://localhost:3000/a')
    h.host.navigate(id, 'http://localhost:3000/b')
    wc.commit('http://localhost:3000/b')

    h.host.navigate(id, 'http://localhost:3000/c')
    expect(h.host.back(id)?.pendingUrl).toBeNull()

    h.host.navigate(id, 'http://localhost:3000/c')
    expect(h.host.reload(id, false)?.pendingUrl).toBeNull()
  })

  it('stops a load: the page stops, nothing is pending and the bar is back on the committed address', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    h.host.navigate(id, 'http://localhost:3000/slow')
    expect(stateOf(h, id).loading).toBe(true)

    const stopped = h.host.stop(id)
    expect(wc.stopped).toBe(1)
    expect(stopped).toMatchObject({ url: 'http://localhost:3000/', pendingUrl: null, loading: false })
    expect(stateOf(h, id)).toMatchObject({ pendingUrl: null, loading: false })
  })

  it('stops the quiet reconnect too, so a stopped tab does not start loading again', () => {
    vi.useFakeTimers()
    const h = workspace()
    const url = 'http://localhost:5173/'
    const { id, wc } = openTab(h, url)
    wc.failLoad(-102, 'ERR_CONNECTION_REFUSED', url)
    expect(stateOf(h, id).retryingUntil).not.toBeNull()

    h.host.stop(id)
    expect(stateOf(h, id)).toMatchObject({ retryingUntil: null, problem: null })
    vi.advanceTimersByTime(60_000)
    expect(wc.loads).toEqual([url])
  })

  it('answers null for a tab that is not there', () => {
    const h = workspace()
    expect(h.host.stop(99)).toBeNull()
  })
})

describe('browser host - a dev server that is not up yet', () => {
  it('retries a refused connection on its own and connects once the server answers', () => {
    vi.useFakeTimers()
    const h = workspace()
    const url = 'http://localhost:5173/'
    const { id, wc } = openTab(h, url)
    expect(wc.loads).toEqual([url])
    const start = Date.now()

    wc.failLoad(-102, 'ERR_CONNECTION_REFUSED', url)
    const waiting = stateOf(h, id)
    expect(waiting.problem).toContain('Waiting for')
    expect(waiting.problem).toContain(url)
    expect(waiting.retryingUntil).toBe(start + 30_000)
    expect(historyOf(h)).toEqual([])

    // The backoff: a third of a second, then twice that.
    vi.advanceTimersByTime(299)
    expect(wc.loads).toHaveLength(1)
    vi.advanceTimersByTime(1)
    expect(wc.loads).toEqual([url, url])
    wc.failLoad(-102, 'ERR_CONNECTION_REFUSED', url)
    vi.advanceTimersByTime(599)
    expect(wc.loads).toHaveLength(2)
    vi.advanceTimersByTime(1)
    expect(wc.loads).toHaveLength(3)
    // The deadline runs from the first refusal, not from each one.
    expect(stateOf(h, id).retryingUntil).toBe(start + 30_000)

    // The server is up: the third attempt arrives. Nothing was pressed.
    wc.commit(url, { title: 'Dev server' })
    const loaded = stateOf(h, id)
    expect(loaded.problem).toBeNull()
    expect(loaded.retryingUntil).toBeNull()
    expect(loaded.title).toBe('Dev server')
    expect(historyOf(h)).toEqual([url])
    vi.advanceTimersByTime(60_000)
    expect(wc.loads).toHaveLength(3)
  })

  it('gives up after thirty seconds and says to reload', () => {
    vi.useFakeTimers()
    const h = workspace()
    const url = 'http://localhost:5173/'
    const { id, wc } = openTab(h, url)
    const start = Date.now()
    wc.failLoad(-102, 'ERR_CONNECTION_REFUSED', url)
    for (let attempt = 0; attempt < 40 && stateOf(h, id).retryingUntil !== null; attempt++) {
      vi.advanceTimersToNextTimer()
      wc.failLoad(-102, 'ERR_CONNECTION_REFUSED', url)
    }
    const gaveUp = stateOf(h, id)
    expect(gaveUp.retryingUntil).toBeNull()
    expect(gaveUp.problem).toContain('30 seconds')
    expect(gaveUp.problem).toContain('reload')
    // At the first refusal past the deadline, which is at most one 4s step late.
    expect(Date.now() - start).toBeGreaterThanOrEqual(30_000)
    expect(Date.now() - start).toBeLessThanOrEqual(34_000)

    const attempts = wc.loads.length
    vi.advanceTimersByTime(60_000)
    expect(wc.loads).toHaveLength(attempts)
  })

  it('retries nothing but a refused connection: a name or a certificate is an answer', () => {
    vi.useFakeTimers()
    const h = workspace()

    const name = openTab(h, 'http://nowhere.invalid/')
    name.wc.failLoad(-105, 'ERR_NAME_NOT_RESOLVED', 'http://nowhere.invalid/')
    expect(stateOf(h, name.id).retryingUntil).toBeNull()
    expect(stateOf(h, name.id).problem).toContain('ERR_NAME_NOT_RESOLVED')

    const cert = openTab(h, 'https://127.0.0.2:8443/')
    cert.wc.failLoad(-202, 'ERR_CERT_AUTHORITY_INVALID', 'https://127.0.0.2:8443/')
    const refused = stateOf(h, cert.id)
    expect(refused.retryingUntil).toBeNull()
    expect(refused.problem).toContain('certificate')
    expect(refused.problem).toContain('no way past')

    vi.advanceTimersByTime(60_000)
    expect(name.wc.loads).toHaveLength(1)
    expect(cert.wc.loads).toHaveLength(1)
  })

  it('says nothing about a load a newer one replaced', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.failLoad(-3, 'ERR_ABORTED', 'http://localhost:3000/', { errorPage: false })
    expect(stateOf(h, id).problem).toBeNull()
    expect(h.host.entries(id)).toEqual([])
  })

  it('stops retrying when the tab is reloaded or sent somewhere else', () => {
    vi.useFakeTimers()
    const h = workspace()
    const url = 'http://localhost:5173/'
    const { id, wc } = openTab(h, url)
    wc.failLoad(-102, 'ERR_CONNECTION_REFUSED', url)
    h.host.navigate(id, 'http://localhost:4000/')
    expect(stateOf(h, id).retryingUntil).toBeNull()
    vi.advanceTimersByTime(60_000)
    expect(wc.loads).toEqual([url, 'http://localhost:4000/'])
  })
})

describe('browser host - where the view is', () => {
  it('paints nothing until placed, then sits on the placeholder in DIPs at the window zoom', () => {
    const h = workspace()
    h.window.webContents.zoomFactor = 1.5
    const { id, view } = openTab(h, 'http://localhost:3000/')
    expect(h.window.children).toContain(view)
    expect(view.visible).toBe(false)
    expect(view.getBounds()).toEqual({ x: 0, y: 0, width: 0, height: 0 })

    // A phone-width placeholder: 390 CSS pixels at 150% is 585 DIPs.
    h.host.bounds({ id, x: 10, y: 100, width: 390, height: 400, visible: true })
    expect(view.getBounds()).toEqual({ x: 15, y: 150, width: 585, height: 600 })
    expect(view.visible).toBe(true)
  })

  it("is clear while empty and white from the first page on, the ground a browser gives a page that sets none", () => {
    const h = workspace()
    const { id, view, wc } = openTab(h)
    expect(view.background).toBe('#00000000')
    h.host.navigate(id, 'localhost:3000')
    expect(view.background).toBe('#00000000')
    wc.commit('http://localhost:3000/')
    expect(view.background).toBe('#ffffff')
  })

  it('never lets the view into the top 36px, whatever the window reports', () => {
    const h = workspace()
    const { id, view } = openTab(h, 'http://localhost:3000/')
    h.host.bounds({ id, x: 0, y: 0, width: 800, height: 500, visible: true })
    expect(view.getBounds()).toEqual({ x: 0, y: 36, width: 800, height: 464 })
    h.host.bounds({ id, x: 0, y: 10, width: 800, height: 20, visible: true })
    expect(view.getBounds()).toEqual({ x: 0, y: 36, width: 800, height: 0 })
  })

  it('hides a view by not painting it, and keeps its page and its history', () => {
    const h = workspace()
    const { id, view, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    wc.commit('http://localhost:3000/two')
    h.host.bounds({ id, x: 0, y: 80, width: 800, height: 500, visible: true })

    h.host.bounds({ id, x: 0, y: 80, width: 800, height: 500, visible: false })
    expect(view.visible).toBe(false)
    expect(h.window.children).toContain(view)
    expect(wc.isDestroyed()).toBe(false)
    expect(stateOf(h, id)).toMatchObject({ url: 'http://localhost:3000/two', canGoBack: true })

    h.host.bounds({ id, x: 0, y: 80, width: 800, height: 500, visible: true })
    expect(view.visible).toBe(true)
  })

  it('goes back and forward through the history the page made', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    wc.commit('http://localhost:3000/two')
    expect(h.host.back(id)?.url).toBe('http://localhost:3000/')
    expect(stateOf(h, id)).toMatchObject({ canGoBack: false, canGoForward: true })
    expect(h.host.forward(id)?.url).toBe('http://localhost:3000/two')
  })
})

describe('browser host - the console', () => {
  it("keeps what the page logs, counts errors and warnings, and holds the last thousand lines", () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.log('info', 'hello', 'http://localhost:3000/app.js', 4)
    wc.log('warning', 'careful')
    wc.log('error', 'broken')
    expect(h.host.entries(id).map(({ level, message, source, line }) => ({ level, message, source, line }))).toEqual([
      { level: 'info', message: 'hello', source: 'http://localhost:3000/app.js', line: 4 },
      { level: 'warning', message: 'careful', source: '', line: 0 },
      { level: 'error', message: 'broken', source: '', line: 0 }
    ])
    expect(stateOf(h, id).errors).toBe(2)

    // An event in a shape Helm does not know is recorded, not dropped.
    wc.emit('console-message', { message: 'no level' })
    expect(messages(h, id).at(-1)).toContain('does not recognise')

    for (let n = 0; n < 1000; n++) wc.log('info', `line ${String(n)}`)
    expect(h.host.entries(id)).toHaveLength(1000)
    expect(messages(h, id)[0]).toBe('line 0')
    expect(messages(h, id).at(-1)).toBe('line 999')
  })

  it('evaluates in the page as a user gesture and prints what came back', async () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.answer = (source) => {
      if (source === 'thing') return { a: 1 }
      if (source === 'text') return 'FIXTURE-VALUE'
      if (source === 'nothing') return undefined
      throw new Error('ReferenceError: boom is not defined')
    }
    expect(await h.host.evaluate(id, 'thing')).toEqual({ ok: true, value: '{\n  "a": 1\n}', error: null })
    expect(await h.host.evaluate(id, 'text')).toEqual({ ok: true, value: 'FIXTURE-VALUE', error: null })
    expect(await h.host.evaluate(id, 'nothing')).toEqual({ ok: true, value: 'undefined', error: null })
    const failed = await h.host.evaluate(id, 'boom')
    expect(failed.ok).toBe(false)
    expect(failed.error).toContain('boom is not defined')
    expect(wc.evaluated.every((call) => call.userGesture)).toBe(true)

    h.host.close(id)
    expect((await h.host.evaluate(id, 'thing')).ok).toBe(false)
  })
})

describe('browser host - security posture', () => {
  it('on "This machine only", refuses a remote address with a sentence and fetches nothing; loopback still loads', () => {
    const h = workspace({ browserReach: 'local' })
    const { id, wc } = openTab(h)

    h.host.navigate(id, 'http://example.com/docs')
    expect(wc.loads).toEqual([])
    const refused = stateOf(h, id)
    expect(refused.problem).toContain('example.com')
    expect(refused.problem).toContain('This machine only')
    expect(h.host.entries(id).at(-1)?.level).toBe('error')

    h.host.navigate(id, 'http://127.0.0.1:8080/')
    expect(wc.loads).toEqual(['http://127.0.0.1:8080/'])
    expect(stateOf(h, id).problem).toBeNull()
    wc.commit('http://127.0.0.1:8080/')

    // A link the page follows goes through the same rule.
    expect(browserWillNavigate(wc.id, 'http://example.com/')).toBe(false)
    expect(stateOf(h, id).problem).toContain('example.com')
    expect(browserWillNavigate(wc.id, 'http://localhost:3000/next')).toBe(true)

    // The setting is read per navigation, not when the tab was made.
    h.patch({ browserReach: 'web' })
    h.host.navigate(id, 'http://example.com/docs')
    expect(wc.loads.at(-1)).toBe('http://example.com/docs')
  })

  it('runs every page sandboxed, isolated, with no preload, on the persistent browser partition', () => {
    const h = workspace()
    const { view } = openTab(h, 'http://localhost:3000/')
    const prefs = view.webPreferences
    expect(prefs['session']).toBe(partition())
    expect(prefs).toMatchObject({
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
      nodeIntegrationInSubFrames: false,
      webviewTag: false,
      webSecurity: true,
      allowRunningInsecureContent: false
    })
    expect(prefs['preload']).toBeUndefined()
  })

  it('refuses a file URL with a sentence and stays where it was', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    h.host.navigate(id, 'file:///C:/Windows/win.ini')
    expect(wc.loads).toEqual(['http://localhost:3000/'])
    expect(stateOf(h, id)).toMatchObject({ url: 'http://localhost:3000/' })
    expect(stateOf(h, id).problem).toContain('http and https')
    expect(browserWillNavigate(wc.id, 'file:///C:/Windows/win.ini')).toBe(false)
  })

  describe('a tab refused its first address', () => {
    it('opened at a link out of reach says that address and is named after its host, with no page claimed', () => {
      const h = workspace({ browserReach: 'local' })
      // `browser:open` with an address: how a plugin's link and the user's own arrive.
      const { state, problem } = h.host.open({ url: 'https://example.com/docs' })
      const wc = FakeWebContentsView.created.at(-1)!.webContents!
      expect(wc.loads).toEqual([])
      expect(problem).toContain('This machine only')
      expect(state).toMatchObject({
        url: '',
        refusedUrl: 'https://example.com/docs',
        host: 'example.com',
        title: 'example.com',
        loading: false,
        canGoBack: false
      })
      expect(state?.problem).toContain('example.com')
    })

    it("opened beside a project whose remembered address is out of reach says that address", () => {
      const h = workspace({
        browserReach: 'local',
        browserProjectUrls: { 'c:\\projects\\site': 'https://example.com/app' }
      })
      const { id, wc } = openTabFor(h, 'C:\\Projects\\Site')
      expect(wc.loads).toEqual([])
      expect(stateOf(h, id)).toMatchObject({
        url: '',
        refusedUrl: 'https://example.com/app',
        host: 'example.com',
        title: 'example.com'
      })
    })

    it("opened for a session at an address out of reach says that address", async () => {
      const h = workspace({ browserReach: 'local' })
      vi.useFakeTimers()
      const pending = h.host.openFor(SESSIONS.get(7)!, 'https://example.com/')
      const wc = FakeWebContentsView.created.at(-1)!.webContents!
      await vi.advanceTimersByTimeAsync(1000)
      const { state, problem } = await pending
      expect(wc.loads).toEqual([])
      expect(problem).toContain('example.com')
      expect(state).toMatchObject({
        url: '',
        refusedUrl: 'https://example.com/',
        host: 'example.com',
        title: 'example.com',
        openedBy: 'alpha'
      })
    })

    it('names an address with no host by the address itself', () => {
      const h = workspace()
      const { id } = openTab(h, 'file:///C:/Windows/win.ini')
      expect(stateOf(h, id)).toMatchObject({
        url: '',
        refusedUrl: 'file:///C:/Windows/win.ini',
        host: '',
        title: 'file:///C:/Windows/win.ini'
      })
    })

    it('says the page once one loads, and a later refusal leaves the page that is there', () => {
      const h = workspace({ browserReach: 'local' })
      const { id, wc } = openTab(h, 'https://example.com/')
      expect(stateOf(h, id).refusedUrl).toBe('https://example.com/')

      h.host.navigate(id, 'localhost:3000')
      expect(stateOf(h, id).refusedUrl).toBeNull()
      wc.commit('http://localhost:3000/')

      h.host.navigate(id, 'https://example.com/')
      expect(stateOf(h, id)).toMatchObject({ url: 'http://localhost:3000/', refusedUrl: null, host: 'localhost:3000' })
      expect(stateOf(h, id).problem).toContain('example.com')
    })
  })

  it('holds ten tabs at most, from the strip and from a page', () => {
    const h = workspace()
    const tabs = Array.from({ length: 10 }, () => openTab(h, 'http://localhost:3000/'))
    const made = FakeWebContentsView.created.length

    const eleventh = h.host.open({ url: 'http://localhost:3000/' })
    expect(eleventh.state).toBeNull()
    expect(eleventh.problem).toContain('10')

    const page = tabs[0]!
    expect(
      browserWindowOpen(page.wc.id, { url: 'http://localhost:3000/two', disposition: 'foreground-tab', features: '' })
    ).toEqual({ action: 'deny' })
    expect(stateOf(h, page.id).problem).toContain('10')
    expect(h.host.entries(page.id).at(-1)?.level).toBe('warning')
    expect(FakeWebContentsView.created.length).toBe(made)
  })

  it("exempts exactly the live browser views from the app's navigation lock, never the window", () => {
    const h = workspace()
    expect(exemptedWebContents()).toEqual([])
    const a = openTab(h, 'http://localhost:3000/')
    const b = openTab(h, 'http://localhost:3001/')
    expect([...exemptedWebContents()].sort()).toEqual([a.wc.id, b.wc.id].sort())

    const windowId = h.window.webContents.id
    expect(exemptedWebContents()).not.toContain(windowId)
    expect(browserWillNavigate(windowId, 'http://localhost:3000/')).toBe(false)
    expect(
      browserWindowOpen(windowId, { url: 'http://localhost:3000/', disposition: 'new-window', features: 'width=400' })
    ).toEqual({ action: 'deny' })

    h.host.close(a.id)
    expect(exemptedWebContents()).toEqual([b.wc.id])
    b.view.closeItself()
    expect(exemptedWebContents()).toEqual([])
    expect(browserWillNavigate(b.wc.id, 'http://localhost:3001/')).toBe(false)
  })

  it('sends a user agent without the Electron token and the app name, with the Chrome version unchanged', () => {
    const h = workspace()
    openTab(h)
    expect(partition().userAgent).toBe(
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.7871.212 Safari/537.36'
    )
  })

  it('keeps session cookies with one listener on the partition, however many tabs open', () => {
    const h = workspace()
    openTab(h)
    openTab(h)
    expect(partition().cookies.listenerCount('changed')).toBe(1)
  })

  it('writes the cookie store to disk when a page stops loading, and on a quit', async () => {
    const h = workspace()
    const { wc } = openTab(h, 'http://localhost:3000/')
    const cookies = partition().cookies
    const before = cookies.flushed
    wc.emit('did-stop-loading')
    expect(cookies.flushed).toBe(before + 1)
    await flushBrowserCookies()
    expect(cookies.flushed).toBe(before + 2)
  })

  it('accepts a self-signed certificate on loopback and hands every other verdict back to Chromium', () => {
    const h = workspace()
    openTab(h)
    const verify = partition().certificateVerify!
    const verdict = (hostname: string): number | null => {
      let answer: number | null = null
      verify({ hostname }, (value) => {
        answer = value
      })
      return answer
    }
    expect(verdict('127.0.0.1')).toBe(0)
    expect(verdict('localhost')).toBe(0)
    expect(verdict('::1')).toBe(0)
    expect(verdict('127.0.0.2')).toBe(-3)
    expect(verdict('example.com')).toBe(-3)
    expect(verdict('localhost.example.com')).toBe(-3)
  })

  it('refuses a download, hands a web address to the system browser, and says so on every tab', () => {
    const h = workspace()
    const a = openTab(h, 'http://127.0.0.1:8080/download')
    a.wc.commit('http://127.0.0.1:8080/download')
    const b = openTab(h, 'http://127.0.0.1:8080/')
    const handedOff = fakeBrowser.openedExternally.length

    let prevented = false
    partition().emit(
      'will-download',
      { preventDefault: () => (prevented = true) },
      { getURL: () => 'http://127.0.0.1:8080/payload.bin' }
    )
    expect(prevented).toBe(true)
    expect(fakeBrowser.openedExternally.slice(handedOff)).toEqual(['http://127.0.0.1:8080/payload.bin'])
    for (const tab of [a, b]) {
      expect(stateOf(h, tab.id).problem).toContain('does not download')
      expect(messages(h, tab.id).at(-1)).toContain('download refused')
    }
    expect(a.wc.loads).toEqual(['http://127.0.0.1:8080/download'])
    expect(stateOf(h, a.id).url).toBe('http://127.0.0.1:8080/download')

    // Anything that is not a web address is refused and handed nowhere.
    let alsoPrevented = false
    partition().emit(
      'will-download',
      { preventDefault: () => (alsoPrevented = true) },
      { getURL: () => 'file:///C:/payload.exe' }
    )
    expect(alsoPrevented).toBe(true)
    expect(fakeBrowser.openedExternally.slice(handedOff)).toEqual(['http://127.0.0.1:8080/payload.bin'])
  })

  it('grants clipboard writing and fullscreen to the page that has the keyboard, and nothing else to anything', () => {
    const h = workspace()
    const { wc } = openTab(h, 'http://127.0.0.1:8080/')
    const ask = (contents: unknown, permission: string): boolean => {
      let answer: boolean | null = null
      partition().permissionRequest!(contents, permission, (granted) => (answer = granted))
      if (answer === null) throw new Error(`${permission} was never answered`)
      return answer
    }

    wc.hasFocus = true
    expect(ask(wc, 'clipboard-sanitized-write')).toBe(true)
    expect(ask(wc, 'fullscreen')).toBe(true)
    for (const permission of ['clipboard-read', 'geolocation', 'media', 'notifications', 'openExternal']) {
      expect(ask(wc, permission), permission).toBe(false)
    }

    // A page in the background - an agent's, say - gets neither.
    wc.hasFocus = false
    expect(ask(wc, 'clipboard-sanitized-write')).toBe(false)
    expect(ask(wc, 'fullscreen')).toBe(false)

    // Nor do contents that are not a page in the Browser tab: a popup, the app.
    const popup = new FakeWebContents()
    popup.hasFocus = true
    expect(ask(popup, 'clipboard-sanitized-write')).toBe(false)
    expect(ask(popup, 'fullscreen')).toBe(false)

    expect(partition().permissionCheck!(wc, 'clipboard-sanitized-write', 'http://127.0.0.1:8080', {})).toBe(true)
    expect(partition().permissionCheck!(wc, 'geolocation', 'http://127.0.0.1:8080', {})).toBe(false)
    expect(partition().permissionCheck!(null, 'fullscreen', '', {})).toBe(false)
    expect(partition().devicePermission!({ deviceType: 'usb' })).toBe(false)
  })
})

describe('browser host - the keyboard inside a page', () => {
  it("takes the Browser tab's keys before the page sees them, and leaves the page every other key", () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')

    expect(wc.press({ key: 'w', control: true })).toBe(true)
    expect(wc.press({ key: 'F5' })).toBe(true)
    expect(wc.press({ key: 'T', control: true, shift: true })).toBe(true)
    expect(h.commands).toEqual([
      { id, command: 'close-page' },
      { id, command: 'reload' },
      { id, command: 'reopen-page' }
    ])
    // A command drawn in Helm's chrome takes the keyboard back to the window
    // first; a reload leaves it in the page.
    expect(h.window.webContents.focused).toBe(2)

    // The page's own keys, a key coming back up and AltGr pass through.
    expect(wc.press({ key: 'a' })).toBe(false)
    expect(wc.press({ key: 'n', control: true })).toBe(false)
    expect(wc.press({ key: 'w', control: true, type: 'keyUp' })).toBe(false)
    expect(wc.press({ key: 'w', control: true, alt: true })).toBe(false)
    expect(h.commands).toHaveLength(3)

    // Held: Ctrl+W is swallowed but closes nothing more; Ctrl+= keeps zooming.
    expect(wc.press({ key: 'w', control: true, isAutoRepeat: true })).toBe(true)
    expect(wc.press({ key: '=', control: true, isAutoRepeat: true })).toBe(true)
    expect(h.commands.slice(3)).toEqual([{ id, command: 'zoom-in' }])
  })

  it('takes Escape to stop a page while it loads, and leaves the page its Escape otherwise', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')

    // Loading: Escape is the browser's.
    expect(wc.press({ key: 'Escape' })).toBe(true)
    expect(h.commands).toEqual([{ id, command: 'stop' }])
    // It stops the page where it is; the keyboard stays there.
    expect(h.window.webContents.focused).toBe(0)
    expect(wc.press({ key: 'Escape', shift: true })).toBe(false)

    // Loaded: a dialog, a menu or a game in the page has its Escape.
    wc.commit('http://localhost:3000/')
    expect(wc.press({ key: 'Escape' })).toBe(false)
    expect(h.commands).toHaveLength(1)
  })

  it('takes Escape to stop a loading page on macOS too', () => {
    const h = workspace({}, 'darwin')
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    expect(wc.press({ key: 'Escape' })).toBe(true)
    expect(h.commands).toEqual([{ id, command: 'stop' }])
  })

  it('leaves Escape to a page that has the whole screen, even while it loads', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    h.host.bounds({ id, x: 0, y: 100, width: 800, height: 500, visible: true })
    wc.enterFullscreen()
    expect(wc.press({ key: 'Escape' })).toBe(false)
    expect(h.commands).toEqual([])
  })

  it("takes the Mac's Cmd chords on macOS, and leaves the page its Ctrl chords and Option's characters", () => {
    const h = workspace({}, 'darwin')
    const { id, wc } = openTab(h, 'http://localhost:3000/')

    expect(wc.press({ key: 'w', meta: true })).toBe(true)
    expect(wc.press({ key: 'r', meta: true })).toBe(true)
    expect(wc.press({ key: 'T', meta: true, shift: true })).toBe(true)
    expect(wc.press({ key: '[', meta: true })).toBe(true)
    expect(h.commands).toEqual([
      { id, command: 'close-page' },
      { id, command: 'reload' },
      { id, command: 'reopen-page' },
      { id, command: 'back' }
    ])
    expect(h.window.webContents.focused).toBe(2)

    // Ctrl+W and F5 are Windows' keys, a page's own on a Mac; so is a
    // character typed with Option, and a Cmd chord coming back up.
    expect(wc.press({ key: 'w', control: true })).toBe(false)
    expect(wc.press({ key: 'F5' })).toBe(false)
    expect(wc.press({ key: 'ArrowLeft', alt: true })).toBe(false)
    expect(wc.press({ key: 'w', meta: true, type: 'keyUp' })).toBe(false)
    expect(h.commands).toHaveLength(4)

    // Held: Cmd+W closes nothing more; Cmd+[ keeps going back.
    expect(wc.press({ key: 'w', meta: true, isAutoRepeat: true })).toBe(true)
    expect(wc.press({ key: '[', meta: true, isAutoRepeat: true })).toBe(true)
    expect(h.commands.slice(4)).toEqual([{ id, command: 'back' }])
  })

  it("leaves an agent's key presses to the page, even the Browser tab's own", async () => {
    vi.useFakeTimers()
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    const pressed = h.host.press(id, 'w', ['control'])
    const typed = h.host.typeInto(id, 'T')
    await vi.advanceTimersByTimeAsync(1000)
    await Promise.all([pressed, typed])
    expect(h.commands).toEqual([])
    // And the next key the user presses is theirs again.
    expect(wc.press({ key: 'w', control: true })).toBe(true)
    expect(h.commands).toEqual([{ id, command: 'close-page' }])
  })

  it("leaves an agent's Cmd+W to the page on macOS too", async () => {
    vi.useFakeTimers()
    const h = workspace({}, 'darwin')
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    const pressed = h.host.press(id, 'w', ['meta'])
    await vi.advanceTimersByTimeAsync(1000)
    await pressed
    expect(h.commands).toEqual([])
    expect(wc.press({ key: 'w', meta: true })).toBe(true)
    expect(h.commands).toEqual([{ id, command: 'close-page' }])
  })
})

/** A tab a session opened, loaded: what `browser_open` leaves behind. */
async function openAgentTab(
  h: Workspace,
  opener: BrowserOpener,
  url: string
): Promise<{ id: number; wc: FakeWebContents }> {
  vi.useFakeTimers()
  const pending = h.host.openFor(opener, url)
  const wc = FakeWebContentsView.created.at(-1)!.webContents!
  wc.commit(url)
  await vi.advanceTimersByTimeAsync(1000)
  const { state } = await pending
  if (state === null) throw new Error('openFor refused')
  return { id: state.id, wc }
}

describe('browser host - a page shared with a session', () => {
  const alpha = SESSIONS.get(7)!
  const beta = SESSIONS.get(8)!

  it('shares a page the user opened with one session at a time, by its id, and takes it back', () => {
    const h = workspace()
    const { id } = openTab(h, 'http://localhost:3000/')
    expect(stateOf(h, id).sharedWith).toBeNull()
    expect(h.host.shareTargets()).toEqual([
      { session: 7, name: 'alpha' },
      { session: 8, name: 'beta' }
    ])

    expect(h.host.share(id, 7)?.sharedWith).toEqual({ session: 7, name: 'alpha' })
    expect(h.host.sharedWith(id)).toEqual(alpha)
    // A new share replaces the old one: one session drives a page at a time.
    expect(h.host.share(id, 8)?.sharedWith).toEqual({ session: 8, name: 'beta' })
    expect(h.host.sharedWith(id)).toEqual(beta)
    // A session that cannot take one - ended, no tools - changes nothing.
    expect(h.host.share(id, 99)?.sharedWith).toEqual({ session: 8, name: 'beta' })
    expect(h.host.share(id, null)?.sharedWith).toBeNull()
    expect(h.host.sharedWith(id)).toBeNull()
  })

  it("never shares a page an agent opened: it is that session's already", async () => {
    const h = workspace()
    const { id } = await openAgentTab(h, alpha, 'http://127.0.0.1:8080/')
    expect(h.host.share(id, 8)?.sharedWith).toBeNull()
    expect(h.host.sharedWith(id)).toBeNull()
  })

  it('lets the session it is shared with send it somewhere, and leaves closing it to the user', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    h.host.share(id, 7)

    expect(h.host.navigateFor(alpha, id, 'http://localhost:3000/next').problem).toBeNull()
    expect(wc.loads.at(-1)).toBe('http://localhost:3000/next')
    const other = h.host.navigateFor(beta, id, 'http://localhost:3000/other')
    expect(other.state).toBeNull()
    expect(other.problem).toContain('is not shared with this session')

    const closing = h.host.closeFor(alpha, id)
    expect(closing).toEqual({
      closed: false,
      problem: `Browser tab ${String(id)} is the user's. They shared it with this session to read and drive, and closing it is theirs.`
    })
    expect(h.host.states(id)).toHaveLength(1)
  })

  it('takes back what was shared with a session when it ends, and its own pages stop saying it can drive them', async () => {
    const h = workspace()
    const user = openTab(h, 'http://localhost:3000/')
    h.host.share(user.id, 7)
    const own = await openAgentTab(h, alpha, 'http://127.0.0.1:8080/')
    expect(stateOf(h, own.id)).toMatchObject({ openedBy: 'alpha', openerRunning: true })

    h.host.revoke('token-beta')
    expect(stateOf(h, user.id).sharedWith).toEqual({ session: 7, name: 'alpha' })

    h.host.revoke('token-alpha')
    expect(stateOf(h, user.id).sharedWith).toBeNull()
    // Who opened it is history and stays; that it can drive the page does not.
    expect(stateOf(h, own.id)).toMatchObject({ openedBy: 'alpha', openerRunning: false })
  })

  it('shares only the page it was given: a page that one opens is the user\'s alone', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    h.host.share(id, 7)
    const answer = browserWindowOpen(wc.id, {
      url: 'http://localhost:3000/elsewhere',
      disposition: 'foreground-tab',
      features: ''
    })
    if (answer.action !== 'allow' || answer.createWindow === undefined) throw new Error('the page was refused')
    answer.createWindow(handedOver(new FakeWebContents()))
    const spawned = h.opened.at(-1)!.state
    expect(spawned.sharedWith).toBeNull()
    expect(h.host.sharedWith(spawned.id)).toBeNull()
  })
})

describe('browser host - the pointer and the wheel an agent uses', () => {
  it('hovers with a pointer move and nothing pressed, at whole pixels', async () => {
    vi.useFakeTimers()
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    const moved = h.host.hover(id, 40.6, 12.2)
    await vi.advanceTimersByTimeAsync(200)
    await moved
    expect(wc.inputs).toEqual([{ type: 'mouseMove', x: 41, y: 12 }])
  })

  it('scrolls with a precise wheel turn at the point, its delta the wheel\'s way round', async () => {
    vi.useFakeTimers()
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    const turned = h.host.scroll(id, 640, 400, 0, 300)
    await vi.advanceTimersByTimeAsync(200)
    await turned
    expect(wc.inputs).toEqual([
      { type: 'mouseMove', x: 640, y: 400 },
      {
        type: 'mouseWheel',
        x: 640,
        y: 400,
        deltaX: -0,
        deltaY: -300,
        hasPreciseScrollingDeltas: true,
        canScroll: true
      }
    ])
  })
})

describe('browser host - a page with the whole screen', () => {
  it('fills the window while the page has the screen, ignores the pane meanwhile, and goes back where it was', () => {
    const h = workspace()
    const { id, view, wc } = openTab(h, 'http://localhost:3000/')
    const other = openTab(h, 'http://localhost:3000/two')
    h.host.bounds({ id, x: 100, y: 80, width: 800, height: 500, visible: true })
    expect(h.window.children.at(-1)).toBe(other.view)

    h.window.fullScreen = true
    h.window.contentBounds = { x: 0, y: 0, width: 2560, height: 1440 }
    wc.enterFullscreen()
    // The whole window, the top 36px included, above every other view.
    expect(view.getBounds()).toEqual({ x: 0, y: 0, width: 2560, height: 1440 })
    expect(view.visible).toBe(true)
    expect(h.window.children.at(-1)).toBe(view)

    // The pane re-measures in a window laid out at that size; that is not where the page goes.
    h.host.bounds({ id, x: 0, y: 80, width: 2560, height: 1300, visible: false })
    expect(view.getBounds()).toEqual({ x: 0, y: 0, width: 2560, height: 1440 })
    expect(view.visible).toBe(true)

    // A window resized under it keeps the page filling it.
    h.window.contentBounds = { x: 0, y: 0, width: 1920, height: 1080 }
    h.window.emit('resize')
    expect(view.getBounds()).toEqual({ x: 0, y: 0, width: 1920, height: 1080 })

    h.window.fullScreen = false
    wc.leaveFullscreen()
    expect(view.getBounds()).toEqual({ x: 100, y: 80, width: 800, height: 500 })
    expect(view.visible).toBe(true)
    h.window.emit('resize')
    expect(view.getBounds()).toEqual({ x: 100, y: 80, width: 800, height: 500 })

    // And the pane is heard again.
    h.host.bounds({ id, x: 120, y: 80, width: 700, height: 500, visible: true })
    expect(view.getBounds()).toEqual({ x: 120, y: 80, width: 700, height: 500 })
  })

  it('gives the screen back when a page closes while it has it', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    h.host.bounds({ id, x: 100, y: 80, width: 800, height: 500, visible: true })
    h.window.fullScreen = true
    wc.enterFullscreen()
    h.host.close(id)
    expect(h.window.isFullScreen()).toBe(false)
  })
})

describe('browser host - popups and pages that close themselves', () => {
  it('retires the tab of a page that closed itself, and nothing touching that id throws afterwards', async () => {
    const h = workspace()
    const { id, view, wc } = openTab(h, 'http://127.0.0.1:8080/selfclose')
    wc.commit('http://127.0.0.1:8080/selfclose')

    view.closeItself()
    expect(h.closed).toEqual([id])
    expect(h.host.states().map((state) => state.id)).not.toContain(id)
    expect(h.host.states(id)).toEqual([])
    expect(() => h.host.bounds({ id, x: 0, y: 80, width: 400, height: 300, visible: true })).not.toThrow()
    expect(h.host.navigate(id, 'http://127.0.0.1:8080/')).toBeNull()
    expect(h.host.back(id)).toBeNull()
    expect(h.host.zoom(id, 1)).toBeNull()
    expect((await h.host.evaluate(id, '1')).ok).toBe(false)

    // Closing it from the strip afterwards tells the window nothing twice.
    h.host.close(id)
    expect(h.closed).toEqual([id])
  })

  it('answers rather than throws in the moment between a page closing and Electron saying so', () => {
    const h = workspace()
    const { id, view, wc } = openTab(h, 'http://127.0.0.1:8080/selfclose')
    wc.commit('http://127.0.0.1:8080/selfclose')
    view.closeItself({ announce: false })

    expect(() => h.host.states()).not.toThrow()
    expect(stateOf(h, id).url).toBe('')
    expect(() => h.host.bounds({ id, x: 0, y: 80, width: 400, height: 300, visible: true })).not.toThrow()
    expect(h.host.navigate(id, 'http://127.0.0.1:8080/')?.problem).toContain('closed itself')
    expect(h.host.zoom(id, 1)).toBeNull()
    expect(() => h.host.find(id, 'x', true)).not.toThrow()
    expect(() => h.host.stopFind(id)).not.toThrow()
  })

  it('opens target=_blank, a plain window.open and a middle click as pages that keep their opener, never windows', () => {
    const h = workspace()
    const opener = openTab(h, 'http://127.0.0.1:8080/opener')
    opener.wc.commit('http://127.0.0.1:8080/opener')
    const made = FakeWebContentsView.created.length
    const attached = h.window.children.length

    // A plain window.open: Chromium has already made the page's web contents,
    // and the view adopts them rather than loading the address afresh.
    const answer = browserWindowOpen(opener.wc.id, {
      url: 'http://127.0.0.1:8080/two',
      disposition: 'foreground-tab',
      features: ''
    })
    if (answer.action !== 'allow' || answer.createWindow === undefined) throw new Error('the page was refused')
    expect(answer.outlivesOpener).toBe(true)
    expect(answer.overrideBrowserWindowOptions?.webPreferences).toMatchObject({
      session: partition(),
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false
    })
    const guest = new FakeWebContents()
    expect(answer.createWindow(handedOver(guest))).toBe(guest)
    expect(FakeWebContentsView.created.at(-1)!.webContents).toBe(guest)
    expect(guest.loads).toEqual([])
    expect(exemptedWebContents()).toContain(guest.id)
    expect(h.window.children.length).toBe(attached + 1)
    const page = h.opened.at(-1)!
    expect(page).toMatchObject({ after: opener.id, background: false })
    expect(page.state.openedBy).toBeNull()

    // A middle click: no contents to adopt, so the address is loaded here, and
    // the page in front stays in front.
    const middle = browserWindowOpen(opener.wc.id, {
      url: 'http://127.0.0.1:8080/three',
      disposition: 'background-tab',
      features: ''
    })
    if (middle.action !== 'allow' || middle.createWindow === undefined) throw new Error('the page was refused')
    middle.createWindow(handedOver())
    expect(FakeWebContentsView.created.at(-1)!.webContents?.loads).toEqual(['http://127.0.0.1:8080/three'])
    expect(h.opened.at(-1)).toMatchObject({ after: opener.id, background: true })

    // A shift-click is new-window with nothing asked of the window: a page too.
    const shifted = browserWindowOpen(opener.wc.id, {
      url: 'http://127.0.0.1:8080/four',
      disposition: 'new-window',
      features: ''
    })
    expect(shifted.action === 'allow' && shifted.createWindow !== undefined).toBe(true)
    expect(shifted.action === 'allow' && shifted.overrideBrowserWindowOptions?.parent).toBeFalsy()

    // Pages, not windows, and they outlive the page that opened them.
    expect(FakeWebContentsView.created.length).toBe(made + 2)
    h.host.close(opener.id)
    expect(h.host.states().map((state) => state.id)).toEqual([page.state.id, page.state.id + 1])
  })

  it('gives window.open with features a real popup, held to the posture and the reach of the tab that opened it', () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://127.0.0.1:8080/signin')
    wc.commit('http://127.0.0.1:8080/signin')
    const made = FakeWebContentsView.created.length

    const answer = browserWindowOpen(wc.id, {
      url: 'http://127.0.0.1:8080/popup',
      disposition: 'new-window',
      features: 'width=480,height=5000'
    })
    if (answer.action !== 'allow') throw new Error('the popup was refused')
    expect(answer.outlivesOpener).toBe(false)
    const options = answer.overrideBrowserWindowOptions!
    expect(options.width).toBe(480)
    expect(options.height).toBeLessThan(5000)
    expect(options.parent).toBe(h.window)
    expect(options.webPreferences).toMatchObject({
      session: partition(),
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false
    })
    // A window, not a tab.
    expect(FakeWebContentsView.created.length).toBe(made)

    // Electron makes it.
    const popup = new FakeBrowserWindow()
    wc.emit('did-create-window', popup, { url: 'http://127.0.0.1:8080/popup' })
    expect(exemptedWebContents()).toContain(popup.webContents.id)
    expect(messages(h, id)).toContain('opened a popup window for http://127.0.0.1:8080/popup')
    popup.webContents.commit('http://127.0.0.1:8080/popup', { title: 'Sign in' })
    expect(popup.title).toBe('127.0.0.1:8080 - Sign in')

    // A redirect inside it answers to the opener's reach, and is refused on the opener's tab.
    h.patch({ browserReach: 'local' })
    expect(browserWillNavigate(popup.webContents.id, 'http://example.com/oauth')).toBe(false)
    expect(stateOf(h, id).problem).toContain('example.com')
    expect(browserWillNavigate(popup.webContents.id, 'http://localhost:9000/callback')).toBe(true)

    // It closes itself.
    popup.destroy()
    expect(exemptedWebContents()).not.toContain(popup.webContents.id)
    expect(messages(h, id).at(-1)).toContain('popup window closed')
  })

  it('refuses a popup to an address out of reach, on the tab that asked', () => {
    const h = workspace({ browserReach: 'local' })
    const { id, wc } = openTab(h, 'http://127.0.0.1:8080/signin')
    wc.commit('http://127.0.0.1:8080/signin')
    const made = FakeWebContentsView.created.length
    expect(
      browserWindowOpen(wc.id, { url: 'http://127.0.0.2:9000/popup', disposition: 'new-window', features: 'width=480' })
    ).toEqual({ action: 'deny' })
    expect(FakeWebContentsView.created.length).toBe(made)
    expect(stateOf(h, id).problem).toContain('This machine only')
    expect(stateOf(h, id).problem).toContain('127.0.0.2')
  })

  it("gives window.open from a session's tab another of that session's pages, never a window", async () => {
    vi.useFakeTimers()
    const h = workspace()
    const opener = { key: 'token-alpha', name: 'agent alpha' }
    const pending = h.host.openFor(opener, 'http://127.0.0.1:8080/signin')
    const agentPage = FakeWebContentsView.created.at(-1)!.webContents!
    agentPage.commit('http://127.0.0.1:8080/signin')
    await vi.advanceTimersByTimeAsync(1000)
    const agentTab = (await pending).state!
    expect(agentTab.openedBy).toBe('agent alpha')

    // At the end of the strip, behind the page in front.
    expect(h.opened.at(-1)).toMatchObject({ after: null, background: true })

    const answer = browserWindowOpen(agentPage.id, {
      url: 'http://127.0.0.1:8080/popup',
      disposition: 'new-window',
      features: 'width=480,height=640'
    })
    if (answer.action !== 'allow' || answer.createWindow === undefined) throw new Error('the page was refused')
    // Not a window: nothing was handed to Electron to make one with.
    expect(answer.overrideBrowserWindowOptions?.parent).toBeUndefined()
    const guest = new FakeWebContents()
    answer.createWindow(handedOver(guest))
    const spawned = h.opened.at(-1)!
    expect(spawned.state.id).not.toBe(agentTab.id)
    expect(spawned).toMatchObject({ after: agentTab.id, background: true })
    expect(spawned.state.openedBy).toBe('agent alpha')
    expect(h.host.openerOf(spawned.state.id)).toEqual(opener)
    expect(FakeWebContentsView.created.at(-1)!.webContents).toBe(guest)
    guest.commit('http://127.0.0.1:8080/popup')
    await vi.advanceTimersByTimeAsync(1000)
  })
})

describe("browser host - a page's icon", () => {
  const PNG = new Uint8Array([0x89, 0x50, 0x4e, 0x47])
  const PNG_URL = `data:image/png;base64,${Buffer.from(PNG).toString('base64')}`
  const image = (): FakeRoute => ({ headers: { 'content-type': 'image/png' }, chunks: [PNG] })
  const net = fakeBrowser.net

  /** The network answers these addresses with an image, and refuses the rest. */
  const serve = (...urls: string[]): void => {
    for (const url of urls) net.routes.set(url, image())
  }

  afterEach(() => net.reset())

  it('fetches the icon a page names through the browser partition, with its cookies, and sends it once', async () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/', { title: 'App' })
    serve('http://localhost:3000/favicon.ico')
    wc.reportFavicons(['http://localhost:3000/favicon.ico'])
    await vi.waitFor(() => expect(h.favicons).toEqual([{ id, icon: PNG_URL }]))
    expect(net.asked).toEqual(['http://localhost:3000/favicon.ico'])
    expect(net.requests[0]!.options).toMatchObject({
      session: partition(),
      redirect: 'manual',
      useSessionCookies: true
    })

    // The same icon again - the page re-reporting it on another page of the
    // site - is no change, so nothing is sent; and a second tab on the site
    // gets it without a second fetch.
    wc.commit('http://localhost:3000/about')
    wc.reportFavicons(['http://localhost:3000/favicon.ico'])
    const other = openTab(h, 'http://localhost:3000/')
    other.wc.commit('http://localhost:3000/')
    other.wc.reportFavicons(['http://localhost:3000/favicon.ico'])
    await vi.waitFor(() => expect(h.favicons).toHaveLength(2))
    expect(h.favicons).toEqual([
      { id, icon: PNG_URL },
      { id: other.id, icon: PNG_URL }
    ])
    expect(net.asked).toEqual(['http://localhost:3000/favicon.ico'])
  })

  it('never fetches an icon the reach rule refuses', async () => {
    const h = workspace({ browserReach: 'local' })
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    serve('https://cdn.example.com/icon.png', 'http://localhost:3000/icon.png')
    wc.reportFavicons(['https://cdn.example.com/icon.png'])
    // The barrier: a later report the reach allows has landed, so the first is done.
    wc.reportFavicons(['http://localhost:3000/icon.png'])
    await vi.waitFor(() => expect(h.favicons).toEqual([{ id, icon: PNG_URL }]))
    expect(net.asked).toEqual(['http://localhost:3000/icon.png'])
  })

  it('follows an icon that redirects within reach, and never asks for a hop out of it', async () => {
    const h = workspace({ browserReach: 'local' })
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    net.routes.set('http://localhost:3000/favicon.ico', { redirect: 'http://127.0.0.1:4000/icon.png' })
    serve('http://127.0.0.1:4000/icon.png')
    wc.reportFavicons(['http://localhost:3000/favicon.ico'])
    await vi.waitFor(() => expect(h.favicons).toEqual([{ id, icon: PNG_URL }]))
    expect(net.asked).toEqual(['http://localhost:3000/favicon.ico', 'http://127.0.0.1:4000/icon.png'])

    const away = openTab(h, 'http://localhost:5173/')
    away.wc.commit('http://localhost:5173/')
    net.routes.set('http://localhost:5173/favicon.ico', { redirect: 'https://cdn.example.com/icon.png' })
    serve('https://cdn.example.com/icon.png')
    away.wc.reportFavicons(['http://localhost:5173/favicon.ico'])
    // The barrier: a later report on that tab, within reach, has landed.
    serve('http://localhost:5173/icon.png')
    away.wc.reportFavicons(['http://localhost:5173/icon.png'])
    await vi.waitFor(() => expect(h.favicons.at(-1)).toEqual({ id: away.id, icon: PNG_URL }))
    expect(net.asked).not.toContain('https://cdn.example.com/icon.png')
  })

  it("holds an agent's tab to the agent's reach", async () => {
    const h = workspace({ browserReach: 'web', browserMcpLocalOnly: true })
    const { id, wc } = await openAgentTab(h, SESSIONS.get(7)!, 'http://localhost:4000/')
    vi.useRealTimers()
    serve('https://cdn.example.com/icon.png', 'http://localhost:4000/icon.png')
    wc.reportFavicons(['https://cdn.example.com/icon.png'])
    wc.reportFavicons(['http://localhost:4000/icon.png'])
    await vi.waitFor(() => expect(h.favicons).toEqual([{ id, icon: PNG_URL }]))
    expect(net.asked).toEqual(['http://localhost:4000/icon.png'])

    // The user's own tab, under the same settings, may have the internet's icon.
    const mine = openTab(h, 'https://example.com/')
    mine.wc.commit('https://example.com/')
    mine.wc.reportFavicons(['https://cdn.example.com/icon.png'])
    await vi.waitFor(() => expect(h.favicons.at(-1)).toEqual({ id: mine.id, icon: PNG_URL }))
    expect(net.asked).toEqual(['http://localhost:4000/icon.png', 'https://cdn.example.com/icon.png'])
  })

  it('clears the icon when the page goes to another site, and keeps it on the same one', async () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    serve('http://localhost:3000/favicon.ico')
    wc.reportFavicons(['http://localhost:3000/favicon.ico'])
    await vi.waitFor(() => expect(h.favicons).toHaveLength(1))

    wc.commit('http://localhost:3000/next')
    expect(h.favicons).toHaveLength(1)
    wc.commit('http://localhost:5173/')
    expect(h.favicons).toEqual([
      { id, icon: PNG_URL },
      { id, icon: null }
    ])
  })

  it('drops an icon that lands after its page went to another site', async () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    net.routes.set('http://localhost:3000/favicon.ico', { hang: true })
    wc.reportFavicons(['http://localhost:3000/favicon.ico'])
    await vi.waitFor(() => expect(net.asked).toEqual(['http://localhost:3000/favicon.ico']))
    wc.commit('http://localhost:5173/')

    // A second tab still on the old site reports the same icon, so it waits on
    // the same fetch - and hears the answer after the first tab did.
    const other = openTab(h, 'http://localhost:3000/')
    other.wc.commit('http://localhost:3000/')
    other.wc.reportFavicons(['http://localhost:3000/favicon.ico'])
    const request = net.requests[0]!
    const response = new FakeIncomingMessage(200, { 'content-type': 'image/png' })
    request.emit('response', response)
    response.emit('data', Buffer.from(PNG))
    response.emit('end')
    await vi.waitFor(() => expect(h.favicons).toEqual([{ id: other.id, icon: PNG_URL }]))
    expect(h.favicons.some((favicon) => favicon.id === id)).toBe(false)
  })

  it('sends null when a page with an icon names none it may have', async () => {
    const h = workspace()
    const { id, wc } = openTab(h, 'http://localhost:3000/')
    wc.commit('http://localhost:3000/')
    serve('http://localhost:3000/favicon.ico')
    wc.reportFavicons(['http://localhost:3000/favicon.ico'])
    await vi.waitFor(() => expect(h.favicons).toHaveLength(1))
    wc.reportFavicons(['file:///C:/icon.png'])
    await vi.waitFor(() => expect(h.favicons).toHaveLength(2))
    expect(h.favicons).toEqual([
      { id, icon: PNG_URL },
      { id, icon: null }
    ])
  })
})
