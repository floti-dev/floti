import { EventEmitter } from 'node:events'

/**
 * The part of Electron the browser pane's main-process half touches, faked for
 * `main/browser.test.ts`:
 *
 *   vi.mock('electron', async () => ({
 *     ...(await import('../../test/electron')).electronFake(),
 *     ...(await import('../../test/browser-electron')).browserElectronFake()
 *   }))
 *
 * Helm's side is real; Chromium's side is played by the test. A fake web
 * contents records what Helm asked of it (`loads`, `reloads`, `finds`) and the
 * test raises the events Chromium would raise in reply (`commit`, `failLoad`,
 * `closeItself`). That split is what lets a test prove Helm's own behaviour -
 * the retry on a refused connection, say - with no Chromium underneath to do
 * the same thing on its own.
 *
 * Nothing here imports `electron` or `vitest`, so the mock factory can load it.
 */

let nextContentsId = 1000

/** The event object Electron passes first. Helm calls `preventDefault` on some. */
const electronEvent = (): { preventDefault: () => void } => ({ preventDefault: () => undefined })

/** A key as `before-input-event` reports it. */
export interface FakeKey {
  key: string
  control?: boolean
  shift?: boolean
  alt?: boolean
  meta?: boolean
  type?: 'keyDown' | 'keyUp'
  isAutoRepeat?: boolean
}

/**
 * The partition's cookie store, holding nothing. It counts flushes and records
 * what was set; `main/browserCookies.test.ts` plays the store's side itself.
 */
export class FakeCookies extends EventEmitter {
  flushed = 0
  readonly sets: Array<Record<string, unknown>> = []
  get(): Promise<never[]> {
    return Promise.resolve([])
  }
  set(details: Record<string, unknown>): Promise<void> {
    this.sets.push(details)
    return Promise.resolve()
  }
  flushStore(): Promise<void> {
    this.flushed += 1
    return Promise.resolve()
  }
}

/** Electron's default user agent as the app gets it: the app's token and Electron's included. */
export const ELECTRON_USER_AGENT =
  'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Floti/0.0.0-test Chrome/150.0.7871.212 Electron/43.3.0 Safari/537.36'

/** The handlers `browserSession()` installs, kept so a test can call them. */
export class FakeSession extends EventEmitter {
  readonly cookies = new FakeCookies()
  userAgent = ELECTRON_USER_AGENT
  getUserAgent(): string {
    return this.userAgent
  }
  setUserAgent(agent: string): void {
    this.userAgent = agent
  }
  permissionRequest: ((contents: unknown, permission: string, callback: (granted: boolean) => void) => void) | null =
    null
  permissionCheck: ((...args: unknown[]) => boolean) | null = null
  devicePermission: ((...args: unknown[]) => boolean) | null = null
  certificateVerify: ((request: { hostname: string }, callback: (verdict: number) => void) => void) | null = null
  cleared = 0

  setPermissionRequestHandler(handler: NonNullable<FakeSession['permissionRequest']>): void {
    this.permissionRequest = handler
  }
  setPermissionCheckHandler(handler: NonNullable<FakeSession['permissionCheck']>): void {
    this.permissionCheck = handler
  }
  setDevicePermissionHandler(handler: NonNullable<FakeSession['devicePermission']>): void {
    this.devicePermission = handler
  }
  setCertificateVerifyProc(proc: NonNullable<FakeSession['certificateVerify']>): void {
    this.certificateVerify = proc
  }
  clearStorageData(): Promise<void> {
    this.cleared += 1
    return Promise.resolve()
  }
}

/**
 * How the fake network answers one address: a redirect to `redirect`, or a
 * response. `chunks` is the body in pieces, `endless` a body that never ends
 * until the request is abandoned, `hang` an answer that never comes, `refuse`
 * a connection that fails.
 */
export interface FakeRoute {
  redirect?: string
  status?: number
  headers?: Record<string, string>
  chunks?: Uint8Array[]
  endless?: number
  hang?: boolean
  refuse?: boolean
}

/** Electron's `IncomingMessage`, as far as a body goes. */
export class FakeIncomingMessage extends EventEmitter {
  constructor(
    readonly statusCode: number,
    readonly headers: Record<string, string | string[]>
  ) {
    super()
  }
}

/**
 * Electron's `ClientRequest` with `redirect: 'manual'` (measured on 43.3.0):
 * a redirect raises `redirect` with the address it is going to, and the
 * request goes there only if `followRedirect()` is called inside that event;
 * otherwise the redirect is cancelled and the request fails with an `error`.
 * It never answers with a 3xx response.
 */
export class FakeNetRequest extends EventEmitter {
  /** Whether `followRedirect` was called during the `redirect` being raised. */
  private following = false
  aborted = false
  /** How many body chunks were delivered, for a test about where reading stops. */
  delivered = 0

  constructor(
    readonly options: { url: string; session?: unknown; redirect?: string; useSessionCookies?: boolean },
    private readonly net: FakeNet
  ) {
    super()
  }

  followRedirect(): void {
    this.following = true
  }
  abort(): void {
    if (this.aborted) return
    this.aborted = true
    queueMicrotask(() => this.emit('abort'))
  }
  end(): void {
    this.go(this.options.url)
  }

  private go(url: string): void {
    queueMicrotask(() => {
      if (this.aborted) return
      this.net.asked.push(url)
      const route = this.net.routes.get(url)
      if (route === undefined || route.refuse === true) {
        this.emit('error', new Error('net::ERR_CONNECTION_REFUSED'))
        return
      }
      if (route.hang === true) return
      if (route.redirect !== undefined) {
        const to = new URL(route.redirect, url).href
        this.following = false
        this.emit('redirect', route.status ?? 302, 'GET', to, { location: [to] })
        if (this.aborted) return
        if (!this.following) {
          this.emit('error', new Error('Redirect was cancelled'))
          return
        }
        this.go(to)
        return
      }
      const response = new FakeIncomingMessage(route.status ?? 200, route.headers ?? {})
      this.emit('response', response)
      const deliver = (chunk: Uint8Array): void => {
        this.delivered += 1
        response.emit('data', Buffer.from(chunk))
      }
      if (route.endless !== undefined) {
        // At most a thousand pieces: a reader that never stops fails its test
        // here rather than hanging it.
        for (let piece = 0; piece < 1000 && !this.aborted; piece += 1) deliver(new Uint8Array(route.endless))
        return
      }
      for (const chunk of route.chunks ?? []) {
        if (this.aborted) return
        deliver(chunk)
      }
      if (!this.aborted) response.emit('end')
    })
  }
}

/** Electron's `net`, answering from routes a test sets, and recording every address asked for. */
export class FakeNet {
  readonly routes = new Map<string, FakeRoute>()
  /** Every address the network was asked for, redirect hops included, in order. */
  readonly asked: string[] = []
  readonly requests: FakeNetRequest[] = []

  request(options: { url: string; session?: unknown; redirect?: string; useSessionCookies?: boolean }): FakeNetRequest {
    const request = new FakeNetRequest(options, this)
    this.requests.push(request)
    return request
  }

  /** Back to no routes and nothing asked. */
  reset(): void {
    this.routes.clear()
    this.asked.length = 0
    this.requests.length = 0
  }
}

export class FakeWebContents extends EventEmitter {
  readonly id = nextContentsId++
  /** Every address Helm asked this page to load, in order. */
  readonly loads: string[] = []
  readonly reloads: Array<'ordinary' | 'ignoring-cache'> = []
  readonly finds: Array<{ query: string; forward: boolean; findNext: boolean }> = []
  readonly stoppedFinding: string[] = []
  readonly evaluated: Array<{ source: string; userGesture: boolean }> = []
  /** Every input event Helm sent, in order: the pointer and the wheel as well as keys. */
  readonly inputs: Array<Record<string, unknown>> = []
  /** What `executeJavaScript` resolves with, or throws. */
  answer: (source: string) => unknown = () => undefined
  zoomFactor = 1

  private url = ''
  private title = ''
  private loading = false
  private destroyed = false
  private zoomLevel = 0
  private devtools = false
  private history: string[] = []
  private index = -1

  readonly navigationHistory = {
    canGoBack: (): boolean => this.index > 0,
    canGoForward: (): boolean => this.index < this.history.length - 1,
    goBack: (): void => this.travel(-1),
    goForward: (): void => this.travel(1)
  }

  getURL(): string {
    return this.url
  }
  getTitle(): string {
    return this.title
  }
  isLoading(): boolean {
    return this.loading
  }
  isDestroyed(): boolean {
    return this.destroyed
  }
  /**
   * Electron starts the navigation inside the call: `did-start-navigation`, with
   * no initiator, is raised before `loadURL` returns (measured on 43.3.0).
   */
  loadURL(url: string): Promise<void> {
    this.loads.push(url)
    this.startNavigation(url)
    return Promise.resolve()
  }
  /** How many times Helm stopped a load. */
  stopped = 0
  /**
   * `stop()` on 43.3.0: the load ends with `did-stop-loading` and nothing else -
   * no `did-fail-load` - and `getURL()` stays at the page that was there.
   */
  stop(): void {
    this.stopped += 1
    if (!this.loading) return
    this.loading = false
    this.emit('did-stop-loading')
  }
  reload(): void {
    this.reloads.push('ordinary')
  }
  reloadIgnoringCache(): void {
    this.reloads.push('ignoring-cache')
  }
  getZoomLevel(): number {
    return this.zoomLevel
  }
  setZoomLevel(level: number): void {
    this.zoomLevel = level
  }
  getZoomFactor(): number {
    return this.zoomFactor
  }
  setZoomFactor(factor: number): void {
    this.zoomFactor = factor
  }
  /** What main sent this page's renderer, in order: the menu layer's page is told what to draw this way. */
  readonly sent: Array<{ channel: string; payload: unknown }> = []
  send(channel: string, payload: unknown): void {
    this.sent.push({ channel, payload })
  }
  isDevToolsOpened(): boolean {
    return this.devtools
  }
  openDevTools(): void {
    this.devtools = true
  }
  closeDevTools(): void {
    this.devtools = false
  }
  /** How many times Helm moved keyboard focus into this page. */
  focused = 0
  /** Whether the page holds the keyboard, for `isFocused`. Set by the test. */
  hasFocus = false
  focus(): void {
    this.focused += 1
  }
  isFocused(): boolean {
    return this.hasFocus
  }
  /** What `capturePage` paints, as `toDataURL` would give it. Empty means an empty image. */
  picture = 'data:image/png;base64,cGFnZQ=='
  capturePage(): Promise<{ isEmpty: () => boolean; toDataURL: () => string }> {
    const picture = this.picture
    return Promise.resolve({ isEmpty: () => picture === '', toDataURL: () => picture })
  }
  findInPage(query: string, options: { forward?: boolean; findNext?: boolean }): number {
    this.finds.push({ query, forward: options.forward ?? true, findNext: options.findNext ?? false })
    return this.finds.length
  }
  stopFindInPage(action: string): void {
    this.stoppedFinding.push(action)
  }
  executeJavaScript(source: string, userGesture = false): Promise<unknown> {
    this.evaluated.push({ source, userGesture })
    try {
      return Promise.resolve(this.answer(source))
    } catch (err) {
      return Promise.reject(err instanceof Error ? err : new Error(String(err)))
    }
  }
  /**
   * Electron raises `before-input-event` for a key sent this way, synchronously
   * (measured on 43.3.0), so the fake does too - which is what lets a test see
   * that an agent's Ctrl+W is the page's.
   */
  sendInputEvent(event: { type: string; keyCode?: string; modifiers?: string[] } & Record<string, unknown>): void {
    this.inputs.push({ ...event })
    if ((event.type !== 'keyDown' && event.type !== 'keyUp') || event.keyCode === undefined) return
    const mods = event.modifiers ?? []
    const shift = mods.includes('shift')
    const key =
      event.keyCode.length === 1 ? (shift ? event.keyCode.toUpperCase() : event.keyCode.toLowerCase()) : event.keyCode
    this.press({
      key,
      type: event.type,
      control: mods.includes('control'),
      shift,
      alt: mods.includes('alt'),
      meta: mods.includes('meta')
    })
  }
  /** Helm closing the page. Electron reports the contents destroyed. */
  close(): void {
    if (this.destroyed) return
    this.destroyed = true
    this.emit('destroyed')
  }

  // -------------------------------------------------------------------------
  // Chromium's side, raised by the test
  // -------------------------------------------------------------------------

  /** The page named its icons, best first: `page-favicon-updated`. */
  reportFavicons(candidates: string[]): void {
    this.emit('page-favicon-updated', electronEvent(), candidates)
  }

  /** A response arrived and the page committed: `did-navigate` with its status. */
  commit(url: string, options: { status?: number; title?: string } = {}): void {
    this.history = [...this.history.slice(0, this.index + 1), url]
    this.index = this.history.length - 1
    this.arrive(url, options.status ?? 200, options.title ?? '')
  }

  /**
   * A load that failed. Chromium reports the failure and then commits an error
   * page at the same address, which is a main-frame navigation with no status
   * (`-1`) - the order Helm's `did-navigate` guard exists for. A load a newer
   * one superseded (`ERR_ABORTED`) commits nothing: `errorPage: false`.
   */
  failLoad(code: number, description: string, url: string, options: { errorPage?: boolean } = {}): void {
    this.loading = false
    this.emit('did-fail-load', electronEvent(), code, description, url, true)
    if (options.errorPage !== false) {
      this.url = url
      this.emit('did-navigate', electronEvent(), url, -1)
    }
    this.emit('did-stop-loading')
  }

  /**
   * A main-frame navigation starting. `initiator` is the address of the frame
   * that started it - a link clicked, a form sent, a script setting
   * `location` - and absent for one the browser started (`loadURL`, back,
   * forward), as Electron reports it.
   */
  startNavigation(
    url: string,
    options: { initiator?: string; sameDocument?: boolean; mainFrame?: boolean } = {}
  ): void {
    this.loading = true
    this.emit('did-start-navigation', {
      ...electronEvent(),
      url,
      isMainFrame: options.mainFrame ?? true,
      isSameDocument: options.sameDocument ?? false,
      initiator: options.initiator === undefined ? undefined : { url: options.initiator }
    })
  }

  /**
   * The server answered the navigation in flight with a redirect to `url`.
   * `initiator` is the frame that started that navigation, absent for one the
   * browser started.
   */
  redirect(url: string, options: { initiator?: string } = {}): void {
    this.emit('did-redirect-navigation', {
      ...electronEvent(),
      url,
      isMainFrame: true,
      isSameDocument: false,
      initiator: options.initiator === undefined ? undefined : { url: options.initiator }
    })
  }

  /** The page set its title after it arrived: `page-title-updated`. */
  sayTitle(title: string): void {
    this.title = title
    this.emit('page-title-updated', electronEvent(), title)
  }

  /** The page moved within itself - `pushState`, a hash change: `did-navigate-in-page`. */
  navigateInPage(url: string): void {
    this.url = url
    this.emit('did-navigate-in-page', electronEvent(), url, true)
  }

  /**
   * A load that ended with nothing committed: a 204, or a response turned into
   * a download. Only `did-stop-loading` says so (measured on 43.3.0).
   */
  endWithoutCommit(): void {
    this.loading = false
    this.emit('did-stop-loading')
  }

  /**
   * A key pressed with the page holding the keyboard. True when Helm swallowed
   * it (`preventDefault`), which is to say the page never saw it.
   */
  press(key: FakeKey): boolean {
    let prevented = false
    const input = {
      type: key.type ?? 'keyDown',
      key: key.key,
      control: key.control ?? false,
      shift: key.shift ?? false,
      alt: key.alt ?? false,
      meta: key.meta ?? false,
      isAutoRepeat: key.isAutoRepeat ?? false
    }
    this.emit('before-input-event', { preventDefault: () => (prevented = true) }, input)
    return prevented
  }

  /** The page called `requestFullscreen` and was granted it: Electron has made the window fullscreen. */
  enterFullscreen(): void {
    this.emit('enter-html-full-screen')
  }
  /** Escape, or `document.exitFullscreen()`. */
  leaveFullscreen(): void {
    this.emit('leave-html-full-screen')
  }

  /** A line the page wrote to its console, in Electron's event shape. */
  log(level: string, message: string, sourceId = '', lineNumber = 0): void {
    this.emit('console-message', { level, message, sourceId, lineNumber })
  }

  /** The page itself ended: what Electron does after `window.close()`. */
  destroyedByPage(): void {
    this.destroyed = true
    this.emit('destroyed')
  }

  private travel(step: number): void {
    const target = this.history[this.index + step]
    if (target === undefined) return
    this.index += step
    this.arrive(target, 200, this.title)
  }

  private arrive(url: string, status: number, title: string): void {
    this.url = url
    this.title = title
    this.loading = false
    this.emit('did-navigate', electronEvent(), url, status)
    this.emit('page-title-updated', electronEvent(), title)
    this.emit('did-stop-loading')
  }
}

type Bounds = { x: number; y: number; width: number; height: number }

export class FakeWebContentsView {
  /** Every view constructed, newest last. */
  static readonly created: FakeWebContentsView[] = []

  webContents: FakeWebContents | undefined
  readonly webPreferences: Record<string, unknown>
  visible = true
  private bounds: Bounds = { x: 0, y: 0, width: 0, height: 0 }

  /** `webContents` is Electron's adopt: the view shows contents somebody else made. */
  constructor(options: { webPreferences?: Record<string, unknown>; webContents?: FakeWebContents } = {}) {
    this.webPreferences = options.webPreferences ?? {}
    this.webContents = options.webContents ?? new FakeWebContents()
    FakeWebContentsView.created.push(this)
  }

  setBounds(bounds: Bounds): void {
    this.bounds = { ...bounds }
  }
  getBounds(): Bounds {
    return { ...this.bounds }
  }
  setVisible(visible: boolean): void {
    this.visible = visible
  }
  /** The ground last set, as Electron takes it. */
  background = ''
  setBackgroundColor(color: string): void {
    this.background = color
  }

  /**
   * The page called `window.close()`. Electron leaves `webContents` undefined
   * and reports the old contents destroyed. `announce: false` stops between
   * the two, which is the race `contentsOf` in `browser.ts` is written for.
   */
  closeItself(options: { announce?: boolean } = {}): FakeWebContents {
    const wc = this.webContents
    if (wc === undefined) throw new Error('closeItself: this view has already closed')
    this.webContents = undefined
    if (options.announce !== false) wc.destroyedByPage()
    return wc
  }
}

/**
 * A window: the app's own, handed to the host as `options.window`, or a popup
 * Electron made for a `window.open`.
 */
export class FakeBrowserWindow extends EventEmitter {
  readonly webContents = new FakeWebContents()
  readonly children: unknown[] = []
  readonly contentView = {
    // A view the window already holds is raised to the top, as Electron's is.
    addChildView: (view: unknown): void => {
      const at = this.children.indexOf(view)
      if (at >= 0) this.children.splice(at, 1)
      this.children.push(view)
    },
    removeChildView: (view: unknown): void => {
      const at = this.children.indexOf(view)
      if (at >= 0) this.children.splice(at, 1)
    }
  }
  title = ''
  contentBounds: Bounds = { x: 0, y: 0, width: 1600, height: 1000 }
  fullScreen = false
  private destroyed = false

  isFullScreen(): boolean {
    return this.fullScreen
  }
  setFullScreen(on: boolean): void {
    this.fullScreen = on
  }

  isDestroyed(): boolean {
    return this.destroyed
  }
  getContentBounds(): Bounds {
    return { ...this.contentBounds }
  }
  setMenu(): void {}
  setTitle(title: string): void {
    this.title = title
  }
  /** Closing it, by anybody: Electron emits `closed`. */
  destroy(): void {
    if (this.destroyed) return
    this.destroyed = true
    this.emit('closed')
  }
}

/** Everything the fake has been asked, for a test to read. */
export const fakeBrowser = {
  partitions: new Map<string, FakeSession>(),
  views: FakeWebContentsView.created,
  /** Addresses handed to the system browser through `shell.openExternal`. */
  openedExternally: [] as string[],
  /** `net.request`, answered from routes a test sets. */
  net: new FakeNet()
}

export function browserElectronFake(): Record<string, unknown> {
  return {
    WebContentsView: FakeWebContentsView,
    net: {
      request: (options: { url: string; session?: unknown; redirect?: string }) => fakeBrowser.net.request(options),
      fetch: () => Promise.reject(new Error('browserElectronFake: no network in tests'))
    },
    session: {
      fromPartition(name: string): FakeSession {
        let found = fakeBrowser.partitions.get(name)
        if (found === undefined) {
          found = new FakeSession()
          fakeBrowser.partitions.set(name, found)
        }
        return found
      }
    },
    shell: {
      openExternal: (url: string): Promise<void> => {
        fakeBrowser.openedExternally.push(url)
        return Promise.resolve()
      },
      openPath: () => Promise.resolve(''),
      showItemInFolder: (): void => undefined
    }
  }
}
