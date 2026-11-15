import { EventEmitter } from 'node:events'
import { afterEach, describe, expect, it, vi } from 'vitest'
import type { Cookie, CookiesGetFilter, CookiesSetDetails } from 'electron'
import {
  browserUserAgent,
  COOKIE_FLUSH_DEADLINE_MS,
  cookieUrl,
  flushCookies,
  keepSessionCookies,
  keepSignIns,
  keptCookie,
  SESSION_COOKIE_DAYS,
  type CookieJar
} from './browserCookies'

/**
 * The cookie store, played the way Chromium plays it: a cookie that replaces
 * another is announced as the old one's removal (cause `overwrite`) and then
 * the new one's addition, and `set` is announced the same way. A test can
 * hold the store's answer to `get`, to act while a rewrite is pending.
 */
class FakeStore extends EventEmitter {
  readonly held = new Map<string, Cookie>()
  readonly sets: CookiesSetDetails[] = []
  /** When set, `get` waits for the test to call `answer`. */
  holdAnswers = false
  private waiting: Array<() => void> = []
  failSets = false

  get(filter: CookiesGetFilter): Promise<Cookie[]> {
    const read = (): Cookie[] => [...this.held.values()].filter((cookie) => cookie.name === filter.name)
    if (!this.holdAnswers) return Promise.resolve(read())
    return new Promise((resolve) => this.waiting.push(() => resolve(read())))
  }

  /** Lets every held `get` answer with what the store holds now. */
  answer(): void {
    const waiting = this.waiting
    this.waiting = []
    for (const resolve of waiting) resolve()
  }

  set(details: CookiesSetDetails): Promise<void> {
    this.sets.push(details)
    if (this.failSets) return Promise.reject(new Error('refused'))
    const host = new URL(details.url).hostname
    this.put({
      name: details.name ?? '',
      value: details.value ?? '',
      domain: details.domain ?? host,
      hostOnly: details.domain === undefined,
      path: details.path ?? '/',
      secure: details.secure ?? false,
      httpOnly: details.httpOnly ?? false,
      session: details.expirationDate === undefined,
      ...(details.expirationDate === undefined ? {} : { expirationDate: details.expirationDate }),
      sameSite: details.sameSite ?? 'unspecified'
    })
    return Promise.resolve()
  }

  flushStore(): Promise<void> {
    return Promise.resolve()
  }

  /** A page setting a cookie. */
  put(cookie: Cookie): void {
    const key = keyOf(cookie)
    const old = this.held.get(key)
    if (old !== undefined) this.emit('changed', {}, old, 'overwrite', true)
    this.held.set(key, cookie)
    this.emit('changed', {}, cookie, 'inserted', false)
  }

  /** A page removing a cookie: signing out. */
  remove(cookie: Cookie): void {
    const key = keyOf(cookie)
    const old = this.held.get(key)
    if (old === undefined) return
    this.held.delete(key)
    this.emit('changed', {}, old, 'explicit', true)
  }
}

const keyOf = (cookie: Cookie): string => `${cookie.domain ?? ''}|${cookie.path ?? '/'}|${cookie.name}`

const jarOf = (store: FakeStore): CookieJar => store as unknown as CookieJar

/** A session cookie, host-only on app.example.com unless the test says otherwise. */
const sessionCookie = (fields: Partial<Cookie> = {}): Cookie => ({
  name: 'sid',
  value: 'v1',
  domain: 'app.example.com',
  hostOnly: true,
  path: '/',
  secure: true,
  httpOnly: true,
  session: true,
  sameSite: 'lax',
  ...fields
})

const NOW = Date.UTC(2026, 10, 15, 12, 0, 0)
const DAYS_OUT = Math.floor(NOW / 1000) + SESSION_COOKIE_DAYS * 24 * 60 * 60

/** Until every promise the keeper chained has run. */
const settle = async (): Promise<void> => {
  for (let i = 0; i < 10; i += 1) await Promise.resolve()
}

describe('keeping session cookies', () => {
  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('sets a session cookie again, the same but for an expiry 30 days out', async () => {
    const store = new FakeStore()
    keepSessionCookies(jarOf(store), () => NOW)
    store.put(sessionCookie())
    await settle()
    expect(store.sets).toEqual([
      {
        url: 'https://app.example.com/',
        name: 'sid',
        value: 'v1',
        path: '/',
        secure: true,
        httpOnly: true,
        expirationDate: DAYS_OUT,
        sameSite: 'lax'
      }
    ])
    const kept = store.held.get('app.example.com|/|sid')
    expect(kept).toMatchObject({ session: false, expirationDate: DAYS_OUT, hostOnly: true, value: 'v1' })
  })

  it('ignores its own rewrite: one set for one cookie', async () => {
    const store = new FakeStore()
    const changes = vi.fn()
    store.on('changed', changes)
    keepSessionCookies(jarOf(store), () => NOW)
    store.put(sessionCookie())
    await settle()
    // The page's addition, then the rewrite seen as a removal and an addition.
    expect(changes).toHaveBeenCalledTimes(3)
    expect(store.sets).toHaveLength(1)
  })

  it('leaves a cookie that already has an expiry alone', async () => {
    const store = new FakeStore()
    keepSessionCookies(jarOf(store), () => NOW)
    store.put(sessionCookie({ session: false, expirationDate: DAYS_OUT + 999 }))
    await settle()
    expect(store.sets).toEqual([])
  })

  it('never undoes a sign-out that lands while the rewrite is pending', async () => {
    const store = new FakeStore()
    store.holdAnswers = true
    keepSessionCookies(jarOf(store), () => NOW)
    const cookie = sessionCookie()
    store.put(cookie)
    store.remove(cookie)
    // The store answers as if the cookie were still there - an answer that
    // crossed the removal - and the rewrite still does not happen.
    store.held.set('app.example.com|/|sid', cookie)
    store.answer()
    await settle()
    expect(store.sets).toEqual([])
  })

  it('does not set a cookie the store no longer holds', async () => {
    const store = new FakeStore()
    store.holdAnswers = true
    keepSessionCookies(jarOf(store), () => NOW)
    store.put(sessionCookie())
    // Gone without the removal having reached the keeper yet.
    store.held.clear()
    store.answer()
    await settle()
    expect(store.sets).toEqual([])
  })

  it('removes a kept cookie for good when the site signs out, and Clear removes it too', async () => {
    const store = new FakeStore()
    keepSessionCookies(jarOf(store), () => NOW)
    store.put(sessionCookie())
    await settle()
    store.remove(store.held.get('app.example.com|/|sid')!)
    await settle()
    expect(store.held.size).toBe(0)
    expect(store.sets).toHaveLength(1)
  })

  it('keeps only the newest value when the page sets the cookie twice before the first rewrite', async () => {
    const store = new FakeStore()
    store.holdAnswers = true
    keepSessionCookies(jarOf(store), () => NOW)
    store.put(sessionCookie({ value: 'old' }))
    store.put(sessionCookie({ value: 'new' }))
    store.answer()
    await settle()
    expect(store.sets.map((set) => set.value)).toEqual(['new'])
    expect(store.held.get('app.example.com|/|sid')?.value).toBe('new')
  })

  it('re-arms the 30 days each time the page sets the cookie again', async () => {
    const store = new FakeStore()
    let now = NOW
    keepSessionCookies(jarOf(store), () => now)
    store.put(sessionCookie())
    await settle()
    now += 5 * 24 * 60 * 60 * 1000
    store.put(sessionCookie())
    await settle()
    expect(store.sets.map((set) => set.expirationDate)).toEqual([DAYS_OUT, DAYS_OUT + 5 * 24 * 60 * 60])
  })

  it('keeps a domain cookie on its domain and a host-only one host-only', async () => {
    const store = new FakeStore()
    keepSessionCookies(jarOf(store), () => NOW)
    store.put(sessionCookie({ name: 'wide', domain: '.example.com', hostOnly: false, path: '/a' }))
    store.put(sessionCookie({ name: '__Host-sid' }))
    await settle()
    const wide = store.sets.find((set) => set.name === 'wide')
    const host = store.sets.find((set) => set.name === '__Host-sid')
    expect(wide).toMatchObject({ url: 'https://example.com/a', domain: '.example.com', path: '/a' })
    expect(host).toMatchObject({ url: 'https://app.example.com/', path: '/', secure: true })
    expect(host).not.toHaveProperty('domain')
  })

  it('says nothing when the store refuses the rewrite, and keeps working', async () => {
    const store = new FakeStore()
    const said = [
      vi.spyOn(console, 'log'),
      vi.spyOn(console, 'info'),
      vi.spyOn(console, 'warn'),
      vi.spyOn(console, 'error'),
      vi.spyOn(console, 'debug')
    ]
    keepSessionCookies(jarOf(store), () => NOW)
    store.failSets = true
    store.put(sessionCookie())
    await settle()
    store.failSets = false
    store.put(sessionCookie({ name: 'other' }))
    await settle()
    expect(store.sets.map((set) => set.name)).toEqual(['sid', 'other'])
    for (const spy of said) expect(spy).not.toHaveBeenCalled()
  })

  it('stops when undone', async () => {
    const store = new FakeStore()
    const undo = keepSessionCookies(jarOf(store), () => NOW)
    undo()
    store.put(sessionCookie())
    await settle()
    expect(store.sets).toEqual([])
    expect(store.listenerCount('changed')).toBe(0)
  })
})

describe('the cookie a session cookie is kept as', () => {
  it('leaves out a same-site policy of unspecified', () => {
    expect(keptCookie(sessionCookie({ sameSite: 'unspecified' }), NOW)).not.toHaveProperty('sameSite')
    expect(keptCookie(sessionCookie({ sameSite: 'no_restriction' }), NOW).sameSite).toBe('no_restriction')
  })

  it('is set for its own scheme, domain and path', () => {
    expect(cookieUrl({ secure: false, domain: 'localhost', path: '/x/' })).toBe('http://localhost/x/')
    expect(cookieUrl({ secure: true, domain: '.example.com', path: '/' })).toBe('https://example.com/')
  })
})

describe('flushing the cookie store', () => {
  afterEach(() => {
    vi.useRealTimers()
  })

  it('resolves once the store is written', async () => {
    const flushStore = vi.fn(() => Promise.resolve())
    await flushCookies({ flushStore })
    expect(flushStore).toHaveBeenCalledTimes(1)
  })

  it('resolves when the store fails or throws', async () => {
    await expect(flushCookies({ flushStore: () => Promise.reject(new Error('gone')) })).resolves.toBeUndefined()
    await expect(
      flushCookies({
        flushStore: () => {
          throw new Error('destroyed')
        }
      })
    ).resolves.toBeUndefined()
  })

  it('gives up on a store that never answers, so a quit goes on', async () => {
    vi.useFakeTimers()
    let done = false
    void flushCookies({ flushStore: () => new Promise<void>(() => undefined) }).then(() => (done = true))
    await vi.advanceTimersByTimeAsync(COOKIE_FLUSH_DEADLINE_MS - 1)
    expect(done).toBe(false)
    await vi.advanceTimersByTimeAsync(1)
    expect(done).toBe(true)
  })
})

describe('the browser user agent', () => {
  const ELECTRON =
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Floti/2.4.1 Chrome/150.0.7871.212 Electron/43.3.0 Safari/537.36'

  it('drops the app and Electron tokens and keeps the Chrome version', () => {
    expect(browserUserAgent(ELECTRON, 'Floti')).toBe(
      'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.7871.212 Safari/537.36'
    )
  })

  it('matches an app name with spaces the way Electron writes it', () => {
    const agent = ELECTRON.replace('Floti/2.4.1', 'FlotiDev/2.4.1')
    expect(browserUserAgent(agent, 'Floti Dev')).not.toContain('FlotiDev')
  })

  it('leaves an agent that has neither token, and tokens that only start like them, as they are', () => {
    const chrome =
      'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/150.0.7871.212 Safari/537.36'
    expect(browserUserAgent(chrome, 'Floti')).toBe(chrome)
    expect(browserUserAgent(`${chrome} FlotiX/1`, 'Floti')).toBe(`${chrome} FlotiX/1`)
    expect(browserUserAgent(chrome, '')).toBe(chrome)
  })

  it('is what keepSignIns gives the partition, along with the cookie keeper', () => {
    const store = new FakeStore()
    let agent = ELECTRON
    keepSignIns(
      { cookies: jarOf(store), getUserAgent: () => agent, setUserAgent: (next: string) => (agent = next) },
      'Floti'
    )
    expect(agent).toBe(browserUserAgent(ELECTRON, 'Floti'))
    expect(store.listenerCount('changed')).toBe(1)
  })
})
